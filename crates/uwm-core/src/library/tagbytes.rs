//! Tags edited as bytes. An MP3's ID3v2 frames and a FLAC's metadata blocks are copied
//! as they are stored and only what a change is about is rebuilt. lofty re-encodes a
//! whole tag on every write and leaves out what it could not read or does not write
//! (a v2.3 `TYER` of "20080711", the iTunes sort frames in v2.3, Vorbis fields such as
//! `ALBUM ARTIST` or `TOTALTRACKS`), which a write about one field must not do.

use std::borrow::Cow;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};

use lofty::file::FileType;

use super::scan::repair_legacy;
use super::sensme;
use super::tags::{Cover, TagEdit};
use crate::error::{Error, Result, Why};

/// What a rebuild changes.
#[derive(Clone, Copy)]
pub(super) enum Change<'a> {
    /// Put this SMFMF block where the player reads it, in place of any the file had.
    Smfmf(&'a [u8]),
    /// Write these fields and this front cover.
    Edit(&'a TagEdit, Option<&'a Cover>),
}

fn syncsafe(b: &[u8]) -> usize {
    b.iter().fold(0usize, |n, b| (n << 7) | usize::from(b & 0x7f))
}

fn syncsafe_bytes(n: usize) -> [u8; 4] {
    [
        (n >> 21) as u8 & 0x7f,
        (n >> 14) as u8 & 0x7f,
        (n >> 7) as u8 & 0x7f,
        n as u8 & 0x7f,
    ]
}

/// What comes before the audio: the whole ID3v2 tag of an MP3 (nothing when it has
/// none), the `fLaC` mark and metadata blocks of a FLAC.
pub(super) fn head_of(file: &mut fs::File, kind: FileType) -> Result<Vec<u8>> {
    let bad = || Error::unsupported(Why::TagsUnreadable, "the file's tags could not be read");
    file.seek(SeekFrom::Start(0))?;
    let mut head = Vec::new();
    match kind {
        FileType::Mpeg => {
            let mut h = [0u8; 10];
            if file.read_exact(&mut h).is_err() || &h[..3] != b"ID3" {
                return Ok(head);
            }
            head.extend_from_slice(&h);
            head.resize(10 + syncsafe(&h[6..10]), 0);
            file.read_exact(&mut head[10..]).map_err(|_| bad())?;
        }
        FileType::Flac => {
            head.resize(4, 0);
            file.read_exact(&mut head).map_err(|_| bad())?;
            if head != b"fLaC" {
                return Err(bad());
            }
            loop {
                let at = head.len();
                head.resize(at + 4, 0);
                file.read_exact(&mut head[at..]).map_err(|_| bad())?;
                let h = &head[at..];
                let len = (usize::from(h[1]) << 16) | (usize::from(h[2]) << 8) | usize::from(h[3]);
                let last = h[0] & 0x80 != 0;
                head.resize(at + 4 + len, 0);
                file.read_exact(&mut head[at + 4..]).map_err(|_| bad())?;
                if last {
                    break;
                }
            }
        }
        _ => return Err(bad()),
    }
    Ok(head)
}

/// The ID3v1 tag in the last 128 bytes of a file of `size` bytes, if there is one.
pub(super) fn v1_of(file: &mut fs::File, size: u64) -> Option<[u8; 128]> {
    let mut v1 = [0u8; 128];
    file.seek(SeekFrom::Start(size.checked_sub(128)?)).ok()?;
    file.read_exact(&mut v1).ok()?;
    (&v1[..3] == b"TAG").then_some(v1)
}

/// An ID3v2.3 or 2.4 tag as it is stored, each frame with its header. None for other
/// versions and for tags with unsynchronisation, an extended header, a footer or data
/// after the frames: those are not edited byte by byte.
struct RawTag<'a> {
    major: u8,
    revision: u8,
    flags: u8,
    size: usize,
    frames: Vec<&'a [u8]>,
}

fn raw_tag(head: &[u8]) -> Option<RawTag<'_>> {
    if head.len() < 10 || &head[..3] != b"ID3" {
        return None;
    }
    let (major, flags) = (head[3], head[5]);
    if !(3..=4).contains(&major) || flags & 0xd0 != 0 {
        return None;
    }
    let size = syncsafe(&head[6..10]);
    let tag = head.get(10..10 + size)?;
    let mut frames = Vec::new();
    let mut pos = 0;
    while pos + 10 <= tag.len() && tag[pos] != 0 {
        let len = if major == 4 {
            syncsafe(&tag[pos + 4..pos + 8])
        } else {
            u32::from_be_bytes(tag[pos + 4..pos + 8].try_into().ok()?) as usize
        };
        frames.push(tag.get(pos..pos + 10 + len)?);
        pos += 10 + len;
    }
    if tag[pos.min(tag.len())..].iter().any(|b| *b != 0) {
        return None;
    }
    Some(RawTag {
        major,
        revision: head[4],
        flags,
        size,
        frames,
    })
}

/// A stored frame as the frame checks count it: analysis apart from other `GEOB`s.
/// A frame that is compressed or encrypted is never taken for analysis.
fn raw_key(frame: &[u8]) -> &[u8] {
    if &frame[..4] == b"GEOB" && frame[9] == 0 && sensme::geob_is_smfmf(&frame[10..]) {
        b"SMFMF"
    } else {
        &frame[..4]
    }
}

/// The tag `like` describes holding `frames` instead. It keeps its size when they fit,
/// so the audio stays where it was, and otherwise grows with room to spare.
fn tag_with(like: &RawTag<'_>, frames: &[&[u8]]) -> Result<Vec<u8>> {
    let content: usize = frames.iter().map(|f| f.len()).sum();
    let size = if content <= like.size {
        like.size
    } else {
        content + 1024
    };
    if size >= 1 << 28 {
        return Err(Error::unsupported(Why::Id3TooLarge, "the ID3 tag would be too large"));
    }
    let mut out = vec![b'I', b'D', b'3', like.major, like.revision, like.flags];
    out.extend_from_slice(&syncsafe_bytes(size));
    for f in frames {
        out.extend_from_slice(f);
    }
    out.resize(10 + size, 0);
    Ok(out)
}

fn frame_bytes(major: u8, id: &[u8], status: u8, body: &[u8]) -> Vec<u8> {
    let mut frame = id.to_vec();
    if major == 4 {
        frame.extend_from_slice(&syncsafe_bytes(body.len()));
    } else {
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    }
    frame.extend_from_slice(&[status, 0]);
    frame.extend_from_slice(body);
    frame
}

/// The `GEOB` frame Sony's software keeps the analysis in: Latin-1, no file name, the
/// description `USR_SMFMF`.
fn smfmf_frame(major: u8, blob: &[u8]) -> Vec<u8> {
    let mut body = b"\0application/SMFMF\0\0USR_SMFMF\0".to_vec();
    body.extend_from_slice(blob);
    frame_bytes(major, b"GEOB", 0, &body)
}

/// An ID3v1 tag's fields as ID3v2.3 frames, the text bytes as they were (often CP949).
/// Players that find an ID3v2 tag do not read ID3v1 (Android's scanner among them), so
/// a new ID3v2 tag has to carry them.
fn frames_from_v1(v1: &[u8]) -> Vec<Vec<u8>> {
    let field = |b: &[u8]| -> Vec<u8> {
        let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
        let mut text = vec![0u8];
        text.extend_from_slice(b[..end].trim_ascii_end());
        text
    };
    let v11 = v1[125] == 0 && v1[126] != 0;
    let mut frames = Vec::new();
    for (id, range) in [(b"TIT2", 3..33), (b"TPE1", 33..63), (b"TALB", 63..93)] {
        let body = field(&v1[range]);
        if body.len() > 1 {
            frames.push(frame_bytes(3, id, 0, &body));
        }
    }
    if v1[93..97].iter().all(u8::is_ascii_digit) {
        frames.push(frame_bytes(3, b"TYER", 0, &field(&v1[93..97])));
    }
    let comment = field(&v1[97..if v11 { 125 } else { 127 }]);
    if comment.len() > 1 {
        let mut body = b"\0XXX\0".to_vec();
        body.extend_from_slice(&comment[1..]);
        frames.push(frame_bytes(3, b"COMM", 0, &body));
    }
    if v11 {
        frames.push(frame_bytes(3, b"TRCK", 0, format!("\0{}", v1[126]).as_bytes()));
    }
    if v1[127] != 255 {
        frames.push(frame_bytes(3, b"TCON", 0, format!("\0({})", v1[127]).as_bytes()));
    }
    frames
}

/// The text encoding to write: Latin-1 for plain ASCII, otherwise UTF-16 with a byte
/// order mark in ID3v2.3 and UTF-8 in 2.4.
fn encoding_for(major: u8, texts: &[&str]) -> u8 {
    if texts.iter().all(|t| t.is_ascii()) {
        0
    } else if major == 4 {
        3
    } else {
        1
    }
}

fn encode(encoding: u8, text: &str) -> Vec<u8> {
    if encoding == 1 {
        let mut out = vec![0xff, 0xfe];
        out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        out
    } else {
        text.as_bytes().to_vec()
    }
}

fn terminator(encoding: u8) -> &'static [u8] {
    if matches!(encoding, 1 | 2) { &[0, 0] } else { &[0] }
}

fn text_body(major: u8, text: &str) -> Vec<u8> {
    let e = encoding_for(major, &[text]);
    let mut body = vec![e];
    body.extend(encode(e, text));
    body
}

/// A text frame body's first string, in any of ID3's encodings.
fn frame_text(body: &[u8]) -> Option<String> {
    let (&encoding, rest) = body.split_first()?;
    Some(match encoding {
        0 => rest.iter().take_while(|b| **b != 0).map(|b| char::from(*b)).collect(),
        3 => String::from_utf8_lossy(rest.split(|b| *b == 0).next().unwrap_or_default()).into_owned(),
        1 | 2 => {
            let (little, rest) = match rest {
                [0xff, 0xfe, r @ ..] => (true, r),
                [0xfe, 0xff, r @ ..] => (false, r),
                r => (false, r),
            };
            let units = rest.as_chunks::<2>().0.iter().map(|c| {
                if little {
                    u16::from_le_bytes(*c)
                } else {
                    u16::from_be_bytes(*c)
                }
            });
            char::decode_utf16(units.take_while(|u| *u != 0))
                .map(|c| c.unwrap_or('\u{fffd}'))
                .collect()
        }
        _ => return None,
    })
}

/// Text frames whose legacy-encoded text is written back decoded.
const REPAIRED: [&[u8]; 6] = [b"TIT2", b"TPE1", b"TALB", b"TPE2", b"TCON", b"TCOM"];

/// A frame holding CP949 or Shift_JIS text marked Latin-1, written back as Unicode;
/// None when there is nothing to repair.
fn repaired(major: u8, f: &[u8]) -> Option<Vec<u8>> {
    let (id, status, body) = (&f[..4], f[8], &f[10..]);
    if f[9] != 0 || body.first() != Some(&0) {
        return None;
    }
    if REPAIRED.contains(&id) {
        let text = frame_text(body)?;
        let fixed = repair_legacy(&text);
        return (fixed != text).then(|| frame_bytes(major, id, status, &text_body(major, &fixed)));
    }
    if id == b"COMM" && body.len() >= 4 {
        let rest = &body[4..];
        let split = rest.iter().position(|b| *b == 0)?;
        let latin1 = |b: &[u8]| -> String { b.iter().take_while(|c| **c != 0).map(|c| char::from(*c)).collect() };
        let (desc, text) = (latin1(&rest[..split]), latin1(&rest[split + 1..]));
        let (d, t) = (repair_legacy(&desc), repair_legacy(&text));
        if d == desc && t == text {
            return None;
        }
        let e = encoding_for(major, &[&d, &t]);
        let mut out = vec![e];
        out.extend_from_slice(&body[1..4]);
        out.extend(encode(e, &d));
        out.extend_from_slice(terminator(e));
        out.extend(encode(e, &t));
        return Some(frame_bytes(major, id, status, &out));
    }
    None
}

/// The picture type of an `APIC` body.
fn apic_type(body: &[u8]) -> Option<u8> {
    let mime_end = 1 + body.get(1..)?.iter().position(|b| *b == 0)?;
    body.get(mime_end + 1).copied()
}

/// "n/total" with either half replaced, the other kept from what the file had.
fn number_pair(current: Option<&str>, n: Option<u32>, total: Option<u32>) -> Option<String> {
    if n.is_none() && total.is_none() {
        return None;
    }
    let current = current.unwrap_or_default();
    let (cn, ct) = current.split_once('/').unwrap_or((current, ""));
    let n = n.map_or_else(|| cn.trim().to_owned(), |n| n.to_string());
    let t = total.map_or_else(|| ct.trim().to_owned(), |t| t.to_string());
    Some(if t.is_empty() { n } else { format!("{n}/{t}") })
}

/// An ID3v2 tag's frames with the edit applied. The frames it does not touch are kept
/// as they are stored, except legacy-encoded text, which is written back decoded.
fn edit_frames(major: u8, frames: &[&[u8]], edit: &TagEdit, cover: Option<&Cover>) -> Vec<Vec<u8>> {
    let current = |id: &[u8]| {
        frames
            .iter()
            .find(|f| &f[..4] == id && f[9] == 0)
            .and_then(|f| frame_text(&f[10..]))
    };
    let mut set: Vec<(&[u8], String)> = Vec::new();
    let fields: [(&[u8], &Option<String>); 5] = [
        (b"TIT2", &edit.title),
        (b"TPE1", &edit.artist),
        (b"TALB", &edit.album),
        (b"TPE2", &edit.album_artist),
        (b"TCON", &edit.genre),
    ];
    for (id, value) in fields {
        if let Some(v) = value {
            set.push((id, v.clone()));
        }
    }
    if let Some(y) = edit.year {
        let id: &[u8] = if major == 4 { b"TDRC" } else { b"TYER" };
        set.push((id, y.to_string()));
    }
    if let Some(v) = number_pair(current(b"TRCK").as_deref(), edit.track, edit.track_total) {
        set.push((b"TRCK".as_slice(), v));
    }
    if let Some(v) = number_pair(current(b"TPOS").as_deref(), edit.disc, edit.disc_total) {
        set.push((b"TPOS".as_slice(), v));
    }
    let dates: [&[u8]; 5] = [b"TYER", b"TDAT", b"TIME", b"TRDA", b"TDRC"];
    let mut out = Vec::new();
    for f in frames {
        let id = &f[..4];
        let gone = set.iter().any(|(s, _)| *s == id)
            || (edit.year.is_some() && dates.contains(&id))
            || (cover.is_some() && id == b"APIC" && f[9] == 0 && apic_type(&f[10..]) == Some(3));
        if !gone {
            out.push(repaired(major, f).unwrap_or_else(|| f.to_vec()));
        }
    }
    for (id, v) in set {
        out.push(frame_bytes(major, id, 0, &text_body(major, &v)));
    }
    if let Some(c) = cover {
        let mut body = vec![0];
        body.extend_from_slice(c.mime.as_bytes());
        body.extend_from_slice(&[0, 3, 0]);
        body.extend_from_slice(&c.data);
        out.push(frame_bytes(major, b"APIC", 0, &body));
    }
    out
}

/// An MP3's new ID3v2 tag. A file without one gets an ID3v2.3 tag, as Sony's files
/// have, holding what its ID3v1 tag says. None when the tag is of a kind that is not
/// edited byte by byte.
pub(super) fn mp3_head(head: &[u8], v1: Option<&[u8]>, change: Change<'_>) -> Result<Option<Vec<u8>>> {
    let from_v1: Vec<Vec<u8>>;
    let tag = if head.is_empty() {
        from_v1 = v1.map(frames_from_v1).unwrap_or_default();
        RawTag {
            major: 3,
            revision: 0,
            flags: 0,
            size: 0,
            frames: from_v1.iter().map(Vec::as_slice).collect(),
        }
    } else {
        match raw_tag(head) {
            Some(tag) => tag,
            None => return Ok(None),
        }
    };
    let new = match change {
        Change::Smfmf(blob) => {
            let mut kept: Vec<Vec<u8>> = tag
                .frames
                .iter()
                .filter(|f| raw_key(f) != b"SMFMF".as_slice())
                .map(|f| f.to_vec())
                .collect();
            kept.push(smfmf_frame(tag.major, blob));
            kept
        }
        Change::Edit(edit, cover) => edit_frames(tag.major, &tag.frames, edit, cover),
    };
    let refs: Vec<&[u8]> = new.iter().map(Vec::as_slice).collect();
    tag_with(&tag, &refs).map(Some)
}

/// Puts back frames lofty dropped from an MP3 it wrote, byte for byte as the original
/// had them. Frames the edit is about are left as lofty wrote them.
pub(super) fn restore_frames(file: &mut fs::File, original: &[u8], changes: &[&[u8]]) -> Result<()> {
    let Some(before) = raw_tag(original) else {
        return Ok(());
    };
    let head = head_of(file, FileType::Mpeg)?;
    let Some(after) = raw_tag(&head) else {
        return Ok(());
    };
    if after.major != before.major {
        return Ok(());
    }
    let mut frames = after.frames.clone();
    for (i, frame) in before.frames.iter().enumerate() {
        let key = raw_key(frame);
        if changes.contains(&key) || before.frames[..i].iter().any(|f| raw_key(f) == key) {
            continue;
        }
        let had: Vec<&[u8]> = before.frames.iter().copied().filter(|f| raw_key(f) == key).collect();
        let has = after.frames.iter().filter(|f| raw_key(f) == key).count();
        frames.extend(had.into_iter().skip(has));
    }
    if frames.len() == after.frames.len() {
        return Ok(());
    }
    let tag = tag_with(&after, &frames)?;
    let mut rest = Vec::new();
    file.seek(SeekFrom::Start(head.len() as u64))?;
    file.read_to_end(&mut rest)?;
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    file.write_all(&tag)?;
    file.write_all(&rest)?;
    Ok(())
}

const STREAMINFO: u8 = 0;
const PADDING: u8 = 1;
const APPLICATION: u8 = 2;
const VORBIS_COMMENT: u8 = 4;
const PICTURE: u8 = 6;

/// A Vorbis comment block: the vendor string and the `KEY=value` entries.
fn comments(body: &[u8]) -> Option<(&[u8], Vec<&[u8]>)> {
    let le32 =
        |at: usize| -> Option<usize> { Some(u32::from_le_bytes(body.get(at..at + 4)?.try_into().ok()?) as usize) };
    let vendor_len = le32(0)?;
    let vendor = body.get(4..4 + vendor_len)?;
    let count = le32(4 + vendor_len)?;
    let mut pos = 8 + vendor_len;
    let mut entries = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let len = le32(pos)?;
        entries.push(body.get(pos + 4..pos + 4 + len)?);
        pos += 4 + len;
    }
    Some((vendor, entries))
}

fn comments_body(vendor: &[u8], entries: &[Cow<'_, [u8]>]) -> Vec<u8> {
    let mut out = (vendor.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(vendor);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        out.extend_from_slice(&(e.len() as u32).to_le_bytes());
        out.extend_from_slice(e);
    }
    out
}

/// A Vorbis entry's field name as stored and its value, when the value is UTF-8.
fn split_entry(entry: &[u8]) -> Option<(&[u8], &str)> {
    let at = entry.iter().position(|b| *b == b'=')?;
    Some((&entry[..at], std::str::from_utf8(&entry[at + 1..]).ok()?))
}

fn entry_key(entry: &[u8]) -> String {
    let end = entry.iter().position(|b| *b == b'=').unwrap_or(entry.len());
    String::from_utf8_lossy(&entry[..end]).to_ascii_uppercase()
}

/// A Vorbis number field and the names its total is stored under.
const TRACK: [&str; 3] = ["TRACKNUMBER", "TRACKTOTAL", "TOTALTRACKS"];
const DISC: [&str; 3] = ["DISCNUMBER", "DISCTOTAL", "TOTALDISCS"];

/// The entries a number edit sets. The number may carry its total after a slash, which
/// then follows a total edit; the half not edited stays.
fn number_fields<'a>(
    current: Option<&str>,
    names: &'a [&'a str; 3],
    n: Option<u32>,
    total: Option<u32>,
) -> Vec<(&'a [&'a str], String)> {
    let slash = current.is_some_and(|c| c.contains('/'));
    let mut out = Vec::new();
    if let Some(v) = number_pair(current, n, total.filter(|_| slash)) {
        out.push((&names[..1], v));
    }
    if let Some(t) = total {
        out.push((&names[1..], t.to_string()));
    }
    out
}

/// Vorbis fields whose legacy-encoded text is written back decoded.
const REPAIRED_FIELDS: [&str; 7] = [
    "TITLE",
    "ARTIST",
    "ALBUM",
    "ALBUMARTIST",
    "GENRE",
    "COMPOSER",
    "COMMENT",
];

/// A Vorbis comment block with the edit applied. Each field the edit sets replaces the
/// entries spelled any of the ways it is known by; every other entry is kept.
fn edit_comments(body: &[u8], edit: &TagEdit) -> Result<Vec<u8>> {
    let (vendor, entries) = comments(body)
        .ok_or_else(|| Error::unsupported(Why::FlacUnreadable, "the FLAC's comments could not be read"))?;
    let first = |key: &str| {
        entries
            .iter()
            .find(|e| entry_key(e) == key)
            .and_then(|e| split_entry(e))
            .map(|(_, v)| v)
    };
    let mut set: Vec<(&[&str], String)> = Vec::new();
    let fields: [(&[&str], &Option<String>); 5] = [
        (&["TITLE"], &edit.title),
        (&["ARTIST"], &edit.artist),
        (&["ALBUM"], &edit.album),
        (&["ALBUMARTIST", "ALBUM ARTIST", "ALBUM_ARTIST"], &edit.album_artist),
        (&["GENRE"], &edit.genre),
    ];
    for (keys, value) in fields {
        if let Some(v) = value {
            set.push((keys, v.clone()));
        }
    }
    if let Some(y) = edit.year {
        set.push((&["DATE", "YEAR"][..], y.to_string()));
    }
    set.extend(number_fields(
        first("TRACKNUMBER"),
        &TRACK,
        edit.track,
        edit.track_total,
    ));
    set.extend(number_fields(first("DISCNUMBER"), &DISC, edit.disc, edit.disc_total));
    let mut out: Vec<Cow<'_, [u8]>> = Vec::new();
    for e in &entries {
        let key = entry_key(e);
        if set.iter().any(|(keys, _)| keys.contains(&key.as_str())) {
            continue;
        }
        let fixed = split_entry(e)
            .filter(|_| REPAIRED_FIELDS.contains(&key.as_str()))
            .map(|(name, v)| (name, repair_legacy(v), v))
            .filter(|(_, r, v)| r != v);
        match fixed {
            Some((name, r, _)) => {
                let mut entry = name.to_vec();
                entry.push(b'=');
                entry.extend_from_slice(r.as_bytes());
                out.push(Cow::Owned(entry));
            }
            None => out.push(Cow::Borrowed(e)),
        }
    }
    for (keys, v) in set {
        out.push(Cow::Owned(format!("{}={v}", keys[0]).into_bytes()));
    }
    Ok(comments_body(vendor, &out))
}

/// A FLAC `PICTURE` block body for a front cover; the size fields are left unknown.
fn picture_block(c: &Cover) -> Vec<u8> {
    let mut b = 3u32.to_be_bytes().to_vec();
    b.extend_from_slice(&(c.mime.len() as u32).to_be_bytes());
    b.extend_from_slice(c.mime.as_bytes());
    b.extend_from_slice(&[0; 4 * 5]);
    b.extend_from_slice(&(c.data.len() as u32).to_be_bytes());
    b.extend_from_slice(&c.data);
    b
}

/// A FLAC's new metadata. Changed blocks go where they were, new ones before the
/// padding, and the padding gives up or takes the room the change needs so that the
/// audio stays where it was when it can.
pub(super) fn flac_head(head: &[u8], change: Change<'_>) -> Result<Vec<u8>> {
    let mut blocks: Vec<(u8, Cow<'_, [u8]>)> = Vec::new();
    let mut pos = 4;
    while pos + 4 <= head.len() {
        let h = &head[pos..pos + 4];
        let len = (usize::from(h[1]) << 16) | (usize::from(h[2]) << 8) | usize::from(h[3]);
        blocks.push((h[0] & 0x7f, Cow::Borrowed(&head[pos + 4..pos + 4 + len])));
        pos += 4 + len;
    }
    if blocks.first().map(|b| b.0) != Some(STREAMINFO) {
        return Err(Error::unsupported(
            Why::FlacUnreadable,
            "the FLAC metadata could not be read",
        ));
    }
    let had_padding = blocks.iter().any(|b| b.0 == PADDING);
    blocks.retain(|b| b.0 != PADDING);
    match change {
        Change::Smfmf(blob) => {
            blocks.retain(|(kind, body)| !(*kind == APPLICATION && body.starts_with(b"SMFM")));
            let mut app = b"SMFM".to_vec();
            app.extend_from_slice(blob);
            blocks.push((APPLICATION, Cow::Owned(app)));
        }
        Change::Edit(edit, cover) => {
            match blocks.iter().position(|b| b.0 == VORBIS_COMMENT) {
                Some(i) => blocks[i].1 = Cow::Owned(edit_comments(&blocks[i].1, edit)?),
                None => {
                    let empty = comments_body(b"uwm", &[]);
                    blocks.push((VORBIS_COMMENT, Cow::Owned(edit_comments(&empty, edit)?)));
                }
            }
            if let Some(c) = cover {
                blocks.retain(|(kind, body)| !(*kind == PICTURE && body.get(..4) == Some(&3u32.to_be_bytes()[..])));
                blocks.push((PICTURE, Cow::Owned(picture_block(c))));
            }
        }
    }
    if blocks.iter().any(|b| b.1.len() > 0xff_ffff) {
        return Err(Error::unsupported(
            Why::FlacBlockTooLarge,
            "a FLAC metadata block would be too large",
        ));
    }
    let content = 4 + blocks.iter().map(|b| 4 + b.1.len()).sum::<usize>();
    match head.len().checked_sub(content + 4) {
        Some(room) => blocks.push((PADDING, Cow::Owned(vec![0; room]))),
        None if had_padding => blocks.push((PADDING, Cow::Owned(vec![0; 1024]))),
        None => {}
    }
    let mut out = b"fLaC".to_vec();
    let last = blocks.len() - 1;
    for (i, (kind, body)) in blocks.iter().enumerate() {
        out.push(kind | if i == last { 0x80 } else { 0 });
        out.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        out.extend_from_slice(body);
    }
    Ok(out)
}
