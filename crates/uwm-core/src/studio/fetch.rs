//! Fetching a known file from its publisher, on the owner's request, into a folder of theirs.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::catalog::KnownFile;
use crate::{Error, Result, Why};

const USER_AGENT: &str = concat!("UltimateWalkmanManager/", env!("CARGO_PKG_VERSION"));
/// A download that receives nothing for this long has stalled.
const IDLE: Duration = Duration::from_secs(60);
/// How often a waiting download looks at the cancel flag.
const TICK: Duration = Duration::from_millis(500);

fn network(e: ureq::Error) -> Error {
    Error::Network(match e {
        ureq::Error::StatusCode(code) => format!("the server answered {code}"),
        ureq::Error::HostNotFound => "no connection to the internet (host not found)".to_owned(),
        other => other.to_string(),
    })
}

/// The file name the download is saved under: the last part of its URL.
pub fn file_name(url: &str) -> Option<&str> {
    url.rsplit('/')
        .next()
        .filter(|n| !n.is_empty() && !n.contains(['?', '#', '\\']) && *n != "..")
}

/// Downloads `file` into `dir` and returns where it is. The partial download keeps a `.part`
/// name until the last byte is in and its sha256 is the pinned one, so an interrupted or
/// different download never looks like the file.
pub fn fetch(
    file: &KnownFile,
    dir: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf> {
    let url = file
        .url
        .as_deref()
        .ok_or_else(|| Error::unsupported(Why::NoDirectDownload, format!("{} has no direct download", file.id)))?;
    let name = file_name(url).ok_or_else(|| {
        Error::unsupported(Why::NoFileName, format!("no file name in {url}")).with_detail(url.to_string())
    })?;
    download(
        url,
        file.bytes,
        file.sha256.as_deref(),
        &dir.join(name),
        cancel,
        on_progress,
    )
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .user_agent(USER_AGENT)
        .https_only(true)
        .build();
    ureq::Agent::new_with_config(config)
}

/// A small text document, such as a licence manifest.
pub fn text(url: &str) -> Result<String> {
    agent()
        .get(url)
        .call()
        .map_err(network)?
        .body_mut()
        .with_config()
        .limit(16 << 20)
        .read_to_string()
        .map_err(network)
}

/// Downloads `url` to `dest`, through `<dest>.part`, checking the size and sha256 when they
/// are pinned.
pub fn download(
    url: &str,
    bytes: Option<u64>,
    sha256: Option<&str>,
    dest: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf> {
    let dir = dest.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);

    let resp = agent().get(url).call().map_err(network)?;
    let total = resp.body().content_length().or(bytes).unwrap_or(0);
    let body = resp.into_body().into_with_config().limit(u64::MAX).reader();
    let mut out = File::create(&part)?;
    let copied = receive(body, &mut out, cancel, IDLE, &mut |done| on_progress(done, total)).and_then(|(done, got)| {
        out.sync_all()?;
        if let Some(expected) = bytes.filter(|&b| b != done) {
            return Err(Error::Network(format!(
                "the download ended at {done} of {expected} bytes"
            )));
        }
        if let Some(pinned) = sha256.filter(|&p| p != got) {
            return Err(
                Error::unsupported(Why::NotPinnedFile, format!("{url}: sha256 {got}, pinned {pinned}"))
                    .with_detail(url.to_owned()),
            );
        }
        Ok(())
    });
    drop(out);
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, dest)?;
    Ok(dest.to_path_buf())
}

/// Copies `body` into `out` and returns the bytes and their sha256. The body is read on a
/// thread of its own, because a read from a stalled connection blocks with no timeout of its
/// own: the copy gives up after `idle` without data or at once on `cancel`, and that thread
/// ends when its read does.
fn receive(
    mut body: impl Read + Send + 'static,
    out: &mut impl Write,
    cancel: &AtomicBool,
    idle: Duration,
    on_progress: &mut dyn FnMut(u64),
) -> Result<(u64, String)> {
    let (tx, rx) = mpsc::sync_channel::<std::io::Result<Vec<u8>>>(4);
    thread::Builder::new().name("download".into()).spawn(move || {
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let read = body.read(&mut buf).map(|n| buf[..n].to_vec());
            let end = !matches!(&read, Ok(chunk) if !chunk.is_empty());
            if tx.send(read).is_err() || end {
                break;
            }
        }
    })?;
    let mut hash = Sha256::new();
    let mut done = 0u64;
    let mut last = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        match rx.recv_timeout(TICK.min(idle)) {
            Ok(Ok(chunk)) if chunk.is_empty() => break,
            Ok(Ok(chunk)) => {
                out.write_all(&chunk)?;
                hash.update(&chunk);
                done += chunk.len() as u64;
                last = Instant::now();
                on_progress(done);
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(RecvTimeoutError::Timeout) if last.elapsed() >= idle => {
                return Err(Error::Network(format!(
                    "nothing arrived for {} s after {done} bytes",
                    idle.as_secs()
                )));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Error::Network(format!("the download stopped after {done} bytes")));
            }
        }
    }
    Ok((done, hash.finalize().iter().map(|b| format!("{b:02x}")).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body whose server sent some bytes and then stopped answering.
    struct Stalled(Option<Vec<u8>>);

    impl Read for Stalled {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.0.take() {
                Some(first) => {
                    buf[..first.len()].copy_from_slice(&first);
                    Ok(first.len())
                }
                None => loop {
                    thread::park();
                },
            }
        }
    }

    #[test]
    fn hashes_what_it_receives() {
        let mut out = Vec::new();
        let never = AtomicBool::new(false);
        let (done, sha) = receive(&b"abc"[..], &mut out, &never, IDLE, &mut |_| {}).unwrap();
        assert_eq!((done, out.as_slice()), (3, &b"abc"[..]));
        assert_eq!(sha, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn gives_up_on_a_stalled_body() {
        let never = AtomicBool::new(false);
        let started = Instant::now();
        let stalled = receive(
            Stalled(Some(vec![1; 10])),
            &mut Vec::new(),
            &never,
            Duration::from_millis(300),
            &mut |_| {},
        );
        assert!(matches!(stalled, Err(Error::Network(m)) if m.contains("after 10 bytes")));
        assert!(started.elapsed() < Duration::from_secs(5));

        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let cancelled = thread::scope(|s| {
            s.spawn(|| {
                thread::sleep(Duration::from_millis(200));
                cancel.store(true, Ordering::Relaxed);
            });
            receive(Stalled(None), &mut Vec::new(), &cancel, IDLE, &mut |_| {})
        });
        assert!(matches!(cancelled, Err(Error::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn names_the_file_after_the_url() {
        assert_eq!(
            file_name("https://info.update.sony.net/PA001/x/NW-ZX500_0000_V4_06_00_NW_WM_FW.UPG"),
            Some("NW-ZX500_0000_V4_06_00_NW_WM_FW.UPG")
        );
        assert_eq!(file_name("https://androidfilehost.com/?fid=1"), None);
        assert_eq!(file_name("https://example.com/a/"), None);
        assert_eq!(file_name("https://example.com/.."), None);
    }
}
