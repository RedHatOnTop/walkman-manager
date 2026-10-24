//! The music library on the computer, what is on each player, and the plans that
//! move one onto the other.

pub mod analyse;
pub mod art;
pub mod dsd;
pub mod format;
pub mod lookup;
pub mod plan;
pub mod player;
pub mod pull;
pub mod remote;
pub mod retag;
pub mod scan;
pub mod sensme;
mod tagbytes;
pub mod tags;
pub mod transfer;

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

pub use format::{AudioFormat, Codec, Quality};

/// Bump whenever tag reading changes: an index from another version is not reused,
/// so every track is read again.
pub const INDEX_VERSION: u32 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub path: String,
    /// Path below its library root, `/`-separated. Sync keeps this layout.
    pub rel: String,
    pub root: usize,
    pub size: u64,
    pub modified: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    pub duration_ms: u64,
    pub format: AudioFormat,
    /// Hash key of the embedded front cover, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// A sidecar `.lrc` with the same stem, relative to the library root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<String>,
    /// Its text tags are CP949 or Shift_JIS bytes marked as Latin-1, which most players
    /// show garbled; the values above are already decoded.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legacy_text: bool,
    /// Sony's 12 TONE ANALYSIS, which the Walkman sorts into SensMe channels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensme: Option<sensme::SensMe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: String,
    pub title: String,
    /// None when the tracks name different artists or none at all.
    pub artist: Option<String>,
    /// Tracks name two or more different artists and no album artist.
    #[serde(default)]
    pub compilation: bool,
    pub year: Option<u16>,
    pub genre: Option<String>,
    pub tracks: Vec<String>,
    pub duration_ms: u64,
    pub bytes: u64,
    pub quality: Quality,
    /// The format most of its tracks share, for the album's one-line summary.
    pub format: AudioFormat,
    pub mixed_formats: bool,
    /// Art key of the thumbnail in the art cache.
    pub art: Option<String>,
    pub folder: String,
    pub modified: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub version: u32,
    pub roots: Vec<String>,
    pub scanned_at_ms: u64,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub skipped: Vec<Skipped>,
}

impl Index {
    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn tracks_by_id(&self) -> HashMap<&str, &Track> {
        self.tracks.iter().map(|t| (t.id.as_str(), t)).collect()
    }
}

/// FNV-1a, 64 bit. Ids only need to be stable across runs and platforms.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn hex_id(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a(bytes))
}

fn fold(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// `Disc 2`, `CD1`, `Disk 03` and the like, which split one album across folders.
fn is_disc_folder(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    let rest = ["disc", "disk", "cd"].iter().find_map(|p| n.strip_prefix(p));
    rest.is_some_and(|r| {
        let r = r.trim_start_matches([' ', '-', '_', '.']);
        !r.is_empty() && r.chars().all(|c| c.is_ascii_digit())
    })
}

fn parent_dir(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The folder an album lives in, one level up from per-disc folders.
fn album_folder(rel: &str) -> &str {
    let dir = parent_dir(rel);
    match dir.rsplit_once('/') {
        Some((up, last)) if is_disc_folder(last) => up,
        None if is_disc_folder(dir) => "",
        _ => dir,
    }
}

/// Groups tracks into albums. Tracks share an album when their album tags and
/// album artists agree; without an album artist, tracks of one album folder do.
/// Untagged tracks group by folder.
pub fn group_albums(tracks: &[Track], art_for: impl Fn(&[&Track]) -> Option<String>) -> Vec<Album> {
    let mut groups: BTreeMap<(usize, String, String), Vec<&Track>> = BTreeMap::new();
    for t in tracks {
        let folder = album_folder(&t.rel);
        let key = match (&t.album, &t.album_artist) {
            (Some(album), Some(aa)) => (0, fold(album), fold(aa)),
            (Some(album), None) => (t.root, fold(album), folder.to_owned()),
            (None, _) => (t.root, String::new(), folder.to_owned()),
        };
        groups.entry(key).or_default().push(t);
    }

    let mut albums: Vec<Album> = groups
        .into_iter()
        .map(|(key, mut ts)| {
            ts.sort_by(|a, b| {
                (a.disc.unwrap_or(1), a.track.unwrap_or(u32::MAX), &a.rel).cmp(&(
                    b.disc.unwrap_or(1),
                    b.track.unwrap_or(u32::MAX),
                    &b.rel,
                ))
            });
            let first = ts[0];
            let folder_rel = album_folder(&first.rel);
            let folder_name = folder_rel.rsplit('/').next().filter(|s| !s.is_empty());
            let title = first
                .album
                .clone()
                .or_else(|| folder_name.map(str::to_owned))
                .unwrap_or_else(|| first.rel.clone());
            let artist = first.album_artist.clone().or_else(|| {
                let a = first.artist.as_ref()?;
                ts.iter().all(|t| t.artist.as_ref() == Some(a)).then(|| a.clone())
            });
            let named: std::collections::HashSet<String> =
                ts.iter().filter_map(|t| t.artist.as_deref()).map(fold).collect();
            let compilation = artist.is_none() && named.len() > 1;
            let mut counts: HashMap<(Codec, Option<u32>, Option<u8>), usize> = HashMap::new();
            for t in &ts {
                *counts
                    .entry((t.format.codec, t.format.sample_rate, t.format.bits))
                    .or_default() += 1;
            }
            let common = counts.iter().max_by_key(|(k, n)| (**n, k.1, k.2)).map(|(k, _)| *k);
            let format = ts
                .iter()
                .find(|t| Some((t.format.codec, t.format.sample_rate, t.format.bits)) == common)
                .map_or_else(|| first.format.clone(), |t| t.format.clone());
            let root_path = &first.path[..first.path.len() - first.rel.len()];
            Album {
                // From the grouping key, so one title by one artist in two folders stays two albums.
                id: hex_id(format!("{}\0{}\0{}", key.0, key.1, key.2).as_bytes()),
                title,
                artist,
                compilation,
                year: ts.iter().find_map(|t| t.year),
                genre: ts.iter().find_map(|t| t.genre.clone()),
                duration_ms: ts.iter().map(|t| t.duration_ms).sum(),
                bytes: ts.iter().map(|t| t.size).sum(),
                quality: ts.iter().map(|t| t.format.quality()).max().unwrap_or(Quality::Lossy),
                format,
                mixed_formats: counts.len() > 1,
                art: art_for(&ts),
                folder: format!("{}{}", root_path, folder_rel),
                modified: ts.iter().map(|t| t.modified).max().unwrap_or(0),
                tracks: ts.iter().map(|t| t.id.clone()).collect(),
            }
        })
        .collect();
    albums.sort_by(|a, b| {
        let ka = (a.artist.as_deref().map(fold), fold(&a.title));
        let kb = (b.artist.as_deref().map(fold), fold(&b.title));
        ka.cmp(&kb)
    });
    albums
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn track(rel: &str, album: Option<&str>, artist: Option<&str>, album_artist: Option<&str>) -> Track {
        Track {
            id: hex_id(rel.as_bytes()),
            path: format!("/music/{rel}"),
            rel: rel.to_owned(),
            root: 0,
            size: 1000,
            modified: 1,
            title: None,
            artist: artist.map(str::to_owned),
            album: album.map(str::to_owned),
            album_artist: album_artist.map(str::to_owned),
            genre: None,
            track: None,
            disc: None,
            year: None,
            duration_ms: 1000,
            format: AudioFormat {
                codec: Codec::Flac,
                sample_rate: Some(96_000),
                bits: Some(24),
                channels: Some(2),
                bitrate_kbps: None,
            },
            cover: None,
            lyrics: None,
            legacy_text: false,
            sensme: None,
        }
    }

    #[test]
    fn disc_folders_join_one_album() {
        let ts = vec![
            track("A/Album/CD1/01.flac", Some("Album"), Some("A"), None),
            track("A/Album/CD2/01.flac", Some("Album"), Some("A"), None),
            track("A/Album/Disc 3/01.flac", Some("Album"), Some("A"), None),
        ];
        let albums = group_albums(&ts, |_| None);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].tracks.len(), 3);
        assert_eq!(albums[0].folder, "/music/A/Album");
        assert_eq!(albums[0].artist.as_deref(), Some("A"));
    }

    #[test]
    fn copies_in_two_folders_keep_their_own_ids() {
        let ts = vec![
            track("Hits 2008/01 Song.mp3", Some("Song"), Some("2AM"), None),
            track("Ballads/01 Song.mp3", Some("Song"), Some("2AM"), None),
        ];
        let albums = group_albums(&ts, |_| None);
        assert_eq!(albums.len(), 2);
        assert_ne!(albums[0].id, albums[1].id);
    }

    #[test]
    fn compilations_and_untagged() {
        let ts = vec![
            track("Comp/01.flac", Some("Hits"), Some("X"), None),
            track("Comp/02.flac", Some("Hits"), Some("Y"), None),
            track("Other/Hits/01.flac", Some("Hits"), Some("Z"), None),
            track("Loose/a.flac", None, None, None),
            track("Loose/b.flac", None, None, None),
        ];
        let albums = group_albums(&ts, |_| None);
        assert_eq!(albums.len(), 3);
        let comp = albums.iter().find(|a| a.folder == "/music/Comp").unwrap();
        assert_eq!(comp.tracks.len(), 2);
        assert_eq!(comp.artist, None);
        assert!(comp.compilation);
        let loose = albums.iter().find(|a| a.title == "Loose").unwrap();
        assert_eq!(loose.tracks.len(), 2);
        assert!(!loose.compilation);
    }

    #[test]
    fn album_artist_tag_spans_folders() {
        let ts = vec![
            track("x/01.flac", Some("Same"), Some("P"), Some("Band")),
            track("y/02.flac", Some("Same"), Some("Q"), Some("band")),
        ];
        let albums = group_albums(&ts, |_| None);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].artist.as_deref(), Some("Band"));
    }

    #[test]
    fn disc_folder_names() {
        for n in ["CD1", "cd 2", "Disc 03", "disk-4", "DISC_1"] {
            assert!(is_disc_folder(n), "{n}");
        }
        for n in ["CD", "Discography", "cds", "Disc One"] {
            assert!(!is_disc_folder(n), "{n}");
        }
    }
}
