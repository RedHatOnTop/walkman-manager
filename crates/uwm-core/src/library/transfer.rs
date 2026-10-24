//! Carries out a plan over adb, then reads the volume back to check every file.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;

use super::plan::{Action, Plan};
use super::remote::{self, shell_quote};
use crate::adb::Adb;
use crate::error::{Error, ErrorReport, Why};
use crate::probe::Platform;

/// Slowest rate a push is allowed before it counts as stuck: USB 2.0 on the
/// NW-A55 manages about 8 MB/s, so 1 MB/s leaves a wide margin.
const MIN_BYTES_PER_SEC: u64 = 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done_files: usize,
    pub total_files: usize,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub current: Option<String>,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub dest: String,
    pub error: ErrorReport,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub copied: usize,
    pub bytes: u64,
    /// Files whose size on the player matched after the copy.
    pub verified: usize,
    pub failures: Vec<Failure>,
    pub cancelled: bool,
    /// Android only: folders handed to the media scanner, and its answer when it refused.
    pub scanned: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_error: Option<ErrorReport>,
}

/// The fewest folders that cover every path: a folder whose parent is listed is dropped.
pub(crate) fn top_folders<'a>(dests: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let dirs: BTreeSet<&str> = dests
        .into_iter()
        .map(|d| d.rsplit_once('/').map_or("", |(dir, _)| dir))
        .collect();
    let mut out: Vec<String> = Vec::new();
    for d in dirs {
        if d.is_empty() {
            return vec![String::new()];
        }
        if !out.iter().any(|o| d.starts_with(&format!("{o}/"))) {
            out.push(d.to_owned());
        }
    }
    out
}

pub fn run(adb: &Adb, plan: &Plan, cancel: &AtomicBool, on_progress: &mut dyn FnMut(&Progress)) -> Report {
    let serial = plan.target.adb_target.as_str();
    let root = plan.target.root.trim_end_matches('/');
    let work: Vec<_> = plan
        .items
        .iter()
        .filter(|i| matches!(i.action, Action::Copy | Action::Replace))
        .collect();
    let mut progress = Progress {
        total_files: work.len(),
        total_bytes: work.iter().map(|i| i.bytes).sum(),
        ..Progress::default()
    };
    let mut report = Report::default();
    let mut pushed = Vec::new();
    on_progress(&progress);

    for item in &work {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let dest = format!("{root}/{}", item.dest);
        progress.current = Some(item.dest.clone());
        on_progress(&progress);
        let timeout = Duration::from_secs(60 + item.bytes / MIN_BYTES_PER_SEC);
        match adb.push(serial, Path::new(&item.source), &dest, timeout, cancel) {
            Ok(()) => {
                pushed.push(*item);
                report.copied += 1;
                report.bytes += item.bytes;
            }
            Err(e) => {
                // A half-written new file is ours to remove; a replaced one is gone either way.
                if item.action == Action::Copy {
                    let _ = adb.shell(
                        serial,
                        &format!("rm -f {}", shell_quote(&dest)),
                        Duration::from_secs(10),
                    );
                }
                if matches!(e, Error::Cancelled) {
                    report.cancelled = true;
                    break;
                }
                report.failures.push(Failure {
                    dest: item.dest.clone(),
                    error: ErrorReport::from(&e),
                });
                progress.failed += 1;
            }
        }
        progress.done_files += 1;
        progress.done_bytes += item.bytes;
        on_progress(&progress);
    }
    progress.current = None;
    on_progress(&progress);

    if pushed.is_empty() {
        return report;
    }
    match remote::list(adb, serial, root) {
        Ok(listing) => {
            for item in &pushed {
                match listing.files.iter().find(|f| f.rel == item.dest) {
                    Some(f) if f.size == item.bytes => report.verified += 1,
                    Some(f) => report.failures.push(Failure {
                        dest: item.dest.clone(),
                        error: ErrorReport::from(
                            &Error::unsupported(
                                Why::SizeAfterCopy,
                                format!("{} bytes on the player, {} expected", f.size, item.bytes),
                            )
                            .with_var("found", f.size)
                            .with_var("expected", item.bytes),
                        ),
                    }),
                    None => report.failures.push(Failure {
                        dest: item.dest.clone(),
                        error: ErrorReport::from(&Error::unsupported(
                            Why::MissingAfterCopy,
                            "not found on the player after the copy",
                        )),
                    }),
                }
            }
        }
        Err(e) => report.failures.push(Failure {
            dest: root.to_owned(),
            error: ErrorReport::from(
                &Error::unsupported(Why::ReadBackFailed, format!("could not read the volume back: {e}"))
                    .with_detail(e.to_string()),
            ),
        }),
    }

    if plan.target.platform == Platform::Android && report.verified > 0 {
        for folder in top_folders(pushed.iter().map(|i| i.dest.as_str())) {
            let path = if folder.is_empty() {
                root.to_owned()
            } else {
                format!("{root}/{folder}")
            };
            let cmd = format!(
                "am broadcast -a android.intent.action.MEDIA_SCANNER_SCAN_FILE -d {}",
                shell_quote(&format!("file://{path}"))
            );
            match adb.shell(serial, &cmd, Duration::from_secs(20)) {
                Ok(_) => report.scanned.push(path),
                Err(e) => report.scan_error = Some(ErrorReport::from(&e)),
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_cover_paths() {
        let dests = ["A/B/1.flac", "A/B/2.flac", "A/B/CD2/1.flac", "A/C/1.flac", "Z/1.flac"];
        assert_eq!(top_folders(dests), ["A/B", "A/C", "Z"]);
        assert_eq!(top_folders(["x.flac", "A/1.flac"]), [""]);
    }
}
