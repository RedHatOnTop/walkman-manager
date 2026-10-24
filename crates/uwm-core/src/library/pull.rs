//! Copies music off a player into a folder on the computer. A file already there at
//! the same size is left as it is, and nothing on the computer is ever overwritten.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;

use super::plan::{RESERVE_BYTES, Tally, dest_path};
use super::transfer::{Failure, Progress, Report};
use super::{Index, hex_id};
use crate::adb::Adb;
use crate::error::{Error, ErrorReport, Why};

/// Slowest rate a pull may run at before it counts as stuck, as for pushes.
const MIN_BYTES_PER_SEC: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PullAction {
    Copy,
    /// Already on the computer at the same size.
    Present,
    /// A different file of that name is on the computer; it is kept and this one skipped.
    Conflict,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullItem {
    pub track_id: String,
    pub remote: String,
    pub local: String,
    pub bytes: u64,
    pub action: PullAction,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullPlan {
    pub id: String,
    pub device_id: String,
    #[serde(skip)]
    pub adb_target: String,
    pub dest_root: String,
    pub items: Vec<PullItem>,
    pub copy: Tally,
    pub present: Tally,
    pub conflict: Tally,
    pub free_bytes: Option<u64>,
    pub fits: bool,
}

/// The part of a player path below its music folder, so `Music/Artist/Album/x.flac`
/// lands in `<dest>/Artist/Album/x.flac`.
fn below_music(rel: &str) -> &str {
    match rel.split_once('/') {
        Some((first, rest)) if first.eq_ignore_ascii_case("music") => rest,
        _ => rel,
    }
}

pub fn build(index: &Index, track_ids: &[String], dest_root: &Path, device_id: &str, adb_target: &str) -> PullPlan {
    let by_id = index.tracks_by_id();
    let mut items = Vec::new();
    let (mut copy, mut present, mut conflict) = (Tally::default(), Tally::default(), Tally::default());
    for id in track_ids {
        let Some(t) = by_id.get(id.as_str()) else { continue };
        let local = dest_root.join(dest_path(below_music(&t.rel)));
        let action = match fs::metadata(&local) {
            Ok(m) if m.len() == t.size => PullAction::Present,
            Ok(_) => PullAction::Conflict,
            Err(_) => PullAction::Copy,
        };
        let tally = match action {
            PullAction::Copy => &mut copy,
            PullAction::Present => &mut present,
            PullAction::Conflict => &mut conflict,
        };
        tally.files += 1;
        tally.bytes += t.size;
        items.push(PullItem {
            track_id: t.id.clone(),
            remote: t.path.clone(),
            local: local.to_string_lossy().into_owned(),
            bytes: t.size,
            action,
        });
    }
    let free_bytes = free_bytes(dest_root);
    let fits = free_bytes.is_none_or(|free| copy.bytes + RESERVE_BYTES <= free);
    PullPlan {
        id: hex_id(format!("pull\0{device_id}\0{}\0{}", dest_root.display(), track_ids.join(",")).as_bytes()),
        device_id: device_id.to_owned(),
        adb_target: adb_target.to_owned(),
        dest_root: dest_root.to_string_lossy().into_owned(),
        items,
        copy,
        present,
        conflict,
        free_bytes,
        fits,
    }
}

/// Space free for the user on the file system that holds `path` or its nearest
/// existing parent.
#[cfg(unix)]
pub fn free_bytes(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let existing = path.ancestors().find(|p| p.exists())?;
    let c = CString::new(existing.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `stat` is a writable statvfs.
    if unsafe { libc::statvfs(c.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(not(unix))]
pub fn free_bytes(_path: &Path) -> Option<u64> {
    None
}

fn part_path(local: &Path) -> PathBuf {
    let mut name = local.file_name().unwrap_or_default().to_os_string();
    name.push(".uwm-part");
    local.with_file_name(name)
}

/// Pulls every new file into a `.uwm-part` beside its destination, checks its size and
/// only then gives it its name, so an interrupted copy never looks finished.
pub fn run(adb: &Adb, plan: &PullPlan, cancel: &AtomicBool, on_progress: &mut dyn FnMut(&Progress)) -> Report {
    let work: Vec<_> = plan.items.iter().filter(|i| i.action == PullAction::Copy).collect();
    let mut progress = Progress {
        total_files: work.len(),
        total_bytes: work.iter().map(|i| i.bytes).sum(),
        ..Progress::default()
    };
    let mut report = Report::default();
    on_progress(&progress);

    for item in work {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let local = Path::new(&item.local);
        progress.current = Some(
            item.local
                .strip_prefix(&plan.dest_root)
                .unwrap_or(&item.local)
                .trim_start_matches('/')
                .to_owned(),
        );
        on_progress(&progress);
        let part = part_path(local);
        let result = (|| -> Result<(), Error> {
            if let Some(dir) = local.parent() {
                fs::create_dir_all(dir)?;
            }
            let timeout = Duration::from_secs(60 + item.bytes / MIN_BYTES_PER_SEC);
            adb.pull(&plan.adb_target, &item.remote, &part, timeout, cancel)?;
            let got = fs::metadata(&part)?.len();
            if got != item.bytes {
                return Err(Error::Failed {
                    tool: "adb",
                    detail: format!("{got} bytes arrived, {} expected", item.bytes),
                });
            }
            if local.exists() {
                return Err(Error::unsupported(
                    Why::AppearedWhileCopying,
                    format!("{} appeared while copying; it was kept", item.local),
                )
                .with_detail(item.local.clone()));
            }
            fs::rename(&part, local)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                report.copied += 1;
                report.verified += 1;
                report.bytes += item.bytes;
            }
            Err(e) => {
                let _ = fs::remove_file(&part);
                if matches!(e, Error::Cancelled) {
                    report.cancelled = true;
                    break;
                }
                report.failures.push(Failure {
                    dest: item.local.clone(),
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
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::track;

    #[test]
    fn plans_against_the_destination() {
        let dir = std::env::temp_dir().join(format!("uwm-pull-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("A/Album")).unwrap();
        let mut index = Index::default();
        for (rel, size) in [
            ("Music/A/Album/01.flac", 1000),
            ("Music/A/Album/02.flac", 1000),
            ("Music/A/Album/03.flac", 1000),
        ] {
            let mut t = track(rel, Some("Album"), Some("A"), None);
            t.size = size;
            index.tracks.push(t);
        }
        fs::write(dir.join("A/Album/01.flac"), vec![0u8; 1000]).unwrap();
        fs::write(dir.join("A/Album/02.flac"), vec![0u8; 10]).unwrap();
        let ids: Vec<String> = index.tracks.iter().map(|t| t.id.clone()).collect();
        let plan = build(&index, &ids, &dir, "dev", "serial");
        let actions: Vec<_> = plan.items.iter().map(|i| i.action).collect();
        assert_eq!(actions, [PullAction::Present, PullAction::Conflict, PullAction::Copy]);
        assert_eq!(plan.items[2].local, dir.join("A/Album/03.flac").to_string_lossy());
        assert_eq!((plan.copy.files, plan.present.files, plan.conflict.files), (1, 1, 1));
        assert!(plan.free_bytes.is_some_and(|f| f > 0));
        assert!(plan.fits);
        assert_eq!(part_path(Path::new("/x/a.flac")), Path::new("/x/a.flac.uwm-part"));
        assert_eq!(below_music("Music/A/b.mp3"), "A/b.mp3");
        assert_eq!(below_music("Download/b.mp3"), "Download/b.mp3");
        let _ = fs::remove_dir_all(dir);
    }
}
