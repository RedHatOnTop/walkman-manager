use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use lofty::file::FileType;
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag};
use serde::Serialize;
use walkdir::WalkDir;

use super::art::{ArtCache, folder_image};
use super::{AudioFormat, Codec, INDEX_VERSION, Index, Skipped, Track, dsd, group_albums, hex_id, sensme};

const WORKERS: usize = 4;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub found: usize,
    pub done: usize,
    pub reused: usize,
}

struct Found {
    root: usize,
    path: PathBuf,
    rel: String,
    codec: Codec,
    size: u64,
    modified: i64,
    lyrics: Option<String>,
}

fn unix_secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn rel_of(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect();
    Some(parts.join("/"))
}

/// Walks the roots for audio files, skipping hidden entries, and notes the
/// sidecar lyrics and folder images on the way.
fn walk(roots: &[PathBuf], cancel: &AtomicBool) -> (Vec<Found>, BTreeMap<PathBuf, Vec<String>>) {
    let mut found = Vec::new();
    let mut dirs: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for (ri, root) in roots.iter().enumerate() {
        let walker = WalkDir::new(root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
        for entry in walker.filter_map(Result::ok) {
            if cancel.load(Ordering::Relaxed) {
                return (found, dirs);
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(dir) = path.parent() {
                dirs.entry(dir.to_path_buf()).or_default().push(name);
            }
            let Some(codec) = Codec::from_path(path) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let Some(rel) = rel_of(root, path) else {
                continue;
            };
            found.push(Found {
                root: ri,
                path: path.to_path_buf(),
                rel,
                codec,
                size: meta.len(),
                modified: meta.modified().map_or(0, unix_secs),
                lyrics: None,
            });
        }
    }
    for f in &mut found {
        let (Some(dir), Some(stem)) = (f.path.parent(), f.path.file_stem()) else {
            continue;
        };
        let want = format!("{}.lrc", stem.to_string_lossy()).to_lowercase();
        if let Some(name) = dirs
            .get(dir)
            .and_then(|names| names.iter().find(|n| n.to_lowercase() == want))
        {
            f.lyrics = rel_of(&roots[f.root], &dir.join(name));
        }
    }
    (found, dirs)
}

fn text(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|v| repair_legacy(v.trim())).filter(|v| !v.is_empty())
}

fn is_kana_or_kanji(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30ff}' | '\u{4e00}'..='\u{9fff}')
}

/// Old Korean and Japanese MP3s keep CP949 or Shift_JIS bytes in ID3 frames marked
/// Latin-1, which lofty returns one character per byte ("ÀÌ³ë·¡" for "이노래").
/// Android's media scanner guesses a single-byte charset per string instead and
/// sometimes lands on ISO-8859-2 ("ťçśűŔÇ ˛Ţ" for "사랑의 꿈"). Both are turned back
/// into bytes and decoded again when the bytes look like a double-byte encoding.
pub fn repair_legacy(s: &str) -> String {
    let repaired = if s.chars().all(|c| c as u32 <= 0xff) {
        let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
        decode_legacy(&bytes, false)
    } else {
        let (bytes, _, unmappable) = encoding_rs::ISO_8859_2.encode(s);
        if unmappable { None } else { decode_legacy(&bytes, true) }
    };
    repaired.unwrap_or_else(|| s.to_owned())
}

/// Only bytes that single-byte text would not produce are decoded again: C1 control
/// bytes (Shift_JIS lead bytes) or two adjacent bytes at 0xA1 or above. Shift_JIS wins
/// when it yields kana or kanji without half-width katakana, which is what Korean text
/// turns into under Shift_JIS. `strict` is for a guessed charset, where a Central
/// European word can pass the byte test: every non-ASCII character must then come out
/// as Hangul, and no syllable may touch a Latin letter.
fn decode_legacy(bytes: &[u8], strict: bool) -> Option<String> {
    let c1_control = bytes.iter().any(|b| (0x80..0xa0).contains(b));
    if !c1_control && !bytes.windows(2).any(|w| w[0] >= 0xa1 && w[1] >= 0xa1) {
        return None;
    }
    let (jp, jp_bad) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes);
    if !jp_bad && jp.chars().any(is_kana_or_kanji) && !jp.chars().any(|c| ('\u{ff61}'..='\u{ff9f}').contains(&c)) {
        return Some(jp.into_owned());
    }
    let (ko, ko_bad) = encoding_rs::EUC_KR.decode_without_bom_handling(bytes);
    let hangul = |c: char| ('\u{ac00}'..='\u{d7a3}').contains(&c);
    if ko_bad || !ko.chars().any(hangul) {
        return None;
    }
    if strict {
        let latin = |c: char| c.is_alphabetic() && !hangul(c);
        let chars: Vec<char> = ko.chars().collect();
        let touching = chars
            .windows(2)
            .any(|w| (hangul(w[0]) && latin(w[1])) || (latin(w[0]) && hangul(w[1])));
        if touching || chars.iter().any(|c| !c.is_ascii() && !hangul(*c)) {
            return None;
        }
    }
    Some(ko.into_owned())
}

fn apply_tag(t: &mut Track, tag: &Tag, art: &ArtCache) {
    t.legacy_text = [
        ItemKey::TrackTitle,
        ItemKey::TrackArtist,
        ItemKey::AlbumTitle,
        ItemKey::AlbumArtist,
        ItemKey::Genre,
    ]
    .into_iter()
    .filter_map(|k| tag.get_string(k))
    .any(|v| repair_legacy(v.trim()) != v.trim());
    t.title = text(tag.title());
    t.artist = text(tag.artist());
    t.album = text(tag.album());
    t.album_artist = tag
        .get_string(ItemKey::AlbumArtist)
        .map(|s| repair_legacy(s.trim()))
        .filter(|s| !s.is_empty());
    t.genre = text(tag.genre());
    t.track = tag.track();
    t.disc = tag.disk();
    t.year = tag.date().map(|d| d.year).filter(|y| *y > 0);
    let pictures = tag.pictures();
    t.cover = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .and_then(|p| art.store(p.data()));
}

fn read_track(f: &Found, art: &ArtCache) -> Result<Track, String> {
    let mut t = Track {
        id: hex_id(f.path.to_string_lossy().as_bytes()),
        path: f.path.to_string_lossy().into_owned(),
        rel: f.rel.clone(),
        root: f.root,
        size: f.size,
        modified: f.modified,
        title: None,
        artist: None,
        album: None,
        album_artist: None,
        genre: None,
        track: None,
        disc: None,
        year: None,
        duration_ms: 0,
        format: AudioFormat {
            codec: f.codec,
            sample_rate: None,
            bits: None,
            channels: None,
            bitrate_kbps: None,
        },
        cover: None,
        lyrics: f.lyrics.clone(),
        legacy_text: false,
        sensme: None,
    };
    match f.codec {
        Codec::Dsf | Codec::Dff => {
            let d = dsd::read(&f.path).ok_or("not a DSD stream file")?;
            t.format.sample_rate = Some(d.info.rate);
            t.format.bits = Some(1);
            t.format.channels = u8::try_from(d.info.channels).ok();
            t.duration_ms = d.info.duration_ms;
            if let Some(tag) = &d.tag {
                apply_tag(&mut t, tag, art);
            }
        }
        // lofty does not read ASF; a WMA file keeps its name as its title.
        Codec::Wma => {}
        _ => {
            let file = lofty::read_from_path(&f.path).map_err(|e| e.to_string())?;
            let props = file.properties();
            t.format.codec = match file.file_type() {
                FileType::Mp4 if props.bit_depth().is_some() => Codec::Alac,
                FileType::Mp4 | FileType::Aac => Codec::Aac,
                FileType::Flac => Codec::Flac,
                FileType::Wav => Codec::Wav,
                FileType::Aiff => Codec::Aiff,
                FileType::Ape => Codec::Ape,
                FileType::Mpeg => Codec::Mp3,
                FileType::Vorbis | FileType::Speex => Codec::Vorbis,
                FileType::Opus => Codec::Opus,
                FileType::WavPack => Codec::WavPack,
                FileType::Mpc => Codec::Musepack,
                _ => f.codec,
            };
            t.format.sample_rate = props.sample_rate();
            t.format.bits = props.bit_depth();
            t.format.channels = props.channels();
            t.format.bitrate_kbps = props.audio_bitrate().or(props.overall_bitrate());
            t.duration_ms = props.duration().as_millis() as u64;
            if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
                apply_tag(&mut t, tag, art);
            }
            // Walkman firmware reads the analysis from these three containers only.
            if matches!(t.format.codec, Codec::Mp3 | Codec::Flac | Codec::Aac | Codec::Alac) {
                t.sensme = sensme::read(&f.path);
            }
        }
    }
    if t.title.is_none() {
        t.title = f.path.file_stem().map(|s| s.to_string_lossy().into_owned());
    }
    Ok(t)
}

/// Builds the index for `roots`. Tracks whose size and modification time match the
/// previous index are reused without opening the file.
pub fn scan(
    roots: &[PathBuf],
    previous: Option<&Index>,
    art: &ArtCache,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&ScanProgress) + Sync),
) -> Index {
    let (found, dirs) = walk(roots, cancel);
    let known: HashMap<&str, &Track> = previous
        .filter(|p| p.version == INDEX_VERSION)
        .map(|p| p.tracks.iter().map(|t| (t.path.as_str(), t)).collect())
        .unwrap_or_default();

    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let reused = AtomicUsize::new(0);
    let results: Mutex<Vec<Result<Track, Skipped>>> = Mutex::new(Vec::with_capacity(found.len()));
    let report = || {
        progress(&ScanProgress {
            found: found.len(),
            done: done.load(Ordering::Relaxed),
            reused: reused.load(Ordering::Relaxed),
        })
    };
    report();
    std::thread::scope(|s| {
        for _ in 0..WORKERS {
            s.spawn(|| {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(f) = found.get(i) else { break };
                    let path = f.path.to_string_lossy();
                    let result = match known.get(&*path) {
                        Some(old) if old.size == f.size && old.modified == f.modified => {
                            reused.fetch_add(1, Ordering::Relaxed);
                            let mut t = (*old).clone();
                            t.root = f.root;
                            t.rel = f.rel.clone();
                            t.lyrics = f.lyrics.clone();
                            Ok(t)
                        }
                        _ => read_track(f, art).map_err(|reason| Skipped {
                            path: path.into_owned(),
                            reason,
                        }),
                    };
                    results.lock().unwrap().push(result);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(25) {
                        report();
                    }
                }
            });
        }
    });
    report();

    let mut tracks = Vec::new();
    let mut skipped = Vec::new();
    for r in results.into_inner().unwrap() {
        match r {
            Ok(t) => tracks.push(t),
            Err(s) => skipped.push(s),
        }
    }
    tracks.sort_by(|a, b| (a.root, &a.rel).cmp(&(b.root, &b.rel)));
    skipped.sort_by(|a, b| a.path.cmp(&b.path));

    let albums = group_albums(&tracks, |ts| {
        if let Some(key) = ts.iter().find_map(|t| t.cover.clone()) {
            return Some(key);
        }
        let first = Path::new(&ts[0].path);
        let candidates = [first.parent(), first.parent().and_then(Path::parent)];
        candidates.into_iter().flatten().find_map(|dir| {
            let names = dirs.get(dir)?;
            let name = folder_image(names.iter().map(String::as_str))?;
            art.store_file(&dir.join(name))
        })
    });

    Index {
        version: INDEX_VERSION,
        roots: roots.iter().map(|r| r.to_string_lossy().into_owned()).collect(),
        scanned_at_ms: now_ms(),
        tracks,
        albums,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn latin1(bytes: &[u8]) -> String {
        bytes.iter().map(|b| *b as char).collect()
    }

    #[test]
    fn legacy_tags_are_decoded_again() {
        let (cp949, _, _) = encoding_rs::EUC_KR.encode("2AM - 이노래");
        assert_eq!(repair_legacy(&latin1(&cp949)), "2AM - 이노래");
        assert_eq!(latin1(&cp949[6..]), "ÀÌ³ë·¡");
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("あいみょん - マリーゴールド");
        assert_eq!(repair_legacy(&latin1(&sjis)), "あいみょん - マリーゴールド");
        let (kanji, _, _) = encoding_rs::SHIFT_JIS.encode("君はロックを聴かない");
        assert_eq!(repair_legacy(&latin1(&kanji)), "君はロックを聴かない");
        let (cp949, _, _) = encoding_rs::EUC_KR.encode("사랑의 꿈");
        let (guessed, _, _) = encoding_rs::ISO_8859_2.decode(&cp949);
        assert_eq!(guessed, "ťçśűŔÇ ˛Ţ");
        assert_eq!(repair_legacy(&guessed), "사랑의 꿈");
        assert_eq!(repair_legacy("Äđ(Cool)"), "쿨(Cool)");
        for keep in [
            "Björk",
            "Café del Mar",
            "Ægir",
            "Sigur Rós",
            "Mötley Crüe",
            "이미 유니코드",
            "Dvořák",
            "Jeść",
            "Łódź",
            "Žižek",
        ] {
            assert_eq!(repair_legacy(keep), keep);
        }
    }

    #[test]
    fn scans_a_folder_tree() {
        let base = std::env::temp_dir().join(format!("uwm-scan-test-{}", std::process::id()));
        let album = base.join("Artist/Album");
        fs::create_dir_all(&album).unwrap();
        fs::create_dir_all(base.join(".hidden")).unwrap();
        fs::write(album.join("01 Song.wma"), b"not really").unwrap();
        fs::write(album.join("01 song.LRC"), b"[00:01.00]la").unwrap();
        fs::write(album.join("02 Broken.flac"), b"nope").unwrap();
        fs::write(base.join(".hidden/x.flac"), b"nope").unwrap();
        fs::write(album.join("notes.txt"), b"").unwrap();

        let art = ArtCache::new(base.join("art"));
        let cancel = AtomicBool::new(false);
        let index = scan(std::slice::from_ref(&base), None, &art, &cancel, &|_| {});
        assert_eq!(index.tracks.len(), 1);
        let t = &index.tracks[0];
        assert_eq!(t.rel, "Artist/Album/01 Song.wma");
        assert_eq!(t.title.as_deref(), Some("01 Song"));
        assert_eq!(t.lyrics.as_deref(), Some("Artist/Album/01 song.LRC"));
        assert_eq!(index.skipped.len(), 1);
        assert!(index.skipped[0].path.ends_with("02 Broken.flac"));
        assert_eq!(index.albums.len(), 1);
        assert_eq!(index.albums[0].title, "Album");

        let again = scan(std::slice::from_ref(&base), Some(&index), &art, &cancel, &|_| {});
        assert_eq!(again.tracks.len(), 1);

        let mut kept = index.clone();
        kept.tracks[0].title = Some("kept".into());
        let reused = scan(std::slice::from_ref(&base), Some(&kept), &art, &cancel, &|_| {});
        assert_eq!(reused.tracks[0].title.as_deref(), Some("kept"));
        kept.version = INDEX_VERSION - 1;
        let reread = scan(std::slice::from_ref(&base), Some(&kept), &art, &cancel, &|_| {});
        assert_eq!(reread.tracks[0].title.as_deref(), Some("01 Song"));
        let _ = fs::remove_dir_all(base);
    }
}
