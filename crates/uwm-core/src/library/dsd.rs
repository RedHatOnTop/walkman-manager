//! DSD Stream File (.dsf, Sony) and DSDIFF (.dff, Philips) headers. lofty reads
//! neither, so the stream format is parsed here and the ID3v2 block both may
//! carry is handed to lofty on its own.

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::{FileType, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Tag;

#[derive(Debug, Clone, PartialEq)]
pub struct DsdInfo {
    /// One-bit sample rate, 2_822_400 for DSD64.
    pub rate: u32,
    pub channels: u16,
    pub duration_ms: u64,
}

pub struct DsdFile {
    pub info: DsdInfo,
    pub tag: Option<Tag>,
}

/// ID3 blocks larger than this are skipped rather than read into memory.
const MAX_ID3: u64 = 32 * 1024 * 1024;

pub fn read(path: &Path) -> Option<DsdFile> {
    let mut f = File::open(path).ok()?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).ok()?;
    f.seek(SeekFrom::Start(0)).ok()?;
    match &magic {
        b"DSD " => read_dsf(&mut f),
        b"FRM8" => read_dff(&mut f),
        _ => None,
    }
}

fn u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap())
}
fn u64_le(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[..8].try_into().unwrap())
}
fn u64_be(b: &[u8]) -> u64 {
    u64::from_be_bytes(b[..8].try_into().unwrap())
}

fn read_dsf<R: Read + Seek>(r: &mut R) -> Option<DsdFile> {
    let mut head = [0u8; 28 + 52];
    r.read_exact(&mut head).ok()?;
    if &head[28..32] != b"fmt " {
        return None;
    }
    let metadata_at = u64_le(&head[20..28]);
    let fmt = &head[40..];
    let channels = u32_le(&fmt[12..16]) as u16;
    let rate = u32_le(&fmt[16..20]);
    let samples = u64_le(&fmt[24..32]);
    if rate == 0 {
        return None;
    }
    let tag = if metadata_at > 0 {
        id3_at(r, metadata_at, None)
    } else {
        None
    };
    Some(DsdFile {
        info: DsdInfo {
            rate,
            channels,
            duration_ms: samples.saturating_mul(1000) / u64::from(rate),
        },
        tag,
    })
}

fn read_dff<R: Read + Seek>(r: &mut R) -> Option<DsdFile> {
    let mut form = [0u8; 16];
    r.read_exact(&mut form).ok()?;
    if &form[12..16] != b"DSD " {
        return None;
    }
    let end = 12 + u64_be(&form[4..12]);
    let (mut rate, mut channels, mut data_bytes, mut tag) = (0u32, 0u16, 0u64, None);
    let mut at = 16u64;
    while at + 12 <= end {
        r.seek(SeekFrom::Start(at)).ok()?;
        let mut ck = [0u8; 12];
        r.read_exact(&mut ck).ok()?;
        let size = u64_be(&ck[4..12]);
        let body = at + 12;
        match &ck[..4] {
            b"PROP" => {
                let mut sub = body + 4;
                while sub + 12 <= body + size {
                    r.seek(SeekFrom::Start(sub)).ok()?;
                    let mut sk = [0u8; 12];
                    r.read_exact(&mut sk).ok()?;
                    let ssize = u64_be(&sk[4..12]);
                    let mut v = [0u8; 4];
                    match &sk[..4] {
                        b"FS  " => {
                            r.read_exact(&mut v).ok()?;
                            rate = u32::from_be_bytes(v);
                        }
                        b"CHNL" => {
                            r.read_exact(&mut v[..2]).ok()?;
                            channels = u16::from_be_bytes([v[0], v[1]]);
                        }
                        _ => {}
                    }
                    sub += 12 + ssize + (ssize & 1);
                }
            }
            b"DSD " => data_bytes = size,
            b"ID3 " => tag = id3_at(r, body, Some(size)),
            _ => {}
        }
        at = body + size + (size & 1);
    }
    if rate == 0 || channels == 0 {
        return None;
    }
    let bits = data_bytes.saturating_mul(8) / u64::from(channels);
    Some(DsdFile {
        info: DsdInfo {
            rate,
            channels,
            duration_ms: bits.saturating_mul(1000) / u64::from(rate),
        },
        tag,
    })
}

fn id3_at<R: Read + Seek>(r: &mut R, at: u64, len: Option<u64>) -> Option<Tag> {
    let total = r.seek(SeekFrom::End(0)).ok()?;
    let len = len.unwrap_or(total.saturating_sub(at)).min(total.saturating_sub(at));
    if !(10..=MAX_ID3).contains(&len) {
        return None;
    }
    r.seek(SeekFrom::Start(at)).ok()?;
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf).ok()?;
    if &buf[..3] != b"ID3" {
        return None;
    }
    // A bare ID3v2 block reads as an MPEG file without audio frames. The MPEG reader seeks
    // back from the end for ID3v1 and APE footers, so a short block gets zero padding.
    buf.resize(buf.len() + 256, 0);
    let tagged = Probe::new(Cursor::new(buf))
        .set_file_type(FileType::Mpeg)
        .options(ParseOptions::new().read_properties(false))
        .read()
        .ok()?;
    tagged.primary_tag().or_else(|| tagged.first_tag()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::config::WriteOptions;
    use lofty::id3::v2::Id3v2Tag;
    use lofty::prelude::*;

    fn id3_title(title: &str) -> Vec<u8> {
        let mut tag = Id3v2Tag::new();
        tag.set_title(title.to_owned());
        tag.set_artist("Test Artist".to_owned());
        let mut out = Vec::new();
        tag.dump_to(&mut out, WriteOptions::default()).unwrap();
        out
    }

    fn dsf(rate: u32, channels: u32, samples: u64, id3: &[u8]) -> Vec<u8> {
        let data_len = 12 + 4096u64;
        let metadata_at = 28 + 52 + data_len;
        let total = metadata_at + id3.len() as u64;
        let mut b = Vec::new();
        b.extend_from_slice(b"DSD ");
        b.extend_from_slice(&28u64.to_le_bytes());
        b.extend_from_slice(&total.to_le_bytes());
        b.extend_from_slice(&(if id3.is_empty() { 0 } else { metadata_at }).to_le_bytes());
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&52u64.to_le_bytes());
        for v in [1u32, 0, 2, channels, rate, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&samples.to_le_bytes());
        b.extend_from_slice(&4096u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        b.resize(b.len() + 4096, 0x69);
        b.extend_from_slice(id3);
        b
    }

    #[test]
    fn dsf_header_and_tag() {
        let bytes = dsf(5_644_800, 2, 5_644_800 * 3, &id3_title("Track One"));
        let f = read_dsf(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(
            f.info,
            DsdInfo {
                rate: 5_644_800,
                channels: 2,
                duration_ms: 3000
            }
        );
        assert_eq!(f.tag.unwrap().title().as_deref(), Some("Track One"));
    }

    #[test]
    fn dsf_without_tag() {
        let f = read_dsf(&mut Cursor::new(dsf(2_822_400, 2, 2_822_400, &[]))).unwrap();
        assert_eq!(f.info.duration_ms, 1000);
        assert!(f.tag.is_none());
    }

    #[test]
    fn dff_chunks() {
        let id3 = id3_title("Dff Song");
        let mut prop = Vec::new();
        prop.extend_from_slice(b"SND ");
        prop.extend_from_slice(b"FS  ");
        prop.extend_from_slice(&4u64.to_be_bytes());
        prop.extend_from_slice(&2_822_400u32.to_be_bytes());
        prop.extend_from_slice(b"CHNL");
        prop.extend_from_slice(&10u64.to_be_bytes());
        prop.extend_from_slice(&2u16.to_be_bytes());
        prop.extend_from_slice(b"SLFTSRGT");
        let data = vec![0x69u8; 2 * 2_822_400 / 8 * 2];
        let mut body = Vec::new();
        body.extend_from_slice(b"DSD ");
        for (id, payload) in [(b"PROP", &prop[..]), (b"DSD ", &data[..]), (b"ID3 ", &id3[..])] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(payload.len() as u64).to_be_bytes());
            body.extend_from_slice(payload);
            if payload.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut file = Vec::new();
        file.extend_from_slice(b"FRM8");
        file.extend_from_slice(&(body.len() as u64).to_be_bytes());
        file.extend_from_slice(&body);
        let f = read_dff(&mut Cursor::new(file)).unwrap();
        assert_eq!(
            f.info,
            DsdInfo {
                rate: 2_822_400,
                channels: 2,
                duration_ms: 2000
            }
        );
        assert_eq!(f.tag.unwrap().title().as_deref(), Some("Dff Song"));
    }

    #[test]
    fn rejects_other_files() {
        assert!(read_dsf(&mut Cursor::new(b"DSD \x1c\0\0\0\0\0\0\0".repeat(10))).is_none());
    }
}
