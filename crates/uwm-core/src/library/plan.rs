//! A dry run of a sync: what would be copied, what is already there, and what the
//! player's music app cannot play. Nothing is deleted from a player.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::format::{Playback, Unplayable};
use super::remote::RemoteListing;
use super::{Index, hex_id};
use crate::probe::{Platform, VolumeKind};

/// Space left free on the volume after a sync, for the player's own database.
pub const RESERVE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub device_id: String,
    pub adb_target: String,
    pub platform: Platform,
    pub volume: VolumeKind,
    pub root: String,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Copy,
    Replace,
    Skip,
    Unplayable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    pub source: String,
    /// Destination below the music root, `/`-separated.
    pub dest: String,
    pub bytes: u64,
    pub action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Unplayable>,
    /// Size of the file a replace overwrites.
    pub replaces_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub target: Target,
    pub items: Vec<PlanItem>,
    pub copy: Tally,
    pub skip: Tally,
    pub unplayable: Tally,
    /// Bytes the volume loses: new files plus growth of replaced ones.
    pub needed_bytes: u64,
    pub fits: bool,
}

/// FAT32 and exFAT refuse these in names; a trailing dot or space is dropped by
/// Windows-style volumes and would never match again on the next sync.
fn clean_component(part: &str) -> String {
    let mut s: String = part
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    while s.ends_with(['.', ' ']) {
        s.pop();
    }
    if s.is_empty() { "_".to_owned() } else { s }
}

pub fn dest_path(rel: &str) -> String {
    rel.split('/')
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .map(clean_component)
        .collect::<Vec<_>>()
        .join("/")
}

pub fn build(index: &Index, track_ids: &[String], remote: &RemoteListing, playback: &Playback, target: Target) -> Plan {
    let on_player: HashMap<String, u64> = remote.files.iter().map(|f| (f.rel.to_lowercase(), f.size)).collect();
    let by_id = index.tracks_by_id();
    let mut seen = HashSet::new();
    let mut items = Vec::new();

    let mut place = |track_id: Option<String>, source: &str, rel: &str, bytes: u64, reason: Option<Unplayable>| {
        let dest = dest_path(rel);
        if !seen.insert(dest.to_lowercase()) {
            return;
        }
        let existing = on_player.get(&dest.to_lowercase()).copied();
        let action = match (reason, existing) {
            (Some(_), _) => Action::Unplayable,
            (None, Some(size)) if size == bytes => Action::Skip,
            (None, Some(_)) => Action::Replace,
            (None, None) => Action::Copy,
        };
        items.push(PlanItem {
            track_id,
            source: source.to_owned(),
            dest,
            bytes,
            action,
            reason,
            replaces_bytes: if action == Action::Replace {
                existing.unwrap_or(0)
            } else {
                0
            },
        });
    };

    for id in track_ids {
        let Some(t) = by_id.get(id.as_str()) else { continue };
        let reason = playback.check(&t.format, t.size).err();
        place(Some(t.id.clone()), &t.path, &t.rel, t.size, reason);
        if let (Some(lrc), None) = (&t.lyrics, reason) {
            let root = &t.path[..t.path.len() - t.rel.len()];
            let source = format!("{root}{lrc}");
            let size = std::fs::metadata(&source).map_or(0, |m| m.len());
            place(None, &source, lrc, size, None);
        }
    }

    let tally = |a: Action| {
        items
            .iter()
            .filter(|i| i.action == a)
            .fold(Tally::default(), |t, i| Tally {
                files: t.files + 1,
                bytes: t.bytes + i.bytes,
            })
    };
    let copy = Tally {
        files: tally(Action::Copy).files + tally(Action::Replace).files,
        bytes: tally(Action::Copy).bytes + tally(Action::Replace).bytes,
    };
    let needed_bytes = items
        .iter()
        .filter(|i| matches!(i.action, Action::Copy | Action::Replace))
        .map(|i| i.bytes.saturating_sub(i.replaces_bytes))
        .sum::<u64>();
    let fits = needed_bytes + RESERVE_BYTES <= target.free_bytes || needed_bytes == 0;
    let id = hex_id(
        format!(
            "{}\0{}\0{}\0{}",
            target.device_id,
            target.root,
            track_ids.join(","),
            index.scanned_at_ms
        )
        .as_bytes(),
    );
    Plan {
        id,
        skip: tally(Action::Skip),
        unplayable: tally(Action::Unplayable),
        copy,
        needed_bytes,
        fits,
        items,
        target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::remote::RemoteFile;
    use crate::library::tests::track;
    use crate::library::{Codec, group_albums};

    fn playback() -> Playback {
        Playback {
            max_file_bytes: 4 << 30,
            codecs: [("flac".to_owned(), 384_000)].into(),
        }
    }

    fn target(free: u64) -> Target {
        Target {
            device_id: "d".into(),
            adb_target: "d".into(),
            platform: Platform::Android,
            volume: VolumeKind::Card,
            root: "/storage/X/Music".into(),
            free_bytes: free,
        }
    }

    #[test]
    fn copy_skip_replace_unplayable() {
        let mut tracks = vec![
            track("A/B/01 one.flac", Some("B"), Some("A"), None),
            track("A/B/02 two.flac", Some("B"), Some("A"), None),
            track("A/B/03 three.flac", Some("B"), Some("A"), None),
            track("A/B/04 four.opus", Some("B"), Some("A"), None),
        ];
        tracks[3].format.codec = Codec::Opus;
        let index = Index {
            albums: group_albums(&tracks, |_| None),
            tracks,
            ..Index::default()
        };
        let remote = RemoteListing {
            root: "/storage/X/Music".into(),
            exists: true,
            files: vec![
                RemoteFile {
                    rel: "a/b/01 one.flac".into(),
                    size: 1000,
                    modified: 0,
                },
                RemoteFile {
                    rel: "A/B/02 two.flac".into(),
                    size: 400,
                    modified: 0,
                },
            ],
        };
        let ids: Vec<String> = index.tracks.iter().map(|t| t.id.clone()).collect();
        let plan = build(&index, &ids, &remote, &playback(), target(10 << 30));
        let actions: Vec<_> = plan.items.iter().map(|i| i.action).collect();
        assert_eq!(
            actions,
            [Action::Skip, Action::Replace, Action::Copy, Action::Unplayable]
        );
        assert_eq!(plan.items[3].reason, Some(Unplayable::Codec));
        assert_eq!((plan.copy.files, plan.copy.bytes), (2, 2000));
        assert_eq!(plan.needed_bytes, 600 + 1000);
        assert!(plan.fits);
        assert!(!build(&index, &ids, &remote, &playback(), target(1000)).fits);
    }

    #[test]
    fn destination_names() {
        assert_eq!(dest_path("AC/DC: Live?/01 \"Hi\".flac"), "AC/DC_ Live_/01 _Hi_.flac");
        assert_eq!(dest_path("Album.../x.flac"), "Album/x.flac");
        assert_eq!(dest_path("../a//b/./c.flac"), "a/b/c.flac");
    }
}
