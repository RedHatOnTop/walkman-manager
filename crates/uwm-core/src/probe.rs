//! One `adb shell` round trip that reads everything the overview needs, and the
//! parsers that turn it into a [`Snapshot`]. The script runs on both Sony's
//! Linux (ash + busybox) and Android (mksh + toybox), so it sticks to POSIX sh.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::adb::Adb;
use crate::error::Result;
use crate::profile::{self, Profile};

pub const SCRIPT: &str = include_str!("probe.sh");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub taken_at_ms: u64,
    pub identity: Identity,
    pub system: SystemInfo,
    pub power: Option<Power>,
    pub memory: Option<Memory>,
    pub volumes: Vec<Volume>,
    pub audio: Audio,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub serial: Option<String>,
    pub profile_id: Option<String>,
    pub family: Option<String>,
    pub model: Option<String>,
    /// "storage" when the model was told apart by capacity alone.
    pub model_basis: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    WalkmanLinux,
    Android,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub platform: Platform,
    /// "Android 13", "Walkman OS"
    pub os: String,
    /// "TrebleDroid GSI", "Sony firmware"
    pub distribution: Option<String>,
    pub firmware: Option<String>,
    pub api_level: Option<u32>,
    pub build_id: Option<String>,
    pub build_type: Option<String>,
    pub security_patch: Option<String>,
    /// Sony's firmware version as recorded on the vendor side of an Android build.
    pub vendor_firmware: Option<String>,
    pub build: Option<String>,
    pub kernel: Option<String>,
    pub kernel_date: Option<String>,
    pub slot: Option<String>,
    pub bootloader_unlocked: Option<bool>,
    pub verified_boot: Option<String>,
    pub container: Option<Container>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub root: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Power {
    pub level: Option<u8>,
    /// Kernel wording: "Charging", "Discharging", "Full", "Not charging".
    pub status: Option<String>,
    pub voltage_mv: Option<u32>,
    pub temperature_c: Option<f32>,
    pub health: Option<String>,
    pub external_power: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub total_kb: u64,
    pub available_kb: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VolumeKind {
    Internal,
    Card,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub kind: VolumeKind,
    pub mount: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Audio {
    pub cards: Vec<AudioCard>,
    pub streams: Vec<AudioStream>,
    /// AudioFlinger's output threads, read only where the shell user cannot open
    /// /proc/asound: an Android build without root.
    pub outputs: Vec<AudioOutput>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioOutput {
    /// The thread's name, "AudioOut_25".
    pub name: String,
    /// Lower-case thread type: "mixer", "direct", "offload".
    pub kind: String,
    /// Out of standby: the HAL stream is being written.
    pub active: bool,
    /// The HAL format without its prefix: "PCM_32_BIT", "DSD".
    pub format: Option<String>,
    pub rate: Option<u32>,
    pub channels: Option<u32>,
    pub encoding: Option<Encoding>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioCard {
    pub index: u32,
    pub id: String,
    pub name: String,
    /// The card that drives the headphone output (the S-Master codec).
    pub main_output: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStream {
    pub card: u32,
    pub device: u32,
    /// Lower-case ALSA state ("running", "prepared", "paused") or "closed".
    pub state: String,
    pub format: Option<String>,
    pub rate: Option<u32>,
    pub channels: Option<u32>,
    pub encoding: Option<Encoding>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Encoding {
    Pcm {
        bits: u32,
        rate: u32,
    },
    /// Native DSD; `rate` is the one-bit rate, 2_822_400 for DSD64.
    Dsd {
        rate: u64,
    },
}

pub fn run(adb: &Adb, serial: &str) -> Result<Snapshot> {
    let text = adb.shell(serial, SCRIPT, Duration::from_secs(15))?;
    Ok(parse(&text))
}

pub fn parse(text: &str) -> Snapshot {
    let sections = split_sections(text);
    let get = |name: &str| sections.get(name).map(String::as_str).unwrap_or("");
    let props = parse_props(get("props"));
    let mounts = parse_mounts(get("mounts"));
    let volumes = volumes(&parse_df(get("df")), &mounts);
    // ro.sony.swid alone does not tell: Sony's Android vendor partition sets it too.
    let platform = if props.get("ro.hardware").map(String::as_str) == Some("icx")
        || props.get("ro.product.model").map(String::as_str) == Some("HAGOROMO_MODEL")
    {
        Platform::WalkmanLinux
    } else {
        Platform::Android
    };
    let profile = profile::by_props(&props);
    Snapshot {
        taken_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        identity: identity(&props, profile, &volumes),
        system: system(platform, &props, get("version"), &mounts),
        power: parse_power(get("power")),
        memory: parse_meminfo(get("meminfo")),
        volumes,
        audio: Audio {
            outputs: parse_flinger(get("flinger")),
            ..parse_audio(get("asound"))
        },
    }
}

fn split_sections(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut body = String::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(name) = line.strip_prefix("@@") {
            if let Some(prev) = current.take() {
                out.insert(prev, std::mem::take(&mut body));
            }
            current = (name != "end").then(|| name.to_owned());
            continue;
        }
        if current.is_some() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some(prev) = current {
        out.insert(prev, body);
    }
    out
}

pub fn parse_props(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut last: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('[')
            && let Some((key, value)) = rest.split_once("]: [")
        {
            let value = value.strip_suffix(']').unwrap_or(value);
            out.insert(key.to_owned(), value.to_owned());
            last = Some(key.to_owned());
            continue;
        }
        // A value with a newline continues on the following lines.
        if let Some(key) = &last
            && let Some(v) = out.get_mut(key)
        {
            v.push('\n');
            v.push_str(line.strip_suffix(']').unwrap_or(line));
        }
    }
    out
}

fn identity(props: &BTreeMap<String, String>, profile: Option<&Profile>, volumes: &[Volume]) -> Identity {
    let serial = props
        .get("ro.serialno")
        .or_else(|| props.get("ro.boot.serialno"))
        .filter(|s| !s.is_empty())
        .cloned();
    let internal = volumes.iter().find(|v| v.kind == VolumeKind::Internal);
    let variant = match (profile, internal) {
        (Some(p), Some(v)) => p.variant_for_storage(v.total_bytes),
        _ => None,
    };
    let single = profile.filter(|p| p.variants.len() == 1).map(|p| &p.variants[0]);
    let model = variant.or(single).map(|v| v.model.clone());
    Identity {
        serial,
        profile_id: profile.map(|p| p.id.clone()),
        family: profile.map(|p| p.family.clone()),
        model_basis: match (variant, single) {
            (Some(_), None) => Some("storage"),
            (_, Some(_)) => Some("profile"),
            _ => None,
        },
        model,
    }
}

fn system(platform: Platform, props: &BTreeMap<String, String>, version: &str, mounts: &[Mount]) -> SystemInfo {
    let p = |k: &str| props.get(k).filter(|v| !v.is_empty()).cloned();
    let (kernel, kernel_date) = parse_kernel_version(version);
    match platform {
        Platform::WalkmanLinux => SystemInfo {
            platform,
            os: "Walkman OS".into(),
            distribution: Some("Sony firmware".into()),
            firmware: p("ro.sony.swid").and_then(|s| firmware_from_swid(&s)),
            api_level: None,
            build_id: None,
            build_type: None,
            security_patch: None,
            vendor_firmware: None,
            build: p("ro.sony.swid"),
            kernel,
            kernel_date,
            slot: None,
            bootloader_unlocked: None,
            verified_boot: None,
            container: mounts
                .iter()
                .find(|m| m.target != "/system" && m.target.ends_with("/system") && m.source.starts_with("/dev/loop"))
                .map(|m| Container {
                    root: m.target.trim_end_matches("/system").to_owned(),
                }),
        },
        Platform::Android => {
            let release = p("ro.build.version.release").unwrap_or_default();
            let fingerprint = p("ro.build.fingerprint");
            let distribution = if p("ro.product.model").is_some_and(|m| m.contains("TrebleDroid"))
                || p("ro.product.system.name").is_some_and(|m| m.starts_with("treble_"))
            {
                Some("TrebleDroid GSI".to_owned())
            } else if p("ro.cm.version").is_some() || p("ro.lineage.version").is_some() {
                Some("LineageOS".to_owned())
            } else if fingerprint.as_deref().is_some_and(|f| f.starts_with("Sony/")) {
                Some("Sony firmware".to_owned())
            } else {
                None
            };
            SystemInfo {
                platform,
                os: format!("Android {release}").trim_end().to_owned(),
                distribution,
                firmware: p("ro.build.display.id"),
                api_level: p("ro.build.version.sdk").and_then(|v| v.parse().ok()),
                build_id: p("ro.build.id"),
                build_type: p("ro.build.type"),
                security_patch: p("ro.build.version.security_patch"),
                vendor_firmware: p("ro.sony.swid").and_then(|s| firmware_from_swid(&s)),
                build: fingerprint,
                kernel,
                kernel_date,
                slot: p("ro.boot.slot_suffix").map(|s| s.trim_start_matches('_').to_owned()),
                bootloader_unlocked: p("ro.boot.flash.locked").map(|v| v == "0"),
                verified_boot: p("ro.boot.verifiedbootstate"),
                container: None,
            }
        }
    }
}

/// "01.20.E.1.02.00" carries the firmware version in its last three fields.
fn firmware_from_swid(swid: &str) -> Option<String> {
    let parts: Vec<&str> = swid.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    let major: u32 = parts[parts.len() - 3].parse().ok()?;
    let minor = parts[parts.len() - 2];
    minor
        .chars()
        .all(|c| c.is_ascii_digit())
        .then(|| format!("{major}.{minor}"))
}

fn parse_kernel_version(text: &str) -> (Option<String>, Option<String>) {
    let line = text.lines().next().unwrap_or("").trim();
    let release = line
        .strip_prefix("Linux version ")
        .and_then(|r| r.split_whitespace().next())
        .map(str::to_owned);
    // The build date follows the last "PREEMPT" or "SMP" flag word.
    let date = line
        .rsplit_once(" PREEMPT ")
        .or_else(|| line.rsplit_once(" SMP "))
        .map(|(_, d)| d.trim().to_owned())
        .filter(|d| !d.is_empty());
    (release, date)
}

fn parse_power(text: &str) -> Option<Power> {
    let mut supplies: Vec<BTreeMap<String, String>> = Vec::new();
    for line in text.lines() {
        if line.starts_with("== ") {
            supplies.push(BTreeMap::new());
        } else if let (Some(s), Some((k, v))) = (supplies.last_mut(), line.split_once('=')) {
            s.insert(k.to_owned(), v.trim().to_owned());
        }
    }
    let battery = supplies
        .iter()
        .find(|s| s.get("type").map(String::as_str) == Some("Battery"))?;
    let num = |k: &str| battery.get(k).and_then(|v| v.parse::<i64>().ok());
    let external_power = supplies.iter().any(|s| {
        s.get("type").map(String::as_str) != Some("Battery") && s.get("online").map(String::as_str) == Some("1")
    });
    Some(Power {
        level: num("capacity").map(|c| c.clamp(0, 100) as u8),
        status: battery.get("status").cloned(),
        voltage_mv: num("voltage_now").map(|uv| (uv / 1000) as u32),
        temperature_c: num("temp").map(|t| t as f32 / 10.0),
        health: battery.get("health").cloned(),
        external_power,
    })
}

fn parse_meminfo(text: &str) -> Option<Memory> {
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next()?.parse().ok())
    };
    Some(Memory {
        total_kb: field("MemTotal")?,
        available_kb: field("MemAvailable"),
    })
}

#[derive(Debug, Clone)]
struct Mount {
    source: String,
    target: String,
    fstype: String,
}

fn parse_mounts(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            Some(Mount {
                source: f.next()?.to_owned(),
                target: unescape_mount(f.next()?),
                fstype: f.next()?.to_owned(),
            })
        })
        .collect()
}

/// /proc/mounts writes spaces in paths as `\040`.
fn unescape_mount(s: &str) -> String {
    s.replace("\\040", " ")
}

#[derive(Debug, Clone)]
struct DfRow {
    source: String,
    total_kb: u64,
    used_kb: u64,
    free_kb: u64,
    target: String,
}

fn parse_df(text: &str) -> Vec<DfRow> {
    let mut rows = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines().skip_while(|l| !l.starts_with("Filesystem")).skip(1) {
        let mut fields: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
        // busybox puts a long device name on a line of its own.
        if fields.len() == 1 {
            pending = fields.pop();
            continue;
        }
        if let Some(source) = pending.take() {
            fields.insert(0, source);
        }
        if fields.len() < 6 {
            continue;
        }
        let n = |i: usize| fields[i].parse::<u64>().ok();
        if let (Some(total_kb), Some(used_kb), Some(free_kb)) = (n(1), n(2), n(3)) {
            rows.push(DfRow {
                source: fields[0].clone(),
                total_kb,
                used_kb,
                free_kb,
                target: fields[5..].join(" "),
            });
        }
    }
    rows
}

fn volumes(df: &[DfRow], mounts: &[Mount]) -> Vec<Volume> {
    let fstype = |target: &str| {
        mounts
            .iter()
            .rev()
            .find(|m| m.target == target)
            .map(|m| m.fstype.clone())
            .unwrap_or_default()
    };
    let mut out: Vec<Volume> = Vec::new();
    for row in df {
        let card = card_id(&row.target);
        let kind = if row.source == "/emmc@contents" || (row.target == "/data" && row.source.starts_with("/dev/block"))
        {
            VolumeKind::Internal
        } else if card.is_some()
            || row.source.starts_with("/dev/block/mmcblk1")
            || row.source.starts_with("/dev/mmcblk1")
        {
            VolumeKind::Card
        } else {
            continue;
        };
        if out
            .iter()
            .any(|v| v.mount == row.target || (card.is_some() && card_id(&v.mount) == card))
        {
            continue;
        }
        // The FUSE view reads as `fuse`; the card's own filesystem is on the vold mount behind it.
        let filesystem = card
            .map(|id| fstype(&format!("/mnt/media_rw/{id}")))
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| fstype(&row.target));
        out.push(Volume {
            kind,
            mount: row.target.clone(),
            filesystem,
            total_bytes: row.total_kb * 1024,
            used_bytes: row.used_kb * 1024,
            free_bytes: row.free_kb * 1024,
        });
    }
    out.sort_by_key(|v| (v.kind != VolumeKind::Internal, v.mount.clone()));
    out
}

/// Android mounts a card at `/mnt/media_rw/<id>` and serves it through FUSE at `/storage/<id>`.
/// The shell user cannot stat the first, so toybox df on a build without root lists only the second.
fn card_id(target: &str) -> Option<&str> {
    target.strip_prefix("/mnt/media_rw/").or_else(|| {
        target
            .strip_prefix("/storage/")
            .filter(|id| !matches!(*id, "emulated" | "self") && !id.contains('/'))
    })
}

fn parse_audio(text: &str) -> Audio {
    let mut audio = Audio::default();
    let mut lines = text.lines().peekable();
    // /proc/asound/cards: " 1 [imxaudiocxd3778]: imx-audio-cxd37 - imx-audio-cxd3778gf"
    while let Some(line) = lines.peek() {
        if line.starts_with("== ") {
            break;
        }
        let line = lines.next().unwrap();
        let t = line.trim_start();
        let Some((idx, rest)) = t.split_once(' ') else { continue };
        let Ok(index) = idx.parse::<u32>() else { continue };
        let Some((id_part, name_part)) = rest.split_once("]:") else {
            continue;
        };
        let id = id_part.trim_start_matches('[').trim().to_owned();
        let name = name_part.split(" - ").nth(1).unwrap_or(name_part).trim().to_owned();
        let lower = format!("{id} {name}").to_lowercase();
        let main_output = lower.contains("cxd3778") || lower.contains("sonysoccard");
        audio.cards.push(AudioCard {
            index,
            id,
            name,
            main_output,
        });
    }
    let mut block: Vec<&str> = Vec::new();
    let mut header: Option<&str> = None;
    for line in lines.chain(std::iter::once("== ")) {
        if let Some(next) = line.strip_prefix("== ") {
            if let Some(stream) = header.and_then(|h| parse_stream(h, &block)) {
                audio.streams.push(stream);
            }
            header = Some(next).filter(|n| !n.is_empty());
            block.clear();
        } else {
            block.push(line);
        }
    }
    audio
}

/// `path` is ".../cardN/pcmMp/sub0"; `block` is its status, "--", then hw_params.
fn parse_stream(path: &str, block: &[&str]) -> Option<AudioStream> {
    let card = path.split("/card").nth(1)?.split('/').next()?.parse().ok()?;
    let device = path
        .split("/pcm")
        .nth(1)?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()?;
    let split = block.iter().position(|l| *l == "--").unwrap_or(block.len());
    let (status, params) = (&block[..split], block.get(split + 1..).unwrap_or(&[]));
    let state = if status.first().map(|l| l.trim()) == Some("closed") {
        "closed".to_owned()
    } else {
        status
            .iter()
            .find_map(|l| l.trim().strip_prefix("state:"))
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_else(|| "open".to_owned())
    };
    let param = |k: &str| {
        params
            .iter()
            .find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix(':')))
            .map(|v| v.trim().to_owned())
    };
    let format = param("format");
    let rate: Option<u32> = param("rate").and_then(|r| r.split_whitespace().next()?.parse().ok());
    let channels = param("channels").and_then(|c| c.parse().ok());
    let encoding = match (&format, rate) {
        (Some(f), Some(r)) => encoding(f, r),
        _ => None,
    };
    Some(AudioStream {
        card,
        device,
        state,
        format,
        rate,
        channels,
        encoding,
    })
}

fn encoding(format: &str, rate: u32) -> Option<Encoding> {
    let pcm = |bits| Some(Encoding::Pcm { bits, rate });
    match format {
        "S8" | "U8" => pcm(8),
        "S16_LE" | "S16_BE" | "U16_LE" | "U16_BE" => pcm(16),
        "S24_LE" | "S24_BE" | "S24_3LE" | "S24_3BE" | "U24_LE" | "U24_3LE" => pcm(24),
        "S32_LE" | "S32_BE" | "U32_LE" | "U32_BE" | "FLOAT_LE" => pcm(32),
        "DSD_U8" => Some(Encoding::Dsd {
            rate: u64::from(rate) * 8,
        }),
        "DSD_U16_LE" | "DSD_U16_BE" => Some(Encoding::Dsd {
            rate: u64::from(rate) * 16,
        }),
        "DSD_U32_LE" | "DSD_U32_BE" => Some(Encoding::Dsd {
            rate: u64::from(rate) * 32,
        }),
        _ => None,
    }
}

/// `dumpsys media.audio_flinger` cut to each live output thread's header and four fields:
/// "Output thread 0x..., name AudioOut_25, tid 31262, type 4 (OFFLOAD):", "  Standby: no",
/// "  Sample rate: 2822400 Hz", "  HAL format: 0x1a000000 (AUDIO_FORMAT_DSD)", "  Channel count: 2".
fn parse_flinger(text: &str) -> Vec<AudioOutput> {
    let mut out: Vec<AudioOutput> = Vec::new();
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("Output thread ") {
            let name = header
                .split(", ")
                .find_map(|f| f.strip_prefix("name "))
                .unwrap_or_default()
                .to_owned();
            let kind = header
                .rsplit_once('(')
                .and_then(|(_, k)| k.split_once(')'))
                .map(|(k, _)| k.to_lowercase())
                .unwrap_or_default();
            out.push(AudioOutput {
                name,
                kind,
                active: false,
                format: None,
                rate: None,
                channels: None,
                encoding: None,
            });
            continue;
        }
        let (Some(o), Some((key, value))) = (out.last_mut(), line.trim().split_once(": ")) else {
            continue;
        };
        match key {
            "Standby" => o.active = value == "no",
            "Sample rate" => o.rate = value.split_whitespace().next().and_then(|r| r.parse().ok()),
            "HAL format" => {
                o.format = value
                    .split_once("(AUDIO_FORMAT_")
                    .and_then(|(_, f)| f.strip_suffix(')'))
                    .map(str::to_owned);
            }
            "Channel count" => o.channels = value.parse().ok(),
            _ => {}
        }
    }
    for o in &mut out {
        o.encoding = match (o.format.as_deref(), o.rate) {
            (Some(f), Some(rate)) => hal_encoding(f, rate),
            _ => None,
        };
    }
    out
}

fn hal_encoding(format: &str, rate: u32) -> Option<Encoding> {
    let pcm = |bits| Some(Encoding::Pcm { bits, rate });
    match format {
        "PCM_8_BIT" => pcm(8),
        "PCM_16_BIT" => pcm(16),
        "PCM_8_24_BIT" | "PCM_24_BIT_PACKED" => pcm(24),
        "PCM_32_BIT" | "PCM_FLOAT" => pcm(32),
        // AudioFlinger counts DSD in one-bit samples already.
        "DSD" => Some(Encoding::Dsd { rate: u64::from(rate) }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A55: &str = include_str!("../tests/fixtures/nw-a55-stock-probe.txt");
    const ZX507: &str = include_str!("../tests/fixtures/nw-zx507-a13-probe.txt");

    #[test]
    fn a55_stock_snapshot() {
        let s = parse(A55);
        assert_eq!(s.identity.profile_id.as_deref(), Some("nw-a50"));
        assert_eq!(s.identity.model.as_deref(), Some("NW-A55"));
        assert_eq!(s.identity.model_basis, Some("storage"));
        assert_eq!(s.identity.serial.as_deref(), Some("TESTA55SERIAL01"));
        assert_eq!(s.system.platform, Platform::WalkmanLinux);
        assert_eq!(s.system.firmware.as_deref(), Some("1.02"));
        assert_eq!(s.system.kernel.as_deref(), Some("3.10.26"));
        assert_eq!(s.system.kernel_date.as_deref(), Some("Wed Jul 31 16:22:50 JST 2019"));
        assert_eq!(s.system.container.as_ref().map(|c| c.root.as_str()), Some("/tmp/aroot"));
        let p = s.power.unwrap();
        assert!(p.level.is_some_and(|l| l <= 100));
        assert!(p.external_power);
        assert_eq!(p.temperature_c, None);
        assert_eq!(s.volumes.len(), 1);
        assert_eq!(s.volumes[0].kind, VolumeKind::Internal);
        assert_eq!(s.volumes[0].mount, "/tmp/c");
        assert_eq!(s.volumes[0].filesystem, "vfat");
        assert_eq!(s.volumes[0].total_bytes, 12_855_712 * 1024);
        assert_eq!(s.memory.unwrap().total_kb, 467_512);
        assert_eq!(s.audio.cards.len(), 1);
        assert!(s.audio.cards[0].main_output);
        assert!(s.audio.streams.len() >= 5);
        assert!(s.audio.streams.iter().all(|st| st.state == "closed" && st.card == 0));
    }

    #[test]
    fn crlf_transcripts_parse_the_same() {
        // Walkman OS's adbd has no shell protocol v2 and answers with CRLF.
        let crlf = parse(&A55.replace('\n', "\r\n"));
        let lf = parse(A55);
        assert_eq!(crlf.identity, lf.identity);
        assert_eq!(crlf.volumes.len(), lf.volumes.len());
        assert_eq!(crlf.system.firmware, lf.system.firmware);
    }

    #[test]
    fn zx507_a13_snapshot() {
        let s = parse(ZX507);
        assert_eq!(s.identity.profile_id.as_deref(), Some("nw-zx500"));
        assert_eq!(s.identity.model.as_deref(), Some("NW-ZX507"));
        assert_eq!(s.system.platform, Platform::Android);
        assert_eq!(s.system.os, "Android 13");
        assert_eq!(s.system.distribution.as_deref(), Some("TrebleDroid GSI"));
        assert_eq!(s.system.slot.as_deref(), Some("b"));
        assert_eq!(s.system.bootloader_unlocked, Some(true));
        assert_eq!(s.system.verified_boot.as_deref(), Some("orange"));
        assert_eq!(s.system.kernel.as_deref(), Some("4.14.78-g69dd21e"));
        assert_eq!(s.system.vendor_firmware.as_deref(), Some("4.06"));
        assert_eq!(s.system.api_level, Some(33));
        assert_eq!(s.system.build_id.as_deref(), Some("TQ3C.230805.001.A4"));
        assert_eq!(s.system.security_patch.as_deref(), Some("2023-08-05"));
        let p = s.power.unwrap();
        assert_eq!(p.temperature_c.map(|t| (t * 10.0).round() as i32), Some(394));
        assert!(p.external_power);
        let kinds: Vec<_> = s.volumes.iter().map(|v| (v.kind, v.filesystem.as_str())).collect();
        assert_eq!(kinds, [(VolumeKind::Internal, "ext4"), (VolumeKind::Card, "exfat")]);
        assert_eq!(s.volumes[1].mount, "/mnt/media_rw/3564-3463");
        assert!(s.memory.unwrap().available_kb.is_some());
        let dac: Vec<_> = s
            .audio
            .cards
            .iter()
            .filter(|c| c.main_output)
            .map(|c| c.index)
            .collect();
        assert_eq!(dac, [1]);
    }

    #[test]
    fn running_stream_formats() {
        let block = [
            "state: RUNNING",
            "owner_pid   : 1234",
            "--",
            "access: RW_INTERLEAVED",
            "format: S32_LE",
            "subformat: STD",
            "channels: 2",
            "rate: 192000 (192000/1)",
            "period_size: 1920",
        ];
        let st = parse_stream("/proc/asound/card1/pcm2p/sub0", &block).unwrap();
        assert_eq!((st.card, st.device), (1, 2));
        assert_eq!(st.state, "running");
        assert_eq!(
            st.encoding,
            Some(Encoding::Pcm {
                bits: 32,
                rate: 192_000
            })
        );
        let dsd = [
            "state: RUNNING",
            "--",
            "format: DSD_U32_BE",
            "channels: 2",
            "rate: 88200 (88200/1)",
        ];
        let st = parse_stream("/proc/asound/card1/pcm0p/sub0", &dsd).unwrap();
        assert_eq!(st.encoding, Some(Encoding::Dsd { rate: 2_822_400 }));
        let closed = ["closed", "--", "closed"];
        let st = parse_stream("/proc/asound/card0/pcm10p/sub0", &closed).unwrap();
        assert_eq!((st.device, st.state.as_str()), (10, "closed"));
        assert_eq!((st.format, st.encoding), (None, None));
    }

    #[test]
    fn flinger_outputs_without_root() {
        let text = "Output thread 0xe2c976a8c9a0, name AudioOut_D, tid 31248, type 0 (MIXER):\n\
  Standby: yes\n\
  Sample rate: 192000 Hz\n\
  HAL format: 0x3 (AUDIO_FORMAT_PCM_32_BIT)\n\
  Channel count: 2\n\
Output thread 0xe2c88ee03820, name AudioOut_25, tid 31262, type 4 (OFFLOAD):\n\
  Standby: no\n\
  Sample rate: 2822400 Hz\n\
  HAL format: 0x1a000000 (AUDIO_FORMAT_DSD)\n\
  Channel count: 2\n";
        let outs = parse_flinger(text);
        let got: Vec<_> = outs
            .iter()
            .map(|o| (o.name.as_str(), o.kind.as_str(), o.active, o.format.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                ("AudioOut_D", "mixer", false, Some("PCM_32_BIT")),
                ("AudioOut_25", "offload", true, Some("DSD")),
            ]
        );
        assert_eq!(
            outs[0].encoding,
            Some(Encoding::Pcm {
                bits: 32,
                rate: 192_000
            })
        );
        assert_eq!(outs[1].encoding, Some(Encoding::Dsd { rate: 2_822_400 }));
        assert_eq!(outs[1].channels, Some(2));
        let s = parse(&format!("@@flinger\n{text}@@end\n"));
        assert_eq!(s.audio.outputs.len(), 2);
        assert!(s.audio.cards.is_empty());
    }

    #[test]
    fn swid_and_kernel() {
        assert_eq!(firmware_from_swid("01.20.E.1.02.00").as_deref(), Some("1.02"));
        assert_eq!(firmware_from_swid("garbage"), None);
        let (k, d) = parse_kernel_version(
            "Linux version 4.14.78-g69dd21e (x@y) (gcc version 4.9.x) #1 SMP PREEMPT Thu Oct 15 08:58:55 UTC 2026",
        );
        assert_eq!(k.as_deref(), Some("4.14.78-g69dd21e"));
        assert_eq!(d.as_deref(), Some("Thu Oct 15 08:58:55 UTC 2026"));
    }

    #[test]
    fn busybox_wrapped_df_row() {
        let df = "Filesystem           1K-blocks      Used Available Use% Mounted on\n\
/dev/block/vold/public:179,129\n                      124835840  69369600  55466240  56% /mnt/media_rw/3564-3463\n";
        let rows = parse_df(df);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "/dev/block/vold/public:179,129");
        assert_eq!(rows[0].target, "/mnt/media_rw/3564-3463");
    }

    #[test]
    fn card_through_fuse_only() {
        let df = "Filesystem            1K-blocks     Used Available Use% Mounted on\n\
/dev/block/mmcblk2p28  52213760   382664  51683640   1% /data\n\
/dev/fuse              52213760   382664  51683640   1% /storage/emulated\n\
/dev/fuse             124835840 69266176  55569664  56% /storage/3564-3463\n";
        let mounts = parse_mounts(
            "/dev/block/mmcblk2p28 /data ext4 rw 0 0\n\
/dev/fuse /storage/emulated fuse rw 0 0\n\
/dev/block/vold/public:179,129 /mnt/media_rw/3564-3463 exfat rw 0 0\n\
/dev/fuse /storage/3564-3463 fuse rw 0 0\n",
        );
        let v = volumes(&parse_df(df), &mounts);
        let kinds: Vec<_> = v
            .iter()
            .map(|v| (v.kind, v.mount.as_str(), v.filesystem.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                (VolumeKind::Internal, "/data", "ext4"),
                (VolumeKind::Card, "/storage/3564-3463", "exfat"),
            ]
        );
        assert_eq!(v[1].free_bytes, 55569664 * 1024);
    }
}
