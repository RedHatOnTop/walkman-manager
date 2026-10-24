//! Makes SensMe analysis for music Music Center for PC never saw. The analysis is
//! Sony's own engine, `AVLib/MMLib11.dll` from the user's Music Center for PC install,
//! run by mmhost.exe (`mmhost/mmhost.c`, built into this program): natively on Windows,
//! under Wine elsewhere. Nothing of Sony's is part of this program.
//!
//! The engine is given PCM the way Music Center gives it, because the block depends on
//! it: ffmpeg's decoding as 44.1 kHz 16-bit stereo, MP3 without gapless trimming and
//! with the Xing/Info frame as a frame of silence when it directly follows the tag, in
//! 4096-byte pieces.

use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use lofty::file::FileType;
use serde::Serialize;

use super::retag::{OnPlayer, Rewrite, rewrite_computer, rewrite_player};
use super::sensme::{self, SensMe};
use super::transfer::{Progress, Report};
use super::{tagbytes, tags};
use crate::error::{Error, Result, Why};

const HOST_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mmhost.exe"));
const RATE: u32 = 44_100;
const CHANNELS: u32 = 2;
const BITS: u32 = 16;
const PIECE: usize = 4096;
/// One MPEG-1 Layer III frame of 16-bit stereo silence.
const SILENT_FRAME: usize = 1152 * 4;
/// The engine's class id as it is stored in the DLL; a file without it is not the engine.
const CLSID_MUSIC_ANALYSIS2: [u8; 16] = [
    0x2d, 0xed, 0xe9, 0x8d, 0xfb, 0x8c, 0x33, 0x4b, 0xb8, 0xe3, 0xce, 0xe0, 0x6f, 0x29, 0x03, 0xe3,
];
const ENGINE_IN_INSTALL: &str = "Sony/Music Center/AVLib/MMLib11.dll";
/// Long enough for Wine to set up a new prefix before the first answer.
const FIRST_ANSWER: Duration = Duration::from_secs(600);
const ANSWER: Duration = Duration::from_secs(180);

/// What the analysis runs with, for the UI to show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub dll: String,
    /// Wine, or `None` where the engine runs natively.
    pub wine: Option<String>,
    pub ffmpeg: String,
}

#[derive(Debug, Clone)]
pub struct Engine {
    dll: PathBuf,
    host: PathBuf,
    wine: Option<PathBuf>,
    prefix: PathBuf,
    ffmpeg: PathBuf,
}

/// Whether `path` is Music Center's analysis engine.
pub fn is_engine(path: &Path) -> bool {
    fs::read(path).is_ok_and(|d| {
        d.starts_with(b"MZ")
            && d.windows(CLSID_MUSIC_ANALYSIS2.len())
                .any(|w| w == CLSID_MUSIC_ANALYSIS2)
    })
}

/// Where Music Center for PC puts the engine: Windows' own Program Files, or a Wine
/// prefix's (`~/.wine`, Bottles, Lutris, and this program's own).
fn engine_candidates(work: &Path) -> Vec<PathBuf> {
    let mut drives: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(p) = std::env::var_os(var) {
                drives.push(PathBuf::from(p).join(ENGINE_IN_INSTALL));
            }
        }
        return drives;
    }
    let mut prefixes = vec![work.join("wine")];
    if let Some(home) = dirs::home_dir() {
        prefixes.push(home.join(".wine"));
        for bottles in [
            ".var/app/com.usebottles.bottles/data/bottles/bottles",
            ".local/share/bottles/bottles",
            ".local/share/wineprefixes",
            "Games",
        ] {
            prefixes.extend(subdirs(&home.join(bottles)));
        }
    }
    for prefix in prefixes {
        for pf in ["Program Files (x86)", "Program Files"] {
            drives.push(prefix.join("drive_c").join(pf).join(ENGINE_IN_INSTALL));
        }
    }
    drives
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out
}

/// Wine from PATH, else the newest runner Bottles or Lutris installed.
fn find_wine() -> Option<PathBuf> {
    if let Ok(w) = which::which("wine") {
        return Some(w);
    }
    let home = dirs::home_dir()?;
    [
        ".var/app/com.usebottles.bottles/data/bottles/runners",
        ".local/share/bottles/runners",
        ".local/share/lutris/runners/wine",
    ]
    .iter()
    .flat_map(|d| subdirs(&home.join(d)).into_iter().rev())
    .map(|r| r.join("bin/wine"))
    .find(|w| w.is_file())
}

/// The first engine found where Music Center installs it.
pub fn find_dll(work: &Path) -> Option<PathBuf> {
    engine_candidates(work).into_iter().find(|p| is_engine(p))
}

impl Engine {
    /// The engine at `dll`, or the first one found in the usual places. `work` is a
    /// folder of this program's own: the helper and Wine's prefix are kept there.
    pub fn locate(dll: Option<&Path>, work: &Path) -> Result<Engine> {
        if HOST_EXE.is_empty() {
            return Err(Error::unsupported(
                Why::NoAnalysisHelper,
                "this build of the manager was made without its analysis helper (mmhost.exe)",
            ));
        }
        let dll = match dll {
            Some(p) if is_engine(p) => p.to_path_buf(),
            Some(p) => {
                return Err(Error::unsupported(
                    Why::NotAnalysisEngine,
                    format!(
                        "{} is not Music Center's analysis engine (AVLib/MMLib11.dll)",
                        p.display()
                    ),
                )
                .with_detail(p.display().to_string()));
            }
            None => find_dll(work).ok_or_else(|| {
                Error::unsupported(
                    Why::NoMusicCenter,
                    "Music Center for PC was not found; install it, or choose its AVLib/MMLib11.dll",
                )
            })?,
        };
        let wine = if cfg!(windows) {
            None
        } else {
            Some(find_wine().ok_or_else(|| {
                Error::unsupported(
                    Why::NoWine,
                    "Wine was not found; it runs Music Center's analysis engine here",
                )
            })?)
        };
        let ffmpeg = which::which("ffmpeg").map_err(|_| {
            Error::unsupported(
                Why::NoFfmpeg,
                "ffmpeg was not found; it decodes the music for the analysis",
            )
        })?;
        fs::create_dir_all(work)?;
        let host = work.join("mmhost.exe");
        if fs::read(&host).ok().as_deref() != Some(HOST_EXE) {
            let part = work.join("mmhost.exe.uwm-part");
            fs::write(&part, HOST_EXE)?;
            fs::rename(&part, &host)?;
        }
        Ok(Engine {
            dll,
            host,
            wine,
            prefix: work.join("wine"),
            ffmpeg,
        })
    }

    pub fn info(&self) -> EngineInfo {
        EngineInfo {
            dll: self.dll.display().to_string(),
            wine: self.wine.as_ref().map(|w| w.display().to_string()),
            ffmpeg: self.ffmpeg.display().to_string(),
        }
    }

    /// Starts the helper. One session analyses any number of files, one at a time.
    pub fn start(&self) -> Result<Session> {
        let mut cmd = match &self.wine {
            Some(wine) => {
                let mut c = Command::new(wine);
                c.arg(&self.host)
                    .arg(windows_path(&self.dll))
                    .env("WINEPREFIX", &self.prefix)
                    .env("WINEDEBUG", "-all")
                    // No Mono or Gecko prompts and no menu entries for the new prefix.
                    .env("WINEDLLOVERRIDES", "mscoree,mshtml=;winemenubuilder.exe=d");
                c
            }
            None => {
                let mut c = Command::new(&self.host);
                c.arg(&self.dll);
                c
            }
        };
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let tool = if self.wine.is_some() { "Wine" } else { "mmhost" };
        let mut child = cmd.spawn().map_err(|source| Error::Spawn { tool, source })?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let log = spool(child.stderr.take().expect("piped stderr"));
        Ok(Session {
            child: Arc::new(Mutex::new(child)),
            stdin: Some(stdin),
            stdout,
            log,
            ffmpeg: self.ffmpeg.clone(),
            answered: false,
        })
    }
}

/// Wine maps the Unix root as drive Z:.
fn windows_path(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    format!("Z:{}", abs.display().to_string().replace('/', "\\"))
}

/// Keeps the last few kilobytes a child writes to stderr.
fn spool(mut from: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&log);
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n @ 1..) = from.read(&mut buf) {
            let mut l = into.lock().unwrap_or_else(|e| e.into_inner());
            l.extend_from_slice(&buf[..n]);
            let excess = l.len().saturating_sub(8192);
            l.drain(..excess);
        }
    });
    log
}

fn last_lines(log: &Mutex<Vec<u8>>) -> String {
    let l = log.lock().unwrap_or_else(|e| e.into_inner());
    let text = String::from_utf8_lossy(&l);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" / ")
}

/// An MP3 whose first frame, right after the tag, is a Xing, Info or VBRI header.
/// Music Center's decoder plays that frame as silence; when anything sits between the
/// tag and the frame it skips it.
fn leading_info_frame(path: &Path) -> bool {
    let Ok(mut f) = File::open(path) else {
        return false;
    };
    let Ok(tag) = tagbytes::head_of(&mut f, FileType::Mpeg) else {
        return false;
    };
    let mut frame = Vec::with_capacity(200);
    if f.seek(SeekFrom::Start(tag.len() as u64)).is_err() || f.take(200).read_to_end(&mut frame).is_err() {
        return false;
    }
    frame.len() >= 4
        && frame[0] == 0xff
        && frame[1] & 0xe0 == 0xe0
        && frame.windows(4).any(|w| matches!(w, b"Xing" | b"Info" | b"VBRI"))
}

pub struct Session {
    child: Arc<Mutex<Child>>,
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
    log: Arc<Mutex<Vec<u8>>>,
    ffmpeg: PathBuf,
    answered: bool,
}

impl Session {
    /// The SMFMF block Music Center's engine makes for `path`.
    pub fn analyse(&mut self, path: &Path, cancel: &AtomicBool) -> Result<Vec<u8>> {
        let mp3 = FileType::from_path(path) == Some(FileType::Mpeg);
        let mut cmd = Command::new(&self.ffmpeg);
        cmd.args(["-nostdin", "-v", "error"]);
        if mp3 {
            cmd.args(["-flags2", "+skip_manual"]);
        }
        cmd.arg("-i").arg(path).args([
            "-map",
            "0:a:0",
            "-f",
            "s16le",
            "-ac",
            &CHANNELS.to_string(),
            "-ar",
            &RATE.to_string(),
            "-",
        ]);
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut decoder = cmd.spawn().map_err(|source| Error::Spawn { tool: "ffmpeg", source })?;
        let pcm = decoder.stdout.take().expect("piped stdout");
        let decoder_log = spool(decoder.stderr.take().expect("piped stderr"));

        let (done, watch) = mpsc::channel::<()>();
        let limit = if self.answered { ANSWER } else { FIRST_ANSWER };
        let child = Arc::clone(&self.child);
        let stopped = Arc::new(AtomicBool::new(false));
        let timed_out = Arc::clone(&stopped);
        thread::spawn(move || {
            if watch.recv_timeout(limit) == Err(mpsc::RecvTimeoutError::Timeout) {
                timed_out.store(true, Ordering::Relaxed);
                let _ = child.lock().unwrap_or_else(|e| e.into_inner()).kill();
            }
        });

        let fed = self.feed(pcm, mp3 && leading_info_frame(path), cancel);
        let _ = decoder.kill();
        let decoded = decoder.wait().ok().is_some_and(|s| s.success());
        let answer = fed.and_then(|bytes| self.answer().map(|a| (bytes, a)));
        let _ = done.send(());
        let (bytes, (hr, blob)) = answer.map_err(|e| {
            if stopped.load(Ordering::Relaxed) {
                Error::Timeout {
                    tool: "the analysis engine",
                    secs: limit.as_secs(),
                }
            } else {
                match e {
                    Error::Io(_) => Error::Failed {
                        tool: "the analysis engine",
                        detail: format!("it stopped: {}", last_lines(&self.log)),
                    },
                    e => e,
                }
            }
        })?;
        self.answered = true;
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        if !decoded || bytes == 0 {
            return Err(Error::Failed {
                tool: "ffmpeg",
                detail: match last_lines(&decoder_log) {
                    s if s.is_empty() => "no audio could be decoded".to_owned(),
                    s => s,
                },
            });
        }
        if hr != 0 {
            return Err(Error::Failed {
                tool: "the analysis engine",
                detail: format!("it gave no analysis (HRESULT {:#010x})", hr as u32),
            });
        }
        if sensme::decode(&blob).is_none() {
            return Err(Error::Failed {
                tool: "the analysis engine",
                detail: "its analysis is not one the player would accept".to_owned(),
            });
        }
        Ok(blob)
    }

    /// False once the helper has stopped (after a timeout, say); start a new session.
    pub fn alive(&self) -> bool {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        matches!(child.try_wait(), Ok(None))
    }

    /// Streams one job to the helper; the PCM bytes sent. A cancel ends the job early
    /// (the engine still answers, and the answer is dropped).
    fn feed(&mut self, pcm: impl Read, silent_frame: bool, cancel: &AtomicBool) -> Result<u64> {
        let stdin = self.stdin.as_mut().expect("open until the session is dropped");
        let mut w = std::io::BufWriter::with_capacity(1 << 16, stdin);
        w.write_all(b"UWMJ")?;
        for v in [RATE, CHANNELS, BITS] {
            w.write_all(&v.to_le_bytes())?;
        }
        let zeros = [0u8; SILENT_FRAME];
        let lead: &[u8] = if silent_frame { &zeros } else { &[] };
        let mut source = BufReader::with_capacity(1 << 16, lead.chain(pcm));
        let mut piece = [0u8; PIECE];
        let mut sent = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let mut n = 0;
            while n < PIECE {
                match source.read(&mut piece[n..])? {
                    0 => break,
                    k => n += k,
                }
            }
            if n == 0 {
                break;
            }
            w.write_all(&(n as u32).to_le_bytes())?;
            w.write_all(&piece[..n])?;
            sent += n as u64;
            if n < PIECE {
                break;
            }
        }
        w.write_all(&0u32.to_le_bytes())?;
        w.flush()?;
        Ok(sent.saturating_sub(lead.len() as u64))
    }

    fn answer(&mut self) -> Result<(i32, Vec<u8>)> {
        let mut head = [0u8; 12];
        self.stdout.read_exact(&mut head)?;
        if &head[..4] != b"UWMR" {
            return Err(Error::Failed {
                tool: "the analysis engine",
                detail: "its helper answered out of step".to_owned(),
            });
        }
        let hr = i32::from_le_bytes(head[4..8].try_into().expect("4 bytes"));
        let n = u32::from_le_bytes(head[8..12].try_into().expect("4 bytes"));
        let mut blob = vec![0u8; n as usize];
        self.stdout.read_exact(&mut blob)?;
        Ok((hr, blob))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        for _ in 0..50 {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// A track to analyse, on the computer or on a player.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyseItem {
    pub track_id: String,
    pub path: String,
    /// The path shown and kept below the backup folder.
    pub rel: String,
    pub size: u64,
}

/// Whether the analysis can be written into a file of this name.
pub fn writable(path: &str) -> bool {
    matches!(FileType::from_path(path), Some(FileType::Mpeg | FileType::Flac))
}

/// Analyses each file and writes the analysis into it: where it is on the computer, or
/// on a player through `on_player` (backed up and checked like a tag edit). Gives the
/// report and, by item, the analysis that was written.
pub fn run(
    engine: &Engine,
    items: &[AnalyseItem],
    on_player: Option<OnPlayer>,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
) -> (Report, Vec<Option<SensMe>>) {
    let files: Vec<Rewrite> = items
        .iter()
        .map(|i| Rewrite {
            path: &i.path,
            rel: &i.rel,
            size: i.size,
        })
        .collect();
    let mut found = vec![None; items.len()];
    let mut session: Option<Session> = None;
    let mut apply = |n: usize, file: &Path| -> Result<()> {
        if !session.as_ref().is_some_and(Session::alive) {
            session = Some(engine.start()?);
        }
        let blob = session.as_mut().expect("started above").analyse(file, cancel)?;
        tags::write_smfmf(file, &blob)?;
        found[n] = sensme::decode(&blob);
        Ok(())
    };
    let report = match on_player {
        Some(at) => rewrite_player(at, &files, cancel, on_progress, &mut apply),
        None => rewrite_computer(&files, cancel, on_progress, &mut apply),
    };
    (report, found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, data: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uwm-analyse-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::write(&p, data).unwrap();
        p
    }

    fn info_frame(tag: &[u8]) -> Vec<u8> {
        let mut f = vec![0xff, 0xfb, 0x90, 0x64];
        f.extend(std::iter::repeat_n(0u8, 32));
        f.extend_from_slice(tag);
        f.extend(std::iter::repeat_n(0u8, 417 - f.len()));
        f
    }

    #[test]
    fn finds_the_info_frame_only_right_after_the_tag() {
        let id3 = |body: usize| {
            let mut t = b"ID3\x03\x00\x00".to_vec();
            t.extend_from_slice(&[0, 0, (body >> 7) as u8 & 0x7f, body as u8 & 0x7f]);
            t.extend(std::iter::repeat_n(0u8, body));
            t
        };
        let right_after = [id3(100), info_frame(b"Info")].concat();
        assert!(leading_info_frame(&temp("a.mp3", &right_after)));
        let no_tag = info_frame(b"Xing");
        assert!(leading_info_frame(&temp("b.mp3", &no_tag)));
        let junk_between = [id3(100), vec![0x55; 170], info_frame(b"Info")].concat();
        assert!(!leading_info_frame(&temp("c.mp3", &junk_between)));
        let plain = [id3(100), info_frame(b"\0\0\0\0")].concat();
        assert!(!leading_info_frame(&temp("d.mp3", &plain)));
    }

    #[test]
    fn knows_the_engine_by_its_class() {
        let mut dll = b"MZ".to_vec();
        dll.extend(std::iter::repeat_n(0u8, 300));
        dll.extend_from_slice(&CLSID_MUSIC_ANALYSIS2);
        assert!(is_engine(&temp("MMLib11.dll", &dll)));
        assert!(!is_engine(&temp("other.dll", &dll[..200])));
        if cfg!(unix) {
            assert_eq!(
                windows_path(Path::new("/home/a/Music Center/MMLib11.dll")),
                "Z:\\home\\a\\Music Center\\MMLib11.dll"
            );
        }
    }
}
