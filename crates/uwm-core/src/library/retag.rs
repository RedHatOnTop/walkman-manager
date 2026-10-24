//! Writes tags into an album's files where they are. On the computer each file is
//! rewritten through a copy beside it. On a player each file is first copied off and
//! checked against the player's own checksum, the copy is kept as the backup, and the
//! file is rewritten in place so the player's media database keeps its row (and the
//! playlists that point at it).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use md5::{Digest, Md5};
use serde::Serialize;

use super::remote::shell_quote;
use super::tags::{self, Cover, TagEdit};
use super::transfer::{Failure, Progress, Report, top_folders};
use crate::adb::Adb;
use crate::error::{Error, ErrorReport, Result, Why};

const MIN_BYTES_PER_SEC: u64 = 1024 * 1024;
/// Shell-writable and outside the media folders, so the scanner never sees half a file.
const ANDROID_STAGING: &str = "/data/local/tmp";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetagItem {
    pub track_id: String,
    /// Where the file is: a path on the computer, or on the player.
    pub path: String,
    /// The path shown and kept below the backup folder.
    pub rel: String,
    pub size: u64,
    pub edit: TagEdit,
    pub cover: bool,
}

#[derive(Debug, Clone)]
pub enum Place {
    Computer,
    Player {
        device_id: String,
        adb_target: String,
        android: bool,
    },
}

#[derive(Debug, Clone)]
pub struct RetagJob {
    pub id: String,
    pub place: Place,
    pub items: Vec<RetagItem>,
    pub cover: Option<Cover>,
}

fn md5_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path)?;
    let mut h = Md5::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn remote_md5(adb: &Adb, serial: &str, path: &str) -> Result<String> {
    let out = adb.shell(
        serial,
        &format!("md5sum {}", shell_quote(path)),
        Duration::from_secs(90),
    )?;
    let sum = out.split_whitespace().next().unwrap_or("");
    if sum.len() == 32 && sum.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(sum.to_ascii_lowercase())
    } else {
        Err(Error::Failed {
            tool: "adb",
            detail: format!("md5sum on the player answered: {}", out.trim()),
        })
    }
}

/// A file a job rewrites: where it is, and the path shown and kept below the backup
/// folder.
#[derive(Debug, Clone, Copy)]
pub struct Rewrite<'a> {
    pub path: &'a str,
    pub rel: &'a str,
    pub size: u64,
}

fn rewrites(job: &RetagJob) -> Vec<Rewrite<'_>> {
    job.items
        .iter()
        .map(|i| Rewrite {
            path: &i.path,
            rel: &i.rel,
            size: i.size,
        })
        .collect()
}

fn write_item(job: &RetagJob, n: usize, file: &Path) -> Result<()> {
    let item = &job.items[n];
    tags::write(file, &item.edit, job.cover.as_ref().filter(|_| item.cover))
}

/// Rewrites files on the computer.
pub fn run_computer(job: &RetagJob, cancel: &AtomicBool, on_progress: &mut dyn FnMut(&Progress)) -> Report {
    rewrite_computer(&rewrites(job), cancel, on_progress, &mut |n, file| {
        write_item(job, n, file)
    })
}

fn progress_of(files: &[Rewrite]) -> Progress {
    Progress {
        total_files: files.len(),
        total_bytes: files.iter().map(|f| f.size).sum(),
        ..Progress::default()
    }
}

/// Has `apply` rewrite each file where it is; `apply` gets the file's number and path.
pub fn rewrite_computer(
    files: &[Rewrite],
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
    apply: &mut dyn FnMut(usize, &Path) -> Result<()>,
) -> Report {
    let mut progress = progress_of(files);
    let mut report = Report::default();
    for (n, file) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        progress.current = Some(file.rel.to_owned());
        on_progress(&progress);
        match apply(n, Path::new(file.path)) {
            Ok(()) => {
                report.copied += 1;
                report.verified += 1;
                report.bytes += file.size;
            }
            Err(Error::Cancelled) => {
                report.cancelled = true;
                break;
            }
            Err(e) => {
                progress.failed += 1;
                report.failures.push(Failure {
                    dest: file.rel.to_owned(),
                    error: ErrorReport::from(&e),
                });
            }
        }
        progress.done_files += 1;
        progress.done_bytes += file.size;
        on_progress(&progress);
    }
    progress.current = None;
    on_progress(&progress);
    report
}

/// Puts `local` in place of `remote` on the player, writing into the existing file.
fn replace_remote(adb: &Adb, serial: &str, android: bool, local: &Path, remote: &str, n: usize) -> Result<()> {
    let staged = if android {
        format!("{ANDROID_STAGING}/uwm-retag-{n}.part")
    } else {
        format!("{remote}.uwm-part")
    };
    let size = fs::metadata(local)?.len();
    let timeout = Duration::from_secs(60 + size / MIN_BYTES_PER_SEC);
    let never = AtomicBool::new(false);
    adb.push(serial, local, &staged, timeout, &never)?;
    let script = format!(
        "cat {s} > {r}; status=$?; rm -f {s}; exit $status",
        s = shell_quote(&staged),
        r = shell_quote(remote)
    );
    adb.shell(serial, &script, timeout)?;
    Ok(())
}

/// Rewrites files on a player.
pub fn run_player(
    adb: &Adb,
    job: &RetagJob,
    work: &Path,
    backup: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
) -> Report {
    match &job.place {
        Place::Player {
            adb_target, android, ..
        } => rewrite_player(
            OnPlayer {
                adb,
                serial: adb_target,
                android: *android,
                work,
                backup,
            },
            &rewrites(job),
            cancel,
            on_progress,
            &mut |n, file| write_item(job, n, file),
        ),
        Place::Computer => run_computer(job, cancel, on_progress),
    }
}

/// Where a player job works: the player, a scratch folder and the run's backup folder.
#[derive(Clone, Copy)]
pub struct OnPlayer<'a> {
    pub adb: &'a Adb,
    pub serial: &'a str,
    pub android: bool,
    pub work: &'a Path,
    pub backup: &'a Path,
}

/// Has `apply` rewrite each file of a player, on a copy. Each original is copied into
/// `backup` and checked by checksum before anything is written, the copy is written in
/// place of the file so the player's media database keeps its row, and a file that does
/// not arrive intact is put back from the backup.
pub fn rewrite_player(
    at: OnPlayer,
    files: &[Rewrite],
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
    apply: &mut dyn FnMut(usize, &Path) -> Result<()>,
) -> Report {
    let OnPlayer {
        adb,
        serial,
        android,
        work,
        backup,
    } = at;
    let mut progress = progress_of(files);
    let mut report = Report::default();
    let mut written: Vec<&str> = Vec::new();
    for (n, item) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        progress.current = Some(item.rel.to_owned());
        on_progress(&progress);
        let kept = backup.join(item.rel.trim_start_matches('/'));
        let scratch = work.join(format!(
            "{n}.{}",
            Path::new(item.rel)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("bin")
        ));
        let result = (|| -> Result<()> {
            let before = remote_md5(adb, serial, item.path)?;
            if let Some(dir) = kept.parent() {
                fs::create_dir_all(dir)?;
            }
            let timeout = Duration::from_secs(60 + item.size / MIN_BYTES_PER_SEC);
            adb.pull(serial, item.path, &kept, timeout, cancel)?;
            if md5_file(&kept)? != before {
                let _ = fs::remove_file(&kept);
                return Err(Error::unsupported(
                    Why::BackupMismatch,
                    "the backup copy did not match the file on the player; nothing was written",
                ));
            }
            fs::create_dir_all(work)?;
            fs::copy(&kept, &scratch)?;
            apply(n, &scratch)?;
            let want = md5_file(&scratch)?;
            let put = replace_remote(adb, serial, android, &scratch, item.path, n);
            let after = put.and_then(|_| remote_md5(adb, serial, item.path));
            if after.as_deref().ok() == Some(want.as_str()) {
                return Ok(());
            }
            let why = match after {
                Err(e) => e.to_string(),
                Ok(_) => "the rewritten file did not arrive intact".to_owned(),
            };
            let restored = replace_remote(adb, serial, android, &kept, item.path, n)
                .and_then(|_| remote_md5(adb, serial, item.path))
                .is_ok_and(|sum| sum == before);
            Err(if restored {
                Error::unsupported(Why::OriginalRestored, format!("{why}; the original was put back")).with_detail(why)
            } else {
                Error::unsupported(
                    Why::OriginalKept,
                    format!(
                        "{why}; the original could not be put back and is kept at {}",
                        kept.display()
                    ),
                )
                .with_detail(kept.display().to_string())
            })
        })();
        let _ = fs::remove_file(&scratch);
        match result {
            Ok(()) => {
                report.copied += 1;
                report.verified += 1;
                report.bytes += item.size;
                written.push(item.path);
            }
            Err(e) => {
                if matches!(e, Error::Cancelled) {
                    report.cancelled = true;
                    break;
                }
                progress.failed += 1;
                report.failures.push(Failure {
                    dest: item.rel.to_owned(),
                    error: ErrorReport::from(&e),
                });
            }
        }
        progress.done_files += 1;
        progress.done_bytes += item.size;
        on_progress(&progress);
    }
    progress.current = None;
    on_progress(&progress);

    if android && !written.is_empty() {
        for folder in top_folders(written.iter().copied()) {
            let cmd = format!(
                "am broadcast -a android.intent.action.MEDIA_SCANNER_SCAN_FILE -d {}",
                shell_quote(&format!("file://{folder}"))
            );
            match adb.shell(serial, &cmd, Duration::from_secs(20)) {
                Ok(_) => report.scanned.push(folder),
                Err(e) => report.scan_error = Some(ErrorReport::from(&e)),
            }
        }
    }
    report
}

/// `2026-10-16 2131` in local time, for naming a backup folder.
pub fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    local_stamp(secs)
}

#[cfg(unix)]
pub(crate) fn local_stamp(secs: i64) -> String {
    let t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `t` and `tm` are valid for the call; localtime_r only writes into `tm`.
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return secs.to_string();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

#[cfg(not(unix))]
pub(crate) fn local_stamp(secs: i64) -> String {
    secs.to_string()
}

/// A folder for the originals of one run below `root`.
pub fn backup_dir(root: &Path, device: &str) -> PathBuf {
    let safe: String = device
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    root.join(safe.trim()).join(stamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_and_names() {
        let dir = std::env::temp_dir().join(format!("uwm-retag-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x");
        fs::write(&f, b"hello").unwrap();
        assert_eq!(md5_file(&f).unwrap(), "5d41402abc4b2a76b9719d911017c592");
        let b = backup_dir(Path::new("/b"), "NW-ZX507 / 0010");
        assert!(b.starts_with("/b/NW-ZX507 _ 0010"));
        assert_eq!(stamp().len(), "2026-10-16 213100".len());
        let _ = fs::remove_dir_all(dir);
    }
}
