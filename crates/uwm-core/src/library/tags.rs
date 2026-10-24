//! Writes tags into music files. Only the fields asked for change; everything this does
//! not model, such as the SensMe analysis Sony's software keeps in an ID3 `GEOB` frame,
//! is carried over as it is. [`write_smfmf`] puts that analysis into a file itself.
//! MP3 and FLAC are edited as bytes ([`super::tagbytes`]); lofty writes the other
//! formats and the ID3 tags that cannot be.

use std::fs;
use std::io::{BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType};
use lofty::flac::FlacFile;
use lofty::id3::v2::{Frame, Id3v2Tag, Id3v2Version};
use lofty::mp4::Mp4File;
use lofty::mpeg::MpegFile;
use lofty::ogg::{OggPictureStorage, OpusFile, VorbisFile};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{ItemKey, ItemValue, MergeTag, SplitTag, Tag, TagItem};
use serde::{Deserialize, Serialize};

use super::scan::repair_legacy;
use super::sensme::{self, Id3Frames as Frames};
use super::tagbytes::{self, Change};
use crate::error::{Error, Result, Why};

/// New values for a file's tags. A field left `None` keeps what the file has.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagEdit {
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
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_total: Option<u32>,
}

/// A front cover to embed, replacing the one the file has.
#[derive(Debug, Clone)]
pub struct Cover {
    pub data: Vec<u8>,
    pub mime: &'static str,
}

/// Text that may have been stored in a legacy encoding; it is written back decoded.
const TEXT_KEYS: [ItemKey; 7] = [
    ItemKey::TrackTitle,
    ItemKey::TrackArtist,
    ItemKey::AlbumTitle,
    ItemKey::AlbumArtist,
    ItemKey::Genre,
    ItemKey::Composer,
    ItemKey::Comment,
];

fn repair_items(tag: &mut Tag) {
    for key in TEXT_KEYS {
        let items: Vec<TagItem> = tag.take(key).collect();
        for item in items {
            let fixed = match item.value() {
                ItemValue::Text(s) => {
                    let r = repair_legacy(s);
                    (r != *s).then_some(r)
                }
                _ => None,
            };
            let Some(text) = fixed else {
                tag.push_unchecked(item);
                continue;
            };
            let mut new = TagItem::new(key, ItemValue::Text(text));
            new.set_lang(*item.lang());
            new.set_description(item.description().to_owned());
            tag.push_unchecked(new);
        }
    }
}

fn apply(tag: &mut Tag, edit: &TagEdit, cover: Option<&Cover>, pictures_in_tag: bool) {
    repair_items(tag);
    if let Some(v) = &edit.title {
        tag.set_title(v.clone());
    }
    if let Some(v) = &edit.artist {
        tag.set_artist(v.clone());
    }
    if let Some(v) = &edit.album {
        tag.set_album(v.clone());
    }
    if let Some(v) = &edit.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, v.clone());
    }
    if let Some(v) = &edit.genre {
        tag.set_genre(v.clone());
    }
    if let Some(y) = edit.year {
        tag.remove_key(ItemKey::Year);
        tag.insert_text(ItemKey::RecordingDate, y.to_string());
    }
    if let Some(n) = edit.track {
        tag.set_track(n);
    }
    if let Some(n) = edit.track_total {
        tag.set_track_total(n);
    }
    if let Some(n) = edit.disc {
        tag.set_disk(n);
    }
    if let Some(n) = edit.disc_total {
        tag.set_disk_total(n);
    }
    if let (Some(c), true) = (cover, pictures_in_tag) {
        tag.remove_picture_type(PictureType::CoverFront);
        tag.push_picture(picture(c));
    }
}

fn picture(c: &Cover) -> Picture {
    Picture::unchecked(c.data.clone())
        .pic_type(PictureType::CoverFront)
        .mime_type(if c.mime == "image/png" {
            MimeType::Png
        } else {
            MimeType::Jpeg
        })
        .build()
}

/// Comments and lyrics carry a language code, and lofty refuses to write one that is not
/// three letters. Korean store downloads (Melon among them) leave it as three NUL bytes;
/// such codes become `XXX`, ID3's "unknown language".
fn fix_languages(tag: &mut Id3v2Tag) {
    let fix = |lang: &mut [u8; 3]| {
        if !lang.iter().all(u8::is_ascii_alphabetic) {
            *lang = *b"XXX";
        }
    };
    tag.retain_mut(|frame| {
        match frame {
            Frame::Comment(c) => fix(&mut c.language),
            Frame::UnsynchronizedText(u) => fix(&mut u.language),
            _ => {}
        }
        true
    });
}

fn id3_of(path: &Path) -> Option<(u8, Frames)> {
    let head = tagbytes::head_of(&mut fs::File::open(path).ok()?, FileType::Mpeg).ok()?;
    sensme::id3_frames(&head)
}

/// What the frame check counts a frame as: analysis apart from other `GEOB`s.
fn frame_key((id, body): &(Vec<u8>, Vec<u8>)) -> &[u8] {
    if id == b"GEOB" && sensme::geob_is_smfmf(body) {
        b"SMFMF"
    } else {
        id
    }
}

/// Every frame the file had must still be there after a write, apart from the ones the
/// change is about, so nothing lofty could not read is lost quietly.
fn kept_frames(before: Option<&(u8, Frames)>, after: Option<(u8, Frames)>, changes: &[&[u8]]) -> Result<()> {
    let Some((version, before)) = before else {
        return Ok(());
    };
    // lofty writes an ID3v2.2 tag as a later version, under other frame ids.
    if *version == 2 {
        return Ok(());
    }
    let after = after.map(|(_, f)| f).unwrap_or_default();
    let mut lost: Vec<String> = Vec::new();
    for frame in before {
        let key = frame_key(frame);
        if changes.contains(&key) {
            continue;
        }
        let had = before.iter().filter(|f| frame_key(f) == key).count();
        let has = after.iter().filter(|f| frame_key(f) == key).count();
        let name = String::from_utf8_lossy(key).into_owned();
        if has < had && !lost.contains(&name) {
            lost.push(name);
        }
    }
    if lost.is_empty() {
        Ok(())
    } else {
        Err(Error::unsupported(
            Why::WouldLoseFrames,
            format!(
                "writing would have lost the {} frame(s); the file was left as it was",
                lost.join(", ")
            ),
        )
        .with_detail(lost.join(", ")))
    }
}

/// The ID3 frames an edit may replace or remove.
fn edited_frames(edit: &TagEdit, cover: Option<&Cover>) -> Vec<&'static [u8]> {
    let mut out: Vec<&'static [u8]> = Vec::new();
    let fields: [(bool, &[&'static [u8]]); 8] = [
        (edit.title.is_some(), &[b"TIT2"]),
        (edit.artist.is_some(), &[b"TPE1"]),
        (edit.album.is_some(), &[b"TALB"]),
        (edit.album_artist.is_some(), &[b"TPE2"]),
        (edit.genre.is_some(), &[b"TCON"]),
        (edit.year.is_some(), &[b"TYER", b"TDAT", b"TIME", b"TRDA", b"TDRC"]),
        (edit.track.is_some() || edit.track_total.is_some(), &[b"TRCK"]),
        (edit.disc.is_some() || edit.disc_total.is_some(), &[b"TPOS"]),
    ];
    for (on, ids) in fields {
        if on {
            out.extend_from_slice(ids);
        }
    }
    if cover.is_some() {
        out.push(b"APIC");
    }
    out
}

/// Splits a format's own tag into the generic fields and everything else, edits the
/// generic part and joins them again, so frames outside the generic set survive.
fn edited<T>(current: Option<&mut T>, edit: &TagEdit, cover: Option<&Cover>, pictures_in_tag: bool) -> T
where
    T: SplitTag + Default,
    T::Remainder: MergeTag<Merged = T>,
{
    let tag = current.map(std::mem::take).unwrap_or_default();
    let (rest, mut generic) = tag.split_tag();
    apply(&mut generic, edit, cover, pictures_in_tag);
    rest.merge_tag(generic)
}

/// lofty's errors print only their outermost layer; the cause is in the sources.
fn encoding(e: impl std::error::Error) -> Error {
    let mut message = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        message.push_str(": ");
        message.push_str(&s.to_string());
        source = s.source();
    }
    Error::unsupported(Why::TagWriteFailed, format!("could not write the tags: {message}"))
        .with_detail(message.to_string())
}

fn write_into(file: &mut fs::File, kind: FileType, edit: &TagEdit, cover: Option<&Cover>) -> Result<()> {
    let parse = ParseOptions::new().read_cover_art(true);
    let mut options = WriteOptions::new().preferred_padding(1024);
    file.seek(SeekFrom::Start(0))?;
    match kind {
        FileType::Mpeg => {
            let original = tagbytes::head_of(file, FileType::Mpeg)?;
            file.seek(SeekFrom::Start(0))?;
            let mut f = MpegFile::read_from(file, parse).map_err(encoding)?;
            // A new ID3v2 tag hides the ID3v1 one from players that read v2 first, so it
            // starts from what ID3v1 says.
            if f.id3v2().is_none()
                && let Some(v1) = f.id3v1()
            {
                f.set_id3v2(Tag::from(v1.clone()).into());
            }
            // Players that read ID3v2.3 may not read 2.4; a file keeps the version it had.
            let v4 = f.id3v2().is_some_and(|t| t.original_version() == Id3v2Version::V4);
            options = options.use_id3v23(!v4);
            if let Some(t) = f.id3v2_mut() {
                fix_languages(t);
            }
            let tag = edited(f.id3v2_mut(), edit, cover, true);
            f.set_id3v2(tag);
            file.seek(SeekFrom::Start(0))?;
            f.save_to(file, options).map_err(encoding)?;
            tagbytes::restore_frames(file, &original, &edited_frames(edit, cover))?;
        }
        FileType::Flac => {
            let mut f = FlacFile::read_from(file, parse).map_err(encoding)?;
            let tag = edited(f.vorbis_comments_mut(), edit, None, false);
            f.set_vorbis_comments(tag);
            if let Some(c) = cover {
                f.remove_picture_type(PictureType::CoverFront);
                f.insert_picture(picture(c), None).map_err(encoding)?;
            }
            file.seek(SeekFrom::Start(0))?;
            f.save_to(file, options).map_err(encoding)?;
        }
        FileType::Mp4 => {
            let mut f = Mp4File::read_from(file, parse).map_err(encoding)?;
            let tag = edited(f.ilst_mut(), edit, cover, true);
            f.set_ilst(tag);
            file.seek(SeekFrom::Start(0))?;
            f.save_to(file, options).map_err(encoding)?;
        }
        FileType::Vorbis => {
            let mut f = VorbisFile::read_from(file, parse).map_err(encoding)?;
            let tag = edited(Some(f.vorbis_comments_mut()), edit, cover, true);
            f.set_vorbis_comments(tag);
            file.seek(SeekFrom::Start(0))?;
            f.save_to(file, options).map_err(encoding)?;
        }
        FileType::Opus => {
            let mut f = OpusFile::read_from(file, parse).map_err(encoding)?;
            let tag = edited(Some(f.vorbis_comments_mut()), edit, cover, true);
            f.set_vorbis_comments(tag);
            file.seek(SeekFrom::Start(0))?;
            f.save_to(file, options).map_err(encoding)?;
        }
        other => {
            return Err(Error::unsupported(
                Why::TagFormat,
                format!("writing tags into {other:?} files is not supported"),
            )
            .with_detail(format!("{other:?}")));
        }
    }
    Ok(())
}

/// Whether `write_smfmf` can put analysis into this file.
pub fn smfmf_writable(path: &Path) -> bool {
    matches!(FileType::from_path(path), Some(FileType::Mpeg | FileType::Flac))
}

/// Whether `write` can change this file's tags.
pub fn writable(path: &Path) -> bool {
    matches!(
        FileType::from_path(path),
        Some(FileType::Mpeg | FileType::Flac | FileType::Mp4 | FileType::Vorbis | FileType::Opus)
    )
}

fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".uwm-part");
    path.with_file_name(name)
}

/// Changes a copy beside the file, reads the copy back to check that the audio is
/// unchanged and that `check` holds, and only then puts it in the file's place.
fn rewrite(
    path: &Path,
    change: impl FnOnce(&mut fs::File, FileType) -> Result<()>,
    check: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let before = lofty::read_from_path(path).map_err(encoding)?;
    let kind = before.file_type();
    let part = part_path(path);
    let result = (|| -> Result<()> {
        fs::copy(path, &part)?;
        let mut file = fs::OpenOptions::new().read(true).write(true).open(&part)?;
        change(&mut file, kind)?;
        file.sync_all()?;
        drop(file);
        // The copy's name hides its type, so it is read as the original's.
        let after = Probe::with_file_type(BufReader::new(fs::File::open(&part)?), kind)
            .read()
            .map_err(encoding)?;
        let (a, b) = (before.properties(), after.properties());
        if after.file_type() != kind
            || a.duration() != b.duration()
            || a.sample_rate() != b.sample_rate()
            || a.channels() != b.channels()
        {
            return Err(Error::unsupported(
                Why::ReadBackDiffers,
                "the file read back differently after writing; it was left as it was",
            ));
        }
        check(&part)?;
        fs::rename(&part, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

/// Rebuilds what comes before the audio of an MP3 or FLAC in a copy beside the file,
/// copies the audio over as it is, and puts the copy in the file's place once `check`
/// holds. False, with nothing written, when the MP3's tag is of a kind that is not
/// edited as bytes.
fn rebuild(path: &Path, kind: FileType, change: Change<'_>, check: impl FnOnce(&Path) -> Result<()>) -> Result<bool> {
    let mut original = fs::File::open(path)?;
    let head = tagbytes::head_of(&mut original, kind)?;
    let size = original.metadata()?.len();
    let new_head = if kind == FileType::Mpeg {
        let v1 = if head.is_empty() {
            tagbytes::v1_of(&mut original, size)
        } else {
            None
        };
        match tagbytes::mp3_head(&head, v1.as_ref().map(|v| &v[..]), change)? {
            Some(h) => h,
            None => return Ok(false),
        }
    } else {
        tagbytes::flac_head(&head, change)?
    };
    let audio = size - head.len() as u64;
    let part = part_path(path);
    let result = (|| -> Result<()> {
        let mut out = fs::File::create(&part)?;
        out.write_all(&new_head)?;
        original.seek(SeekFrom::Start(head.len() as u64))?;
        let copied = std::io::copy(&mut original, &mut out)?;
        out.sync_all()?;
        drop(out);
        fs::set_permissions(&part, fs::metadata(path)?.permissions())?;
        if copied != audio || fs::metadata(&part)?.len() != new_head.len() as u64 + audio {
            return Err(Error::unsupported(
                Why::AudioIncomplete,
                "the audio did not copy over whole; the file was left as it was",
            ));
        }
        check(&part)?;
        fs::rename(&part, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result.map(|()| true)
}

/// Writes the edit into the file's tags.
pub fn write(path: &Path, edit: &TagEdit, cover: Option<&Cover>) -> Result<()> {
    let before = id3_of(path);
    let changes = edited_frames(edit, cover);
    let kept = |part: &Path| kept_frames(before.as_ref(), id3_of(part), &changes);
    let kind = FileType::from_path(path);
    if matches!(kind, Some(FileType::Mpeg | FileType::Flac))
        && rebuild(path, kind.unwrap_or(FileType::Mpeg), Change::Edit(edit, cover), kept)?
    {
        return Ok(());
    }
    rewrite(path, |file, kind| write_into(file, kind, edit, cover), kept)
}

/// Puts an SMFMF block into an MP3 or FLAC where the player reads it, in place of any
/// it had. The block must be one the player accepts, and the written copy must give it
/// back and keep every other frame before it takes the file's place.
pub fn write_smfmf(path: &Path, blob: &[u8]) -> Result<()> {
    if sensme::decode(blob).is_none() {
        return Err(Error::unsupported(
            Why::AnalysisRejected,
            "that is not analysis the player would accept",
        ));
    }
    let kind = match FileType::from_path(path) {
        Some(k @ (FileType::Mpeg | FileType::Flac)) => k,
        _ => {
            return Err(Error::unsupported(
                Why::AnalysisFormat,
                "SensMe analysis can be written into MP3 and FLAC files only",
            ));
        }
    };
    let before = id3_of(path);
    let written = rebuild(path, kind, Change::Smfmf(blob), |part| match sensme::extract(part) {
        Some(b) if b == blob => kept_frames(before.as_ref(), id3_of(part), &[b"SMFMF"]),
        _ => Err(Error::unsupported(
            Why::AnalysisReadBack,
            "the analysis did not read back from the written file; it was left as it was",
        )),
    })?;
    if written {
        Ok(())
    } else {
        Err(Error::unsupported(
            Why::Id3Kind,
            "this MP3's ID3 tag is of a kind the analysis is not added to",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One silent MPEG-1 Layer III frame: 128 kbit/s, 44.1 kHz, no padding.
    fn mp3_frames(n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for _ in 0..n {
            out.extend_from_slice(&[0xff, 0xfb, 0x90, 0x64]);
            out.extend(std::iter::repeat_n(0u8, 417 - 4));
        }
        out
    }

    fn v23_frame(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut f = id.to_vec();
        f.extend_from_slice(&(body.len() as u32).to_be_bytes());
        f.extend_from_slice(&[0, 0]);
        f.extend_from_slice(body);
        f
    }

    fn syncsafe(n: usize) -> [u8; 4] {
        [
            (n >> 21) as u8 & 0x7f,
            (n >> 14) as u8 & 0x7f,
            (n >> 7) as u8 & 0x7f,
            n as u8 & 0x7f,
        ]
    }

    /// An MP3 laid out like the ones on the test player: ID3v2.3 with the SensMe data in a
    /// GEOB frame, a title in CP949 bytes marked Latin-1, and a Melon comment whose
    /// language code is three NUL bytes.
    fn analysed_mp3(smfmf: &[u8]) -> Vec<u8> {
        let mut geob = vec![0u8];
        geob.extend_from_slice(b"application/SMFMF\0\0USR_SMFMF\0");
        geob.extend_from_slice(smfmf);
        let (cp949, _, _) = encoding_rs::EUC_KR.encode("이노래");
        let mut tit2 = vec![0u8];
        tit2.extend_from_slice(&cp949);
        let mut frames = v23_frame(b"GEOB", &geob);
        frames.extend(v23_frame(b"TIT2", &tit2));
        frames.extend(v23_frame(b"TPE1", b"\x002AM"));
        frames.extend(v23_frame(b"COMM", b"\x00\x00\x00\x00\x00http://www.melon.com/"));
        let mut out = b"ID3\x03\x00\x00".to_vec();
        out.extend_from_slice(&syncsafe(frames.len()));
        out.extend(frames);
        out.extend(mp3_frames(40));
        out
    }

    fn find(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn keeps_sensme_data_and_version() {
        let dir = std::env::temp_dir().join(format!("uwm-tags-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.mp3");
        let mut smfmf = b"GBPMSTAEMMLW\x01\x00\x80\x00\x00\x00\x00\x04".to_vec();
        smfmf.extend_from_slice(&120.5f32.to_be_bytes());
        fs::write(&path, analysed_mp3(&smfmf)).unwrap();

        let edit = TagEdit {
            album: Some("이노래".into()),
            year: Some(2010),
            track: Some(1),
            ..TagEdit::default()
        };
        let cover = Cover {
            data: b"\xff\xd8\xff\xe0not really a jpeg".to_vec(),
            mime: "image/jpeg",
        };
        write(&path, &edit, Some(&cover)).unwrap();

        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"ID3\x03", "stays ID3v2.3");
        assert!(find(&bytes, b"application/SMFMF\0\0USR_SMFMF\0"));
        assert!(find(&bytes, &smfmf));
        assert!(!dir.join("a.mp3.uwm-part").exists());

        let mut file = fs::File::open(&path).unwrap();
        let f = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
        let tag = f.id3v2().unwrap();
        assert_eq!(tag.title().as_deref(), Some("이노래"), "legacy title decoded on write");
        assert_eq!(tag.artist().as_deref(), Some("2AM"));
        assert_eq!(tag.album().as_deref(), Some("이노래"));
        assert_eq!(tag.track(), Some(1));
        let generic: Tag = tag.clone().into();
        assert_eq!(generic.date().map(|d| d.year), Some(2010));
        assert_eq!(generic.pictures().len(), 1);
        let comment = tag.comments().next().unwrap();
        assert_eq!(
            (&comment.language, &*comment.content),
            (b"\0\0\0", "http://www.melon.com/"),
            "a frame the edit is not about stays as it was stored"
        );
        assert_eq!(f.properties().duration(), {
            let mut orig = std::io::Cursor::new(analysed_mp3(&smfmf));
            MpegFile::read_from(&mut orig, ParseOptions::new())
                .unwrap()
                .properties()
                .duration()
        });

        write(&path, &TagEdit::default(), None).unwrap();
        assert!(find(&fs::read(&path).unwrap(), &smfmf), "a second write keeps it too");
        let _ = fs::remove_dir_all(dir);
    }

    fn count(hay: &[u8], needle: &[u8]) -> usize {
        hay.windows(needle.len()).filter(|w| *w == needle).count()
    }

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uwm-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_blob(bpm: f32) -> Vec<u8> {
        sensme::tests::blob(&[50; 36], &[50; 40], &[50; 10], bpm, 61_000)
    }

    #[test]
    fn puts_analysis_into_an_mp3() {
        let dir = test_dir("smfmf-mp3");
        let path = dir.join("a.mp3");
        let mut tag = b"ID3\x03\x00\x00".to_vec();
        let frames = v23_frame(b"TIT2", b"\x00Title");
        tag.extend_from_slice(&syncsafe(frames.len()));
        tag.extend(frames);
        tag.extend(mp3_frames(40));
        fs::write(&path, &tag).unwrap();

        let first = sample_blob(98.5);
        write_smfmf(&path, &first).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"ID3\x03", "Sony's files are ID3v2.3");
        assert!(find(&bytes, b"GEOB"));
        assert!(find(&bytes, b"\0application/SMFMF\0\0USR_SMFMF\0"));
        assert_eq!(sensme::extract(&path), Some(first.clone()));

        let second = sample_blob(120.0);
        write_smfmf(&path, &second).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(count(&bytes, b"application/SMFMF"), 1, "the old block is replaced");
        assert_eq!(sensme::extract(&path), Some(second));
        let mut file = fs::File::open(&path).unwrap();
        let f = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
        assert_eq!(f.id3v2().unwrap().title().as_deref(), Some("Title"));

        assert!(write_smfmf(&path, b"not analysis").is_err());
        let _ = fs::remove_dir_all(dir);
    }

    /// A text frame's text, in Latin-1 or in UTF-16 with a byte order mark.
    fn text_of(path: &Path, id: &[u8]) -> Option<String> {
        let (_, frames) = id3_of(path)?;
        let (_, body) = frames.into_iter().find(|(i, _)| i == id)?;
        Some(match body[0] {
            1 => {
                let units = body[3..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c));
                char::decode_utf16(units)
                    .map_while(std::result::Result::ok)
                    .take_while(|c| *c != '\0')
                    .collect()
            }
            _ => body[1..]
                .iter()
                .take_while(|b| **b != 0)
                .map(|b| char::from(*b))
                .collect(),
        })
    }

    #[test]
    fn keeps_frames_lofty_drops() {
        let dir = test_dir("tyer");
        let path = dir.join("a.mp3");
        let mut frames = v23_frame(b"TIT2", b"\x00Title");
        frames.extend(v23_frame(b"TYER", b"\x0020080711"));
        frames.extend(v23_frame(b"TSOA", b"\x00Sort Album"));
        let mut file = b"ID3\x03\x00\x00".to_vec();
        file.extend_from_slice(&syncsafe(frames.len()));
        file.extend(frames);
        file.extend(mp3_frames(40));
        fs::write(&path, &file).unwrap();

        write_smfmf(&path, &sample_blob(98.5)).unwrap();
        assert_eq!(text_of(&path, b"TYER").as_deref(), Some("20080711"));
        let album = TagEdit {
            album: Some("Album".into()),
            ..TagEdit::default()
        };
        write(&path, &album, None).unwrap();
        assert_eq!(text_of(&path, b"TYER").as_deref(), Some("20080711"));
        assert_eq!(
            text_of(&path, b"TSOA").as_deref(),
            Some("Sort Album"),
            "an iTunes v2.4 frame in v2.3"
        );
        assert_eq!(text_of(&path, b"TALB").as_deref(), Some("Album"));
        let year = TagEdit {
            year: Some(2009),
            ..TagEdit::default()
        };
        write(&path, &year, None).unwrap();
        assert_eq!(text_of(&path, b"TYER").as_deref(), Some("2009"));
        assert!(sensme::extract(&path).is_some(), "the analysis stays through tag edits");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn gives_an_untagged_mp3_a_tag_and_leaves_odd_tags_alone() {
        let dir = test_dir("smfmf-bare");
        let path = dir.join("a.mp3");
        let (cp949, _, _) = encoding_rs::EUC_KR.encode("첫사랑");
        let mut v1 = b"TAG".to_vec();
        v1.extend_from_slice(&cp949);
        v1.resize(33, 0);
        v1.extend_from_slice(b"Cool");
        v1.resize(125, b' ');
        v1.extend_from_slice(&[0, 4, 255]);
        let mut audio = mp3_frames(40);
        audio.extend_from_slice(&v1);
        fs::write(&path, &audio).unwrap();
        write_smfmf(&path, &sample_blob(98.5)).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"ID3\x03");
        assert!(bytes.ends_with(&audio), "audio and the ID3v1 tag stay");
        assert!(sensme::extract(&path).is_some());
        let (_, frames) = id3_of(&path).unwrap();
        let body = |id: &[u8]| frames.iter().find(|(i, _)| i == id).map(|(_, b)| b.clone());
        assert_eq!(
            body(b"TIT2"),
            Some([&[0u8][..], &cp949].concat()),
            "the title's bytes as they were"
        );
        assert_eq!(body(b"TPE1"), Some(b"\x00Cool".to_vec()));
        assert_eq!(body(b"TRCK"), Some(b"\x004".to_vec()));
        assert_eq!(body(b"TCON"), None, "genre 255 is none");

        let odd = dir.join("b.mp3");
        let mut file = b"ID3\x03\x00\x80".to_vec();
        let frames = v23_frame(b"TIT2", b"\x00Title");
        file.extend_from_slice(&syncsafe(frames.len()));
        file.extend(frames);
        file.extend(mp3_frames(40));
        fs::write(&odd, &file).unwrap();
        assert!(write_smfmf(&odd, &sample_blob(98.5)).is_err(), "an unsynchronised tag");
        assert_eq!(fs::read(&odd).unwrap(), file);
        assert!(!dir.join("b.mp3.uwm-part").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_a_write_that_loses_frames() {
        let frame = |id: &[u8], body: &[u8]| (id.to_vec(), body.to_vec());
        let before = (3, vec![frame(b"TIT2", b"\x00a"), frame(b"TXXX", b"\x00k\x00v")]);
        let lost = (3, vec![frame(b"TIT2", b"\x00b")]);
        assert!(kept_frames(Some(&before), Some(lost.clone()), &[b"TIT2"]).is_err());
        assert!(kept_frames(Some(&before), Some(before.clone()), &[]).is_ok());
        let edited = (3, vec![frame(b"TXXX", b"\x00k\x00v")]);
        assert!(kept_frames(Some(&before), Some(edited), &[b"TIT2"]).is_ok());
        assert!(
            kept_frames(None, Some(lost), &[]).is_ok(),
            "no tag before, nothing to keep"
        );
    }

    fn stream_info() -> Vec<u8> {
        let mut b = vec![0x10, 0x00, 0x10, 0x00, 0, 0, 0, 0, 0, 0];
        let packed: u64 = (44_100 << 44) | (1 << 41) | (15 << 36) | 88_200;
        b.extend_from_slice(&packed.to_be_bytes());
        b.extend_from_slice(&[0; 16]);
        b
    }

    fn flac_file(blocks: &[(u8, Vec<u8>)], audio: &[u8]) -> Vec<u8> {
        let mut out = b"fLaC".to_vec();
        for (i, (kind, body)) in blocks.iter().enumerate() {
            let last = if i + 1 == blocks.len() { 0x80 } else { 0 };
            out.push(kind | last);
            out.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
            out.extend_from_slice(body);
        }
        out.extend_from_slice(audio);
        out
    }

    #[test]
    fn puts_analysis_into_a_flac() {
        let dir = test_dir("smfmf-flac");
        let audio: Vec<u8> = (0..8192u32).map(|i| (i * 7) as u8).collect();
        let comments = b"\x03\x00\x00\x00uwm\x00\x00\x00\x00".to_vec();

        let padded = dir.join("padded.flac");
        let original = flac_file(&[(0, stream_info()), (4, comments.clone()), (1, vec![0; 4096])], &audio);
        fs::write(&padded, &original).unwrap();
        write_smfmf(&padded, &sample_blob(98.5)).unwrap();
        let second = sample_blob(120.0);
        write_smfmf(&padded, &second).unwrap();
        let bytes = fs::read(&padded).unwrap();
        assert_eq!(bytes.len(), original.len(), "padding made room for the block");
        assert!(bytes.ends_with(&audio), "audio untouched");
        assert_eq!(count(&bytes, b"SMFM"), 1, "the old block is replaced");
        assert_eq!(sensme::extract(&padded), Some(second.clone()));

        let bare = dir.join("bare.flac");
        let original = flac_file(&[(0, stream_info()), (4, comments)], &audio);
        fs::write(&bare, &original).unwrap();
        write_smfmf(&bare, &second).unwrap();
        let bytes = fs::read(&bare).unwrap();
        assert_eq!(bytes.len(), original.len() + 8 + second.len());
        assert!(bytes.ends_with(&audio));
        assert_eq!(sensme::extract(&bare), Some(second));
        let _ = fs::remove_dir_all(dir);
    }

    fn vorbis(entries: &[&str]) -> Vec<u8> {
        let mut b = 3u32.to_le_bytes().to_vec();
        b.extend_from_slice(b"uwm");
        b.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for e in entries {
            b.extend_from_slice(&(e.len() as u32).to_le_bytes());
            b.extend_from_slice(e.as_bytes());
        }
        b
    }

    fn flac_picture(data: &[u8]) -> Vec<u8> {
        let mut b = 3u32.to_be_bytes().to_vec();
        b.extend_from_slice(&10u32.to_be_bytes());
        b.extend_from_slice(b"image/jpeg");
        b.extend_from_slice(&[0; 20]);
        b.extend_from_slice(&(data.len() as u32).to_be_bytes());
        b.extend_from_slice(data);
        b
    }

    #[test]
    fn edits_a_flac_without_losing_fields() {
        let dir = test_dir("flac-edit");
        let path = dir.join("a.flac");
        let audio: Vec<u8> = (0..8192u32).map(|i| (i * 5) as u8).collect();
        let comments = vorbis(&[
            "TITLE=Old",
            "ALBUM=Old Album",
            "ALBUM ARTIST=Someone",
            "WRITER=Writer",
            "Date=2001",
            "TRACKNUMBER=3",
            "TOTALTRACKS=9",
            "encoder=x",
        ]);
        let blocks = [
            (0, stream_info()),
            (4, comments),
            (6, flac_picture(b"old cover")),
            (1, vec![0; 4096]),
        ];
        let original = flac_file(&blocks, &audio);
        fs::write(&path, &original).unwrap();
        let edit = TagEdit {
            album: Some("새 앨범".into()),
            track_total: Some(12),
            ..TagEdit::default()
        };
        let cover = Cover {
            data: b"new cover".to_vec(),
            mime: "image/jpeg",
        };
        write(&path, &edit, Some(&cover)).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.len(), original.len(), "padding made room");
        assert!(bytes.ends_with(&audio));
        for kept in [
            "TITLE=Old",
            "ALBUM ARTIST=Someone",
            "WRITER=Writer",
            "Date=2001",
            "TRACKNUMBER=3",
            "encoder=x",
        ] {
            assert!(find(&bytes, kept.as_bytes()), "{kept} kept");
        }
        assert!(find(&bytes, "ALBUM=새 앨범".as_bytes()));
        assert!(!find(&bytes, b"Old Album"));
        assert!(find(&bytes, b"TRACKTOTAL=12"));
        assert!(!find(&bytes, b"TOTALTRACKS"), "the other spelling of the total goes");
        assert!(find(&bytes, b"new cover") && !find(&bytes, b"old cover"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn edits_an_mp3_without_touching_other_frames() {
        let dir = test_dir("mp3-edit");
        let path = dir.join("a.mp3");
        let others = [
            v23_frame(b"TIT2", b"\x00Title"),
            v23_frame(b"TYER", b"\x0020080711"),
            v23_frame(b"TSOA", b"\x00Sort"),
            v23_frame(b"PRIV", b"MELON_UID\x00abc"),
            v23_frame(b"COMM", b"\x00\x00\x00\x00\x00http://www.melon.com/"),
        ];
        let mut frames: Vec<u8> = others.concat();
        frames.extend(v23_frame(b"TALB", b"\x00Old"));
        frames.extend(v23_frame(b"APIC", b"\x00image/jpeg\x00\x03\x00old cover"));
        let mut file = b"ID3\x03\x00\x00".to_vec();
        file.extend_from_slice(&syncsafe(frames.len() + 2048));
        file.extend(frames);
        file.resize(file.len() + 2048, 0);
        let audio = mp3_frames(40);
        file.extend_from_slice(&audio);
        fs::write(&path, &file).unwrap();

        let edit = TagEdit {
            album: Some("새 앨범".into()),
            ..TagEdit::default()
        };
        let cover = Cover {
            data: b"new cover".to_vec(),
            mime: "image/jpeg",
        };
        write(&path, &edit, Some(&cover)).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.len(), file.len(), "padding made room");
        assert!(bytes.ends_with(&audio));
        for f in &others {
            assert!(
                find(&bytes, f),
                "{} kept byte for byte",
                String::from_utf8_lossy(&f[..4])
            );
        }
        assert_eq!(text_of(&path, b"TALB").as_deref(), Some("새 앨범"));
        assert_eq!(count(&bytes, b"APIC"), 1);
        assert!(find(&bytes, b"new cover") && !find(&bytes, b"old cover"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_formats_it_cannot_write() {
        assert!(writable(Path::new("a.MP3")));
        assert!(writable(Path::new("a.flac")));
        assert!(writable(Path::new("a.m4a")));
        assert!(!writable(Path::new("a.dsf")));
        assert!(!writable(Path::new("a.wma")));
        assert!(smfmf_writable(Path::new("a.MP3")));
        assert!(smfmf_writable(Path::new("a.flac")));
        assert!(!smfmf_writable(Path::new("a.m4a")));
    }
}
