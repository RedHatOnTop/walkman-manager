//! What a file the owner brings is: its structure from the first bytes, then its content
//! against the catalog. Nothing here decrypts or unpacks; that is the kit's work.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use super::catalog::{self, KnownFile};
use crate::{Error, Result};

const NWWM_HEADER: u64 = 128;
const EXT4_SUPER: usize = 1024;
const ZIP_PEEK: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormatKind {
    SonyNwwm,
    SonyUpg,
    OtaPayload,
    OtaBlocks,
    Ext4,
    Sparse,
    Xz,
    Gzip,
    BootImage,
    VbMeta,
    KernelImage,
    Zip,
    WmBackup,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Format {
    /// Sony's package for its Android players (NW-ZX500, NW-A100): an encrypted OTA zip.
    SonyNwwm {
        payload_bytes: u64,
    },
    /// Sony's package for Walkman OS players (NW-A50 and older), sealed by a leading MD5.
    SonyUpg,
    /// An Android update zip carrying `payload.bin` (A/B devices).
    OtaPayload {
        device: Option<String>,
    },
    /// An Android update zip carrying `system.new.dat` and its transfer list.
    OtaBlocks {
        device: Option<String>,
    },
    Ext4 {
        fs_bytes: u64,
        label: String,
    },
    Sparse,
    Xz,
    Gzip,
    BootImage {
        header_version: u32,
    },
    VbMeta,
    /// An arm64 kernel `Image`, with the release its banner names.
    KernelImage {
        release: Option<String>,
    },
    Zip,
    /// `manifest.txt` of the backup walkman-a55's backup UPG leaves on an NW-A50 series player
    /// (`WMBACKUP`): one line per partition image beside it, with the md5 the player took.
    WmBackup {
        images: u32,
    },
    Unknown,
}

impl Format {
    pub fn kind(&self) -> FormatKind {
        match self {
            Format::SonyNwwm { .. } => FormatKind::SonyNwwm,
            Format::SonyUpg => FormatKind::SonyUpg,
            Format::OtaPayload { .. } => FormatKind::OtaPayload,
            Format::OtaBlocks { .. } => FormatKind::OtaBlocks,
            Format::Ext4 { .. } => FormatKind::Ext4,
            Format::Sparse => FormatKind::Sparse,
            Format::Xz => FormatKind::Xz,
            Format::Gzip => FormatKind::Gzip,
            Format::BootImage { .. } => FormatKind::BootImage,
            Format::VbMeta => FormatKind::VbMeta,
            Format::KernelImage { .. } => FormatKind::KernelImage,
            Format::Zip => FormatKind::Zip,
            Format::WmBackup { .. } => FormatKind::WmBackup,
            Format::Unknown => FormatKind::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Damage {
    /// Shorter than its own header says: a download that stopped.
    Truncated { expected: u64, actual: u64 },
    /// The size and kind of a known file, other content.
    Changed { known: String },
    /// An image a backup's list names is missing beside it, or reads otherwise than the
    /// player's md5: a copy that stopped or went wrong.
    Unlike { file: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identified {
    pub path: PathBuf,
    pub bytes: u64,
    /// Modification time, seconds since the epoch; with `bytes` it tells a later look
    /// whether the file is still the one that was hashed.
    pub modified: i64,
    pub format: Format,
    pub sha256: String,
    pub md5: String,
    /// The catalog entry this file is.
    pub known: Option<String>,
    pub damage: Option<Damage>,
}

/// Size and modification time as `Identified` records them.
pub fn stamp(path: &Path) -> Result<(u64, i64)> {
    let meta = std::fs::metadata(path)?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    Ok((meta.len(), modified))
}

/// Reads the whole file once. `on_progress` gets bytes read so far and the total.
pub fn identify(path: &Path, cancel: &AtomicBool, on_progress: &mut dyn FnMut(u64, u64)) -> Result<Identified> {
    let (bytes, modified) = stamp(path)?;
    let (mut format, mut damage) = sniff(path, bytes)?;
    let upg_candidate = format == Format::Unknown && has_extension(path, "upg");
    let sums = digest(path, bytes, upg_candidate, cancel, on_progress)?;
    if upg_candidate && sums.sealed_md5 {
        format = Format::SonyUpg;
    }
    let known = catalog::recognise(&sums.sha256, &sums.md5);
    if damage.is_none() && matches!(format, Format::WmBackup { .. }) {
        damage = backup_damage(path, cancel, on_progress)?;
    }
    if damage.is_none() && known.is_none() {
        damage = changed_copy_of(format.kind(), bytes).map(|k| Damage::Changed { known: k.id.clone() });
    }
    Ok(Identified {
        path: path.to_path_buf(),
        bytes,
        modified,
        format,
        sha256: sums.sha256,
        md5: sums.md5,
        known: known.map(|k| k.id.clone()),
        damage,
    })
}

fn changed_copy_of(kind: FormatKind, bytes: u64) -> Option<&'static KnownFile> {
    catalog::files()
        .iter()
        .find(|k| k.format == kind && k.bytes == Some(bytes))
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// The structure the first bytes show, and damage the structure alone proves.
pub fn sniff(path: &Path, bytes: u64) -> Result<(Format, Option<Damage>)> {
    let mut f = File::open(path)?;
    let mut head = vec![0u8; 4096];
    let n = read_up_to(&mut f, &mut head)?;
    head.truncate(n);

    if let Some(payload) = nwwm_payload(&head) {
        let expected = NWWM_HEADER + payload;
        let damage = (bytes < expected).then_some(Damage::Truncated {
            expected,
            actual: bytes,
        });
        return Ok((Format::SonyNwwm { payload_bytes: payload }, damage));
    }
    if head.starts_with(b"ANDROID!") && head.len() >= 44 {
        return Ok((
            Format::BootImage {
                header_version: le32(&head, 40),
            },
            None,
        ));
    }
    if head.starts_with(b"AVB0") {
        return Ok((Format::VbMeta, None));
    }
    if head.len() >= 64 && head[56..60] == *b"ARM\x64" {
        return Ok((
            Format::KernelImage {
                release: kernel_release(&mut f)?,
            },
            None,
        ));
    }
    if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0]) {
        return Ok((Format::Xz, None));
    }
    if head.starts_with(&[0x1F, 0x8B]) {
        return Ok((Format::Gzip, None));
    }
    if head.len() >= 4 && le32(&head, 0) == 0xED26_FF3A {
        return Ok((Format::Sparse, None));
    }
    if let Some((fs_bytes, label)) = ext4(&head) {
        let damage = (bytes < fs_bytes).then_some(Damage::Truncated {
            expected: fs_bytes,
            actual: bytes,
        });
        return Ok((Format::Ext4 { fs_bytes, label }, damage));
    }
    if head.starts_with(b"PK\x03\x04") {
        return Ok((zip_format(&mut f, bytes).unwrap_or(Format::Zip), None));
    }
    if let Some(images) = backup_images(&head).filter(|_| (bytes as usize) == head.len()) {
        return Ok((
            Format::WmBackup {
                images: images.len() as u32,
            },
            None,
        ));
    }
    Ok((Format::Unknown, None))
}

const BACKUP_HEADER: &str = "# name size_bytes md5 file";
/// The partitions a kit reads out of a backup; a list without them is some other list.
const BACKUP_NEEDS: [&str; 3] = ["android.img", "option2.img", "recovery.img"];

/// The (file, md5) lines of a backup's `manifest.txt`: `<name> <size> <md5> <file>`, the size
/// column as the backup UPG wrote it, which is not a size.
fn backup_images(text: &[u8]) -> Option<Vec<(String, String)>> {
    let text = std::str::from_utf8(text).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim_end() != BACKUP_HEADER {
        return None;
    }
    let mut images = Vec::new();
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let &[_, _, md5, file] = fields.as_slice() else {
            return None;
        };
        let plain = file.ends_with(".img") && !file.contains(['/', '\\']) && file != "..";
        if !plain || md5.len() != 32 || !md5.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        images.push((file.to_owned(), md5.to_ascii_lowercase()));
    }
    BACKUP_NEEDS
        .iter()
        .all(|n| images.iter().any(|(f, _)| f == n))
        .then_some(images)
}

/// Every image the list names, read once against its md5.
fn backup_damage(list: &Path, cancel: &AtomicBool, on_progress: &mut dyn FnMut(u64, u64)) -> Result<Option<Damage>> {
    let images = backup_images(&std::fs::read(list)?).unwrap_or_default();
    let dir = list.parent().unwrap_or(Path::new("."));
    let mut total = 0;
    for (file, _) in &images {
        match std::fs::metadata(dir.join(file)) {
            Ok(m) => total += m.len(),
            Err(_) => return Ok(Some(Damage::Unlike { file: file.clone() })),
        }
    }
    let mut done = 0;
    for (file, want) in &images {
        let base = done;
        let sums = digest(&dir.join(file), total, false, cancel, &mut |n, _| {
            done = base + n;
            on_progress(done, total);
        })?;
        if sums.md5 != *want {
            return Ok(Some(Damage::Unlike { file: file.clone() }));
        }
    }
    Ok(None)
}

/// The release in the kernel's banner, "Linux version <release> (builder) ...", which is the
/// `uname -r` the running kernel will report.
fn kernel_release(f: &mut File) -> Result<Option<String>> {
    const MARK: &[u8] = b"Linux version ";
    f.seek(SeekFrom::Start(0))?;
    let mut window = Vec::with_capacity(1 << 20);
    let mut chunk = vec![0u8; 1 << 20];
    loop {
        let n = read_up_to(f, &mut chunk)?;
        window.extend_from_slice(&chunk[..n]);
        if let Some(at) = window.windows(MARK.len()).position(|w| w == MARK) {
            let rest = &window[at + MARK.len()..];
            if let Some(end) = rest.iter().position(|&b| b == b' ' || b == 0 || b == b'\n') {
                return Ok(std::str::from_utf8(&rest[..end]).ok().map(str::to_owned));
            }
            if n == 0 {
                return Ok(None);
            }
            continue;
        }
        if n == 0 {
            return Ok(None);
        }
        let keep = window.len().saturating_sub(MARK.len());
        window.drain(..keep);
    }
}

fn read_up_to(f: &mut File, buf: &mut [u8]) -> Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn le64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// The payload size an NWWM header declares: `NWWM`, 56 hex digits of SHA-224, `0` fill to
/// byte 96, 12 decimal digits, zero fill to byte 128.
fn nwwm_payload(h: &[u8]) -> Option<u64> {
    if h.len() < NWWM_HEADER as usize || !h.starts_with(b"NWWM") {
        return None;
    }
    let shaped = h[4..60].iter().all(u8::is_ascii_hexdigit)
        && h[60..96].iter().all(|&b| b == b'0')
        && h[96..108].iter().all(u8::is_ascii_digit)
        && h[108..128].iter().all(|&b| b == 0);
    if !shaped {
        return None;
    }
    std::str::from_utf8(&h[96..108]).ok()?.parse().ok()
}

fn ext4(h: &[u8]) -> Option<(u64, String)> {
    let s = h.get(EXT4_SUPER..EXT4_SUPER + 1024)?;
    if le16(s, 56) != 0xEF53 {
        return None;
    }
    let log = le32(s, 24);
    if log > 6 {
        return None;
    }
    let block = 1024u64 << log;
    const INCOMPAT_64BIT: u32 = 0x80;
    let mut blocks = le32(s, 4) as u64;
    if le32(s, 96) & INCOMPAT_64BIT != 0 {
        blocks |= (le32(s, 0x150) as u64) << 32;
    }
    let label = String::from_utf8_lossy(&s[120..136]).trim_end_matches('\0').to_string();
    Some((blocks.checked_mul(block)?, label))
}

struct ZipEntry {
    name: String,
    method: u16,
    compressed: u64,
    local_offset: u64,
}

/// An Android update zip by what it carries, and the device its metadata or updater script
/// names.
fn zip_format(f: &mut File, bytes: u64) -> Option<Format> {
    let entries = zip_entries(f, bytes)?;
    let has = |n: &str| entries.iter().any(|e| e.name == n);
    let mut device = || {
        entries
            .iter()
            .find(|e| e.name == "META-INF/com/android/metadata")
            .and_then(|e| zip_read(f, e))
            .and_then(|text| metadata_device(&text))
            .or_else(|| {
                entries
                    .iter()
                    .find(|e| e.name == "META-INF/com/google/android/updater-script")
                    .and_then(|e| zip_read(f, e))
                    .and_then(|text| script_device(&text))
            })
    };
    if has("payload.bin") {
        return Some(Format::OtaPayload { device: device() });
    }
    let dat = has("system.new.dat") || has("system.new.dat.br");
    if dat && has("system.transfer.list") {
        return Some(Format::OtaBlocks { device: device() });
    }
    Some(Format::Zip)
}

fn zip_entries(f: &mut File, bytes: u64) -> Option<Vec<ZipEntry>> {
    let tail_len = bytes.min(ZIP_PEEK + 22);
    let tail_start = bytes - tail_len;
    let mut tail = vec![0u8; tail_len as usize];
    f.seek(SeekFrom::Start(tail_start)).ok()?;
    f.read_exact(&mut tail).ok()?;
    let eocd = (0..=tail.len().checked_sub(22)?)
        .rev()
        .find(|&i| tail[i..i + 4] == [b'P', b'K', 5, 6])?;
    let mut count = le16(&tail, eocd + 10) as u64;
    let mut cd_size = le32(&tail, eocd + 12) as u64;
    let mut cd_offset = le32(&tail, eocd + 16) as u64;
    if cd_offset == 0xFFFF_FFFF && eocd >= 20 && tail[eocd - 20..eocd - 16] == [b'P', b'K', 6, 7] {
        let at = le64(&tail, eocd - 12);
        let mut z = [0u8; 56];
        f.seek(SeekFrom::Start(at)).ok()?;
        f.read_exact(&mut z).ok()?;
        if z[..4] != [b'P', b'K', 6, 6] {
            return None;
        }
        count = le64(&z, 32);
        cd_size = le64(&z, 40);
        cd_offset = le64(&z, 48);
    }
    if cd_offset.checked_add(cd_size)? > bytes || cd_size > 64 << 20 {
        return None;
    }
    let mut cd = vec![0u8; cd_size as usize];
    f.seek(SeekFrom::Start(cd_offset)).ok()?;
    f.read_exact(&mut cd).ok()?;
    let mut out = Vec::new();
    let mut i = 0;
    while out.len() as u64 != count && i + 46 <= cd.len() && cd[i..i + 4] == [b'P', b'K', 1, 2] {
        let name_len = le16(&cd, i + 28) as usize;
        let extra_len = le16(&cd, i + 30) as usize;
        let comment_len = le16(&cd, i + 32) as usize;
        let name = cd.get(i + 46..i + 46 + name_len)?;
        out.push(ZipEntry {
            name: String::from_utf8_lossy(name).into_owned(),
            method: le16(&cd, i + 10),
            compressed: le32(&cd, i + 20) as u64,
            local_offset: le32(&cd, i + 42) as u64,
        });
        i += 46 + name_len + extra_len + comment_len;
    }
    Some(out)
}

/// A small text entry, stored or deflated; anything larger than the peek size is skipped.
fn zip_read(f: &mut File, e: &ZipEntry) -> Option<String> {
    if e.compressed > ZIP_PEEK {
        return None;
    }
    let mut local = [0u8; 30];
    f.seek(SeekFrom::Start(e.local_offset)).ok()?;
    f.read_exact(&mut local).ok()?;
    if local[..4] != [b'P', b'K', 3, 4] {
        return None;
    }
    let skip = le16(&local, 26) as i64 + le16(&local, 28) as i64;
    f.seek(SeekFrom::Current(skip)).ok()?;
    let mut data = vec![0u8; e.compressed as usize];
    f.read_exact(&mut data).ok()?;
    let mut text = String::new();
    match e.method {
        0 => text = String::from_utf8_lossy(&data).into_owned(),
        8 => {
            flate2::read::DeflateDecoder::new(&data[..])
                .take(ZIP_PEEK)
                .read_to_string(&mut text)
                .ok()?;
        }
        _ => return None,
    }
    Some(text)
}

fn metadata_device(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("pre-device="))
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
}

/// The first `getprop("ro.product.device") == "x"` an updater script asserts.
fn script_device(text: &str) -> Option<String> {
    let at = text.find("getprop(\"ro.product.device\") == \"")?;
    let rest = &text[at + "getprop(\"ro.product.device\") == \"".len()..];
    Some(rest[..rest.find('"')?].to_string())
}

struct Sums {
    sha256: String,
    md5: String,
    /// The first 16 bytes are the MD5 of the rest, as a Walkman OS UPG is sealed.
    sealed_md5: bool,
}

fn digest(
    path: &Path,
    total: u64,
    sealed: bool,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<Sums> {
    let mut f = File::open(path)?;
    let mut sha = Sha256::new();
    let mut md5 = Md5::new();
    let mut rest = sealed.then(Md5::new);
    let mut seal = [0u8; 16];
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        sha.update(chunk);
        md5.update(chunk);
        if let Some(r) = rest.as_mut() {
            let head = 16u64.saturating_sub(done).min(n as u64) as usize;
            if head > 0 {
                seal[done as usize..done as usize + head].copy_from_slice(&chunk[..head]);
            }
            r.update(&chunk[head..]);
        }
        done += n as u64;
        on_progress(done, total);
    }
    Ok(Sums {
        sha256: hex(&sha.finalize()),
        md5: hex(&md5.finalize()),
        sealed_md5: rest.is_some_and(|r| done > 16 && r.finalize()[..] == seal[..]),
    })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn temp(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uwm-identify-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    fn run(p: &Path) -> Identified {
        identify(p, &AtomicBool::new(false), &mut |_, _| {}).unwrap()
    }

    fn nwwm(payload: u64, body: usize) -> Vec<u8> {
        let mut h = b"NWWM".to_vec();
        h.extend(std::iter::repeat_n(b'a', 56));
        h.extend(std::iter::repeat_n(b'0', 36));
        h.extend(format!("{payload:012}").bytes());
        h.extend(std::iter::repeat_n(0u8, 20));
        h.extend(std::iter::repeat_n(7u8, body));
        h
    }

    #[test]
    fn reads_the_nwwm_header_and_sees_a_short_download() {
        let whole = run(&temp("whole.UPG", &nwwm(1000, 1000 + 9)));
        assert_eq!(whole.format, Format::SonyNwwm { payload_bytes: 1000 });
        assert_eq!(whole.damage, None);
        let cut = run(&temp("cut.UPG", &nwwm(1000, 500)));
        assert_eq!(
            cut.damage,
            Some(Damage::Truncated {
                expected: 1128,
                actual: 628
            })
        );
        let mut odd = nwwm(1000, 1009);
        odd[70] = b'x';
        assert_eq!(run(&temp("odd.UPG", &odd)).format, Format::Unknown);
    }

    #[test]
    fn a_walkman_os_upg_is_sealed_by_the_md5_of_the_rest() {
        let body = vec![3u8; 5000];
        let mut sealed = Md5::digest(&body).to_vec();
        sealed.extend(&body);
        assert_eq!(run(&temp("a.UPG", &sealed)).format, Format::SonyUpg);
        sealed[100] ^= 1;
        assert_eq!(run(&temp("b.UPG", &sealed)).format, Format::Unknown);
        assert_eq!(run(&temp("c.bin", &sealed)).format, Format::Unknown);
    }

    #[test]
    fn reads_a_kernel_banner_across_chunks() {
        let mut img = vec![0u8; (1 << 20) + 100];
        img[56..60].copy_from_slice(b"ARM\x64");
        let banner = b"Linux version 4.14.78-g69dd21e (builder@host) (gcc version 4.9.x) #1 SMP PREEMPT\n";
        let at = (1 << 20) - 20;
        img[at..at + banner.len()].copy_from_slice(banner);
        let r = run(&temp("Image", &img));
        assert_eq!(
            r.format,
            Format::KernelImage {
                release: Some("4.14.78-g69dd21e".into())
            }
        );
        img[at..at + 5].copy_from_slice(b"xxxxx");
        assert_eq!(run(&temp("Image2", &img)).format, Format::KernelImage { release: None });
    }

    #[test]
    fn reads_an_ext4_superblock() {
        let mut img = vec![0u8; 8192];
        let s = EXT4_SUPER;
        img[s + 4..s + 8].copy_from_slice(&2u32.to_le_bytes());
        img[s + 24..s + 28].copy_from_slice(&2u32.to_le_bytes());
        img[s + 56..s + 58].copy_from_slice(&0xEF53u16.to_le_bytes());
        img[s + 120..s + 126].copy_from_slice(b"system");
        let r = run(&temp("system.img", &img));
        assert_eq!(
            r.format,
            Format::Ext4 {
                fs_bytes: 8192,
                label: "system".into()
            }
        );
        assert_eq!(r.damage, None);
        let r = run(&temp("cut.img", &img[..6000]));
        assert_eq!(
            r.damage,
            Some(Damage::Truncated {
                expected: 8192,
                actual: 6000
            })
        );

        // A 64-bit superblock whose block count times block size does not fit in u64.
        img[s + 24..s + 28].copy_from_slice(&6u32.to_le_bytes());
        img[s + 96..s + 100].copy_from_slice(&0x80u32.to_le_bytes());
        img[s + 0x150..s + 0x154].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(ext4(&img), None);
    }

    fn zip(entries: &[(&str, &[u8])], comment: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            out.extend(b"PK\x03\x04");
            out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0, 0]);
            out.extend(name.as_bytes());
            out.extend(*data);
            cd.extend(b"PK\x01\x02");
            cd.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend((data.len() as u32).to_le_bytes());
            cd.extend((data.len() as u32).to_le_bytes());
            cd.extend((name.len() as u16).to_le_bytes());
            cd.extend([0; 12]);
            cd.extend(offset.to_le_bytes());
            cd.extend(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend(&cd);
        out.extend(b"PK\x05\x06");
        out.extend([0, 0, 0, 0]);
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((cd.len() as u32).to_le_bytes());
        out.extend(cd_offset.to_le_bytes());
        out.extend((comment.len() as u16).to_le_bytes());
        out.extend(comment);
        out
    }

    #[test]
    fn tells_update_zips_apart_and_names_their_device() {
        let blocks = zip(
            &[
                ("system.new.dat", b"x"),
                ("system.transfer.list", b"4\n"),
                ("META-INF/com/android/metadata", b"post-build=x\npre-device=ford\n"),
            ],
            b"signed by SignApk",
        );
        assert_eq!(
            run(&temp("u.bin", &blocks)).format,
            Format::OtaBlocks {
                device: Some("ford".into())
            }
        );
        let script = zip(
            &[
                ("system.new.dat", b"x"),
                ("system.transfer.list", b"4\n"),
                (
                    "META-INF/com/google/android/updater-script",
                    b"assert(getprop(\"ro.product.device\") == \"ford\" || abort(\"no\"));",
                ),
            ],
            b"",
        );
        assert_eq!(
            run(&temp("l.zip", &script)).format,
            Format::OtaBlocks {
                device: Some("ford".into())
            }
        );
        let ab = zip(&[("payload.bin", b"CrAU"), ("payload_properties.txt", b"")], b"");
        assert_eq!(run(&temp("ab.zip", &ab)).format, Format::OtaPayload { device: None });
        let plain = zip(&[("a.txt", b"hi")], b"");
        assert_eq!(run(&temp("p.zip", &plain)).format, Format::Zip);
    }

    #[test]
    fn a_backup_list_is_whole_when_every_image_reads_as_the_player_hashed_it() {
        let dir = std::env::temp_dir().join(format!("uwm-identify-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut list = format!("{BACKUP_HEADER}\n");
        for name in ["android", "option2", "recovery", "nvram"] {
            let body = name.repeat(1000);
            std::fs::write(dir.join(format!("{name}.img")), &body).unwrap();
            list += &format!("{name} 2018-01-01 {} {name}.img\n", hex(&Md5::digest(body.as_bytes())));
        }
        std::fs::write(dir.join("manifest.txt"), &list).unwrap();
        let r = run(&dir.join("manifest.txt"));
        assert_eq!(r.format, Format::WmBackup { images: 4 });
        assert_eq!(r.damage, None);

        std::fs::write(dir.join("nvram.img"), "torn").unwrap();
        assert_eq!(
            run(&dir.join("manifest.txt")).damage,
            Some(Damage::Unlike {
                file: "nvram.img".into()
            })
        );
        std::fs::remove_file(dir.join("option2.img")).unwrap();
        assert_eq!(
            run(&dir.join("manifest.txt")).damage,
            Some(Damage::Unlike {
                file: "option2.img".into()
            })
        );

        let without = list
            .replace("recovery 2018", "boot 2018")
            .replace("recovery.img", "boot.img");
        std::fs::write(dir.join("manifest.txt"), without).unwrap();
        assert_eq!(
            run(&dir.join("manifest.txt")).format,
            Format::Unknown,
            "no recovery: not a backup"
        );
        std::fs::write(dir.join("manifest.txt"), list.replace("nvram.img", "../nvram.img")).unwrap();
        assert_eq!(run(&dir.join("manifest.txt")).format, Format::Unknown);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hashes_match_the_usual_tools() {
        let r = run(&temp("abc.txt", b"abc"));
        assert_eq!(
            r.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(r.md5, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(r.known, None);
    }
}
