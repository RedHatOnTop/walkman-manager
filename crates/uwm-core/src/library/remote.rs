//! Music already on a player, listed over adb.

use std::time::Duration;

use serde::Serialize;

use super::format::Codec;
use crate::adb::Adb;
use crate::error::Result;
use crate::probe::{Platform, Volume, VolumeKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFile {
    /// Path below the music root, `/`-separated.
    pub rel: String,
    pub size: u64,
    pub modified: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteListing {
    pub root: String,
    pub exists: bool,
    pub files: Vec<RemoteFile>,
}

/// Where the player's own music app looks on a volume. Android serves the card
/// through `/storage/<id>`, which is also where the shell user may write.
pub fn music_root(platform: Platform, volume: &Volume) -> String {
    match (platform, volume.kind) {
        (Platform::Android, VolumeKind::Internal) => "/sdcard/Music".to_owned(),
        (Platform::Android, VolumeKind::Card) => match volume.mount.strip_prefix("/mnt/media_rw/") {
            Some(id) => format!("/storage/{id}/Music"),
            None => format!("{}/Music", volume.mount),
        },
        (Platform::WalkmanLinux, _) => format!("{}/MUSIC", volume.mount),
    }
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Walkman OS has busybox 1.21 in /sbin, Android has toybox; both take this
/// `find` and `stat` form.
fn script(root: &str) -> String {
    format!(
        "r={}; [ -d \"$r\" ] || {{ echo @@none; exit 0; }}; \
         if [ -x /sbin/busybox ]; then b=/sbin/busybox; else b=; fi; \
         $b find \"$r\" -type f -exec $b stat -c '%s|%Y|%n' {{}} +",
        shell_quote(root)
    )
}

fn wanted(rel: &str) -> bool {
    if rel.split('/').any(|part| part.starts_with('.')) {
        return false;
    }
    let lower = rel.to_ascii_lowercase();
    lower.ends_with(".lrc") || Codec::from_path(std::path::Path::new(rel)).is_some()
}

pub fn parse(root: &str, text: &str) -> RemoteListing {
    let mut files = Vec::new();
    let prefix = format!("{}/", root.trim_end_matches('/'));
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line == "@@none" {
            return RemoteListing {
                root: root.to_owned(),
                exists: false,
                files,
            };
        }
        let mut f = line.splitn(3, '|');
        let (Some(size), Some(mtime), Some(path)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let (Ok(size), Some(rel)) = (size.parse(), path.strip_prefix(&prefix)) else {
            continue;
        };
        if !wanted(rel) {
            continue;
        }
        files.push(RemoteFile {
            rel: rel.to_owned(),
            size,
            modified: mtime.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0),
        });
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    RemoteListing {
        root: root.to_owned(),
        exists: true,
        files,
    }
}

pub fn list(adb: &Adb, serial: &str, root: &str) -> Result<RemoteListing> {
    let out = adb.shell(serial, &script(root), Duration::from_secs(90))?;
    Ok(parse(root, &out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(kind: VolumeKind, mount: &str) -> Volume {
        Volume {
            kind,
            mount: mount.to_owned(),
            filesystem: String::new(),
            total_bytes: 0,
            used_bytes: 0,
            free_bytes: 0,
        }
    }

    #[test]
    fn roots_per_platform() {
        let card = vol(VolumeKind::Card, "/mnt/media_rw/3564-3463");
        assert_eq!(music_root(Platform::Android, &card), "/storage/3564-3463/Music");
        let fuse = vol(VolumeKind::Card, "/storage/3564-3463");
        assert_eq!(music_root(Platform::Android, &fuse), "/storage/3564-3463/Music");
        let data = vol(VolumeKind::Internal, "/data");
        assert_eq!(music_root(Platform::Android, &data), "/sdcard/Music");
        let contents = vol(VolumeKind::Internal, "/contents");
        assert_eq!(music_root(Platform::WalkmanLinux, &contents), "/contents/MUSIC");
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("/a b/it's"), r"'/a b/it'\''s'");
    }

    #[test]
    fn parses_toybox_and_busybox_lines() {
        let text = "36555563|1654078414|/m/Music/Hans Zimmer/Interstellar [2014]/01 - Dreaming|of.flac\r\n\
                    120|1654078414.0|/m/Music/Hans Zimmer/Interstellar [2014]/01 - Dreaming|of.lrc\n\
                    9|1|/m/Music/.thumbnails/x.flac\n\
                    9|1|/m/Music/cover.jpg\n\
                    garbage\n\
                    7|1|/elsewhere/x.flac\n";
        let l = parse("/m/Music", text);
        assert!(l.exists);
        assert_eq!(
            l.files,
            [
                RemoteFile {
                    rel: "Hans Zimmer/Interstellar [2014]/01 - Dreaming|of.flac".into(),
                    size: 36_555_563,
                    modified: 1_654_078_414,
                },
                RemoteFile {
                    rel: "Hans Zimmer/Interstellar [2014]/01 - Dreaming|of.lrc".into(),
                    size: 120,
                    modified: 1_654_078_414,
                },
            ]
        );
        assert!(!parse("/x/MUSIC", "@@none\r\n").exists);
    }
}
