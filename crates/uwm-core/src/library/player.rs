//! The music on a player read as a library: from Android's media store, which is what
//! the player's own music app shows, or from the folder layout when there is none.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use super::format::{AudioFormat, Codec};
use super::remote::{RemoteListing, shell_quote};
use super::scan::repair_legacy;
use super::{INDEX_VERSION, Index, Track, group_albums, hex_id};
use crate::adb::Adb;
use crate::error::{Error, Result};

/// Columns asked of the media store, in the order `content query` prints them.
/// Android 9 (stock NW-ZX500 firmware) lacks the last three.
const COLUMNS: [&str; 15] = [
    "_id",
    "_data",
    "_size",
    "date_modified",
    "duration",
    "title",
    "artist",
    "album",
    "album_id",
    "track",
    "year",
    "mime_type",
    "album_artist",
    "genre",
    "bitrate",
];
const COLUMNS_API28: usize = 12;

/// Where a track's cover comes from on the player, keyed like the art cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteArt {
    pub key: String,
    pub uri: String,
}

#[derive(Debug, Clone)]
pub struct PlayerLibrary {
    pub index: Index,
    pub art: Vec<RemoteArt>,
}

fn query(columns: &[&str]) -> String {
    format!(
        "content query --uri content://media/external/audio/media --projection {} --where {}",
        columns.join(":"),
        shell_quote("is_music=1"),
    )
}

/// Reads the media store. Falls back to the Android 9 columns when the newer ones are
/// refused.
pub fn read_media_store(adb: &Adb, serial: &str) -> Result<PlayerLibrary> {
    let timeout = Duration::from_secs(60);
    let mut columns = &COLUMNS[..];
    let mut out = adb.shell(serial, &query(columns), timeout)?;
    if out.contains("no such column") {
        columns = &COLUMNS[..COLUMNS_API28];
        out = adb.shell(serial, &query(columns), timeout)?;
    }
    if out.contains("Error while accessing provider") {
        let detail = out.lines().take(2).collect::<Vec<_>>().join(" ");
        return Err(Error::Failed { tool: "adb", detail });
    }
    Ok(from_media_store(serial, &out, columns))
}

/// Splits `content query` output into rows of column values. A value may hold a
/// newline, so a line that does not start a row continues the one before.
fn rows(text: &str, columns: &[&str]) -> Vec<HashMap<&'static str, String>> {
    let mut joined: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with("Row: ") {
            joined.push(line.to_owned());
        } else if let Some(last) = joined.last_mut() {
            last.push('\n');
            last.push_str(line);
        }
    }
    joined.iter().filter_map(|line| row(line, columns)).collect()
}

fn row(line: &str, columns: &[&str]) -> Option<HashMap<&'static str, String>> {
    let rest = line.strip_prefix("Row: ")?;
    let mut cur = &rest[rest.find(' ')? + 1..];
    let mut out = HashMap::new();
    for (i, key) in columns.iter().enumerate() {
        cur = cur.strip_prefix(key)?.strip_prefix('=')?;
        let end = match columns.get(i + 1) {
            Some(next) => cur.find(&format!(", {next}="))?,
            None => cur.len(),
        };
        let value = &cur[..end];
        if value != "NULL" {
            let name = COLUMNS.iter().copied().find(|c| c == key)?;
            out.insert(name, value.to_owned());
        }
        cur = cur.get(end + 2..).unwrap_or("");
    }
    Some(out)
}

/// The storage volume a media-store path lives on: `/storage/emulated/0` for the
/// internal one, `/storage/<id>` for a card.
fn volume_root(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/storage/")?;
    let first = rest.split('/').next()?;
    let len = if first == "emulated" {
        "/storage/emulated/0".len()
    } else {
        "/storage/".len() + first.len()
    };
    path.get(..len).filter(|_| path.len() > len)
}

fn text(v: Option<&String>) -> Option<String> {
    v.map(|s| repair_legacy(s.trim()))
        .filter(|s| !s.is_empty() && s != "<unknown>")
}

pub fn from_media_store(serial: &str, out: &str, columns: &[&str]) -> PlayerLibrary {
    let mut roots: Vec<String> = Vec::new();
    let mut tracks = Vec::new();
    let mut art = Vec::new();
    for r in rows(out, columns) {
        let Some(path) = r.get("_data") else { continue };
        let Some(codec) = Codec::from_path(Path::new(path)) else {
            continue;
        };
        let Some(root) = volume_root(path) else { continue };
        let root_index = match roots.iter().position(|x| x == root) {
            Some(i) => i,
            None => {
                roots.push(root.to_owned());
                roots.len() - 1
            }
        };
        let num = |k: &str| r.get(k).and_then(|v| v.parse::<u64>().ok());
        let encoded_track = num("track").unwrap_or(0);
        let cover = r.get("album_id").map(|album_id| {
            let key = hex_id(format!("{serial}\0album\0{album_id}").as_bytes());
            art.push(RemoteArt {
                key: key.clone(),
                uri: format!("content://media/external/audio/albumart/{album_id}"),
            });
            key
        });
        tracks.push(Track {
            id: hex_id(format!("{serial}\0{path}").as_bytes()),
            path: path.clone(),
            rel: path[root.len() + 1..].to_owned(),
            root: root_index,
            size: num("_size").unwrap_or(0),
            modified: num("date_modified").unwrap_or(0) as i64,
            title: text(r.get("title")),
            artist: text(r.get("artist")),
            album: text(r.get("album")),
            album_artist: text(r.get("album_artist")),
            genre: text(r.get("genre")),
            // The media store keeps the disc in the thousands: 2003 is disc 2, track 3.
            track: Some((encoded_track % 1000) as u32).filter(|n| *n > 0),
            disc: Some((encoded_track / 1000) as u32).filter(|n| *n > 0),
            year: num("year").map(|y| y as u16).filter(|y| *y > 0),
            duration_ms: num("duration").unwrap_or(0),
            format: AudioFormat {
                codec,
                sample_rate: None,
                bits: None,
                channels: None,
                bitrate_kbps: num("bitrate").map(|b| (b / 1000) as u32).filter(|k| *k > 0),
            },
            cover,
            lyrics: None,
            legacy_text: ["title", "artist", "album", "album_artist", "genre"]
                .iter()
                .filter_map(|k| r.get(*k))
                .any(|v| repair_legacy(v.trim()) != v.trim()),
            sensme: None,
        });
    }
    art.sort_by(|a, b| a.key.cmp(&b.key));
    art.dedup();
    finish(serial, roots, tracks, art)
}

/// `01 Title`, `01. Title`, `01 - Title` and `1-01 Title` lose their numbers.
fn title_from_stem(stem: &str) -> (Option<u32>, String) {
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    let rest = stem[digits.len()..].trim_start_matches([' ', '.', '-', '_']).trim();
    let number = digits.rsplit('-').next().and_then(|d| d.parse().ok());
    if digits.is_empty() || rest.is_empty() || number.is_none() {
        (None, stem.to_owned())
    } else {
        (number, rest.to_owned())
    }
}

/// A library from listings alone, for Walkman OS: the album is the folder, the artist
/// the folder above it, the title the file name.
pub fn from_listings(serial: &str, listings: &[RemoteListing]) -> PlayerLibrary {
    let mut roots = Vec::new();
    let mut tracks = Vec::new();
    for listing in listings {
        let root_index = roots.len();
        roots.push(listing.root.clone());
        for f in &listing.files {
            let Some(codec) = Codec::from_path(Path::new(&f.rel)) else {
                continue;
            };
            let parts: Vec<&str> = f.rel.split('/').collect();
            let stem = Path::new(parts[parts.len() - 1])
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let (number, title) = title_from_stem(stem);
            let path = format!("{}/{}", listing.root.trim_end_matches('/'), f.rel);
            tracks.push(Track {
                id: hex_id(format!("{serial}\0{path}").as_bytes()),
                path,
                rel: f.rel.clone(),
                root: root_index,
                size: f.size,
                modified: f.modified,
                title: Some(title),
                artist: (parts.len() >= 3).then(|| parts[parts.len() - 3].to_owned()),
                album: None,
                album_artist: None,
                genre: None,
                track: number,
                disc: None,
                year: None,
                duration_ms: 0,
                format: AudioFormat {
                    codec,
                    sample_rate: None,
                    bits: None,
                    channels: None,
                    bitrate_kbps: None,
                },
                cover: None,
                lyrics: None,
                legacy_text: false,
                sensme: None,
            });
        }
    }
    finish(serial, roots, tracks, Vec::new())
}

fn finish(serial: &str, roots: Vec<String>, tracks: Vec<Track>, art: Vec<RemoteArt>) -> PlayerLibrary {
    let mut albums = group_albums(&tracks, |ts| ts.iter().find_map(|t| t.cover.clone()));
    // Player albums sit beside the computer's in one selection, so their ids differ.
    for a in &mut albums {
        a.id = hex_id(format!("{serial}\0{}", a.id).as_bytes());
    }
    let used: std::collections::HashSet<&str> = albums.iter().filter_map(|a| a.art.as_deref()).collect();
    let art = art.into_iter().filter(|a| used.contains(a.key.as_str())).collect();
    PlayerLibrary {
        index: Index {
            version: INDEX_VERSION,
            roots,
            scanned_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
            tracks,
            albums,
            skipped: Vec::new(),
        },
        art,
    }
}

/// The cover the media store keeps for an album, as image bytes.
pub fn read_art(adb: &Adb, serial: &str, uri: &str) -> Result<Vec<u8>> {
    let bytes = adb.exec_out(
        serial,
        &format!("content read --uri {}", shell_quote(uri)),
        Duration::from_secs(20),
    )?;
    // A missing cover answers with a Java stack trace instead of an image.
    let image = bytes.starts_with(&[0xff, 0xd8]) || bytes.starts_with(b"\x89PNG") || bytes.starts_with(b"RIFF");
    if !image {
        let detail = String::from_utf8_lossy(&bytes[..bytes.len().min(160)])
            .trim()
            .to_owned();
        return Err(Error::Failed { tool: "adb", detail });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::remote::RemoteFile;

    const SAMPLE: &str = "Row: 0 _id=1000000027, _data=/storage/3564-3463/Music/Cool/[1994] 1집/01 The Cool Prologue.mp3, _size=1853696, date_modified=1059059008, duration=115856, title=The Cool Prologue, artist=Äð(Cool), album=Cool, album_id=3698273574758496133, track=1, year=1994, mime_type=audio/mpeg, album_artist=NULL, genre=Dance, bitrate=128000\n\
Row: 1 _id=1000000028, _data=/storage/3564-3463/Music/Cool/[1994] 1집/02 Love, Again.mp3, _size=3983744, date_modified=1128474968, duration=248984, title=Love, Again\nPart 2, artist=쿨(Cool), album=Cool, album_id=3698273574758496133, track=1002, year=1994, mime_type=audio/mpeg, album_artist=NULL, genre=Dance, bitrate=128000\n\
Row: 2 _id=1000000029, _data=/storage/emulated/0/Music/Hans Zimmer/Interstellar/01 Dreaming.flac, _size=36555563, date_modified=1654078414, duration=226000, title=Dreaming of the Crash, artist=Hans Zimmer, album=Interstellar, album_id=77, track=1, year=2014, mime_type=audio/flac, album_artist=Hans Zimmer, genre=NULL, bitrate=2304000\n\
Row: 3 _id=1, _data=/storage/emulated/0/Notifications/x.txt, _size=1, date_modified=1, duration=1, title=x, artist=NULL, album=NULL, album_id=1, track=NULL, year=NULL, mime_type=text/plain, album_artist=NULL, genre=NULL, bitrate=NULL\n";

    #[test]
    fn media_store_rows() {
        let lib = from_media_store("SER", SAMPLE, &COLUMNS);
        let idx = &lib.index;
        assert_eq!(idx.roots, ["/storage/3564-3463", "/storage/emulated/0"]);
        assert_eq!(idx.tracks.len(), 3);
        let t = &idx.tracks[1];
        assert_eq!(t.title.as_deref(), Some("Love, Again\nPart 2"));
        assert_eq!(t.rel, "Music/Cool/[1994] 1집/02 Love, Again.mp3");
        assert_eq!((t.disc, t.track), (Some(1), Some(2)));
        assert_eq!(idx.tracks[0].artist.as_deref(), Some("쿨(Cool)"));
        assert_eq!(idx.tracks[2].format.bitrate_kbps, Some(2304));
        assert_eq!(idx.albums.len(), 2);
        let cool = idx.albums.iter().find(|a| a.title == "Cool").unwrap();
        assert_eq!(cool.tracks.len(), 2);
        assert_eq!(cool.artist.as_deref(), Some("쿨(Cool)"));
        assert_eq!(lib.art.len(), 2);
        assert!(
            lib.art
                .iter()
                .any(|a| a.uri == "content://media/external/audio/albumart/77")
        );
        assert_eq!(
            cool.art.as_deref(),
            Some(
                lib.art
                    .iter()
                    .find(|a| a.uri.ends_with("3698273574758496133"))
                    .unwrap()
                    .key
                    .as_str()
            )
        );
    }

    #[test]
    fn android9_columns() {
        let text = "Row: 0 _id=5, _data=/storage/emulated/0/Music/a.flac, _size=10, date_modified=2, duration=3, title=A, artist=B, album=C, album_id=9, track=4, year=2001, mime_type=audio/flac\n";
        let lib = from_media_store("S", text, &COLUMNS[..COLUMNS_API28]);
        assert_eq!(lib.index.tracks.len(), 1);
        assert_eq!(lib.index.tracks[0].track, Some(4));
        assert_eq!(lib.index.tracks[0].album_artist, None);
    }

    #[test]
    fn folder_layout() {
        let listing = RemoteListing {
            root: "/contents/MUSIC".into(),
            exists: true,
            files: vec![
                RemoteFile {
                    rel: "Artist/Album/01 - First.flac".into(),
                    size: 1,
                    modified: 1,
                },
                RemoteFile {
                    rel: "Artist/Album/1-02 Second.flac".into(),
                    size: 1,
                    modified: 1,
                },
                RemoteFile {
                    rel: "Artist/Album/01 - First.lrc".into(),
                    size: 1,
                    modified: 1,
                },
                RemoteFile {
                    rel: "Loose.mp3".into(),
                    size: 1,
                    modified: 1,
                },
            ],
        };
        let lib = from_listings("S", &[listing]);
        assert_eq!(lib.index.tracks.len(), 3);
        let album = lib.index.albums.iter().find(|a| a.title == "Album").unwrap();
        assert_eq!(album.artist.as_deref(), Some("Artist"));
        let first = lib.index.tracks.iter().find(|t| t.rel.ends_with("First.flac")).unwrap();
        assert_eq!((first.track, first.title.as_deref()), (Some(1), Some("First")));
        assert_eq!(title_from_stem("1-02 Second"), (Some(2), "Second".into()));
        assert_eq!(title_from_stem("1979"), (None, "1979".into()));
    }

    #[test]
    fn volume_roots() {
        assert_eq!(
            volume_root("/storage/emulated/0/Music/a.mp3"),
            Some("/storage/emulated/0")
        );
        assert_eq!(
            volume_root("/storage/3564-3463/Music/a.mp3"),
            Some("/storage/3564-3463")
        );
        assert_eq!(volume_root("/sdcard/a.mp3"), None);
    }
}
