//! Sony's "12 TONE ANALYSIS" data (SMFMF) and the SensMe channels a Walkman puts a
//! track in. The file carries no channels: Sony's PC software embeds a block of
//! features and the player derives the channels from it when it indexes the file.
//! This is a port of what NW-A50 firmware 1.02 does, checked against that firmware's
//! own code.
//!
//! What the player decides channels with is firmware data: the 48 centroids, each
//! cluster's time of day and fallback mood, the cluster inputs' weights and the mood
//! tests' thresholds. None of it is part of this program: [`Table`] reads it from the
//! user's own copy of the firmware, and this module holds only where it is and how the
//! player uses it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::remote::shell_quote;
use crate::adb::Adb;
use crate::error::{Error, Result, Why};

const CHUNK_HEADER: usize = 20;
const MISSING: u8 = 255;
/// Words 4..90 of the player's analysis context: STMO by id, STHF by slot, STBF.
const FEATURES: usize = 86;
const CLUSTERS: usize = 48;
/// An ID3 tag or FLAC block larger than this is not analysis data worth reading.
const MAX_TAG: u64 = 64 << 20;

/// STHF record id 0x800 + i fills slot `STHF_SLOT[i]`.
const STHF_SLOT: [u8; 40] = [
    15, 16, 12, 1, 17, 4, 0, 13, 5, 7, 18, 10, 9, 19, 6, 8, 20, 21, 22, 2, 23, 24, 25, 26, 27, 3, 28, 29, 30, 31, 32,
    14, 33, 34, 35, 36, 11, 37, 38, 39,
];
/// The sixteen cluster inputs in centroid column order: the context byte offset each
/// reads, and where in the clustering code the `vldr` that loads its weight sits (None:
/// the code does not scale it).
const CLUSTER_INPUTS: [(usize, Option<usize>); 16] = [
    (0x144, None),
    (0x13C, Some(0x1AA)),
    (0xDC, Some(0x1EC)),
    (0x158, Some(0x19A)),
    (0x160, Some(0x178)),
    (0xFC, None),
    (0x130, Some(0x14E)),
    (0x150, Some(0x142)),
    (0xE0, Some(0x13A)),
    (0xF4, None),
    (0x15C, Some(0x18A)),
    (0x134, Some(0x156)),
    (0xE8, Some(0x16A)),
    (0x12C, None),
    (0x164, Some(0x1DE)),
    (0xF0, Some(0x166)),
];
/// Where each threshold sits in the mood-test code, ascending: the offset of the `cmp`
/// (or, for a range, the `sub`) whose immediate it is. [`moods`] names them by offset.
const LIMITS: [u16; 68] = [
    0x3C, 0x52, 0x56, 0x5A, 0x70, 0x78, 0x7C, 0xDE, 0xE8, 0xF6, 0x104, 0x122, 0x124, 0x12C, 0x15A, 0x166, 0x1C2, 0x1CC,
    0x1DA, 0x21E, 0x228, 0x236, 0x244, 0x26C, 0x29E, 0x2AA, 0x302, 0x306, 0x310, 0x33C, 0x346, 0x354, 0x418, 0x422,
    0x428, 0x430, 0x43E, 0x44C, 0x45A, 0x46A, 0x47C, 0x4D6, 0x4E0, 0x4F2, 0x4F8, 0x4FE, 0x502, 0x51A, 0x524, 0x532,
    0x540, 0x54E, 0x56A, 0x56E, 0x572, 0x576, 0x57C, 0x586, 0x594, 0x5B8, 0x5C2, 0x5D0, 0x5E0, 0x610, 0x61A, 0x628,
    0x636, 0x644,
];

/// A function of the player's library, found by its prologue and the SHA-256 of its
/// first `len` bytes as [`shape`] leaves them. The two NW-A50 builds seen so far hold it
/// alike apart from where it sits.
struct Code {
    prologue: [u8; 4],
    len: usize,
    shape: &'static str,
}

/// The clustering function; the weights are in its literal pool.
const CLUSTERING: Code = Code {
    prologue: [0x2D, 0xE9, 0xF0, 0x4F],
    len: 0x370,
    shape: "f576462ebf50da9adbfec26c363d94761a0cbf15108d86714c9c5ef433bd052b",
};
/// The mood tests, one selector per channel; the thresholds are its immediates.
const MOOD_TESTS: Code = Code {
    prologue: [0x2D, 0xE9, 0xF0, 0x43],
    len: 0x654,
    shape: "15bf668e8ac78122f21c1595429ae943c5b083f6a775b5728ef8701cfea73316",
};

impl Code {
    fn find(&self, data: &[u8]) -> Option<usize> {
        (0..data.len().saturating_sub(self.len))
            .step_by(2)
            .filter(|&at| data[at..at + 4] == self.prologue)
            .find(|&at| format!("{:x}", Sha256::digest(shape(&data[at..at + self.len]))) == self.shape)
    }
}

fn halfword(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

/// Whether a Thumb-2 halfword starts a 32-bit instruction.
fn wide(h: u16) -> bool {
    matches!(h >> 11, 0b11101..=0b11111)
}

/// Thumb-2 code with what moves between builds of the same source cleared: PC-relative
/// offsets, branch targets, and the immediates of compares, moves, adds and subtracts.
fn shape(code: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(code.len());
    let mut at = 0;
    while let Some(mut h) = halfword(code, at) {
        if wide(h) {
            let Some(mut h2) = halfword(code, at + 2) else {
                break;
            };
            if h & 0xF800 == 0xF000 && h2 & 0x8000 != 0 {
                // b.w, bl, blx
                h &= !0x07FF;
                h2 &= !0x2FFF;
            } else if h & 0xFF7F == 0xF85F {
                // ldr.w from a literal
                h2 &= !0x0FFF;
            } else if h & 0xFF3F == 0xED1F || h & 0xFE7F == 0xE85F {
                // vldr, ldrd from a literal
                h2 &= !0x00FF;
            } else if matches!(h & 0xFBFF, 0xF20F | 0xF2AF)
                || (h & 0xFBF0 == 0xF1B0 && h2 & 0x8F00 == 0x0F00)
                || (matches!(h & 0xFBE0, 0xF1A0 | 0xF100) && h2 & 0x8000 == 0)
            {
                // adr.w, and cmp.w, sub.w, add.w with an immediate
                h &= !0x0400;
                h2 &= !0x70FF;
            } else if h & 0xFBF0 == 0xF240 && h2 & 0x8000 == 0 {
                // movw
                h &= !0x040F;
                h2 &= !0x70FF;
            }
            out.extend_from_slice(&h.to_le_bytes());
            out.extend_from_slice(&h2.to_le_bytes());
            at += 4;
        } else {
            if matches!(h & 0xF800, 0x4800 | 0xA000 | 0x2000 | 0x2800 | 0x3000 | 0x3800)
                || (h & 0xF000 == 0xD000 && h & 0x0F00 < 0x0E00)
            {
                // ldr from a literal, adr, movs, cmp, adds, subs, b<cond>
                h &= 0xFF00;
            } else if h & 0xF800 == 0xE000 {
                // b
                h &= 0xF800;
            } else if h & 0xF500 == 0xB100 {
                // cbz, cbnz
                h &= !0x02F8;
            }
            out.extend_from_slice(&h.to_le_bytes());
            at += 2;
        }
    }
    out
}

/// ThumbExpandImm: a 32-bit instruction's 12-bit modified immediate.
fn expand_imm(imm12: u32) -> u32 {
    let imm8 = imm12 & 0xFF;
    match imm12 >> 8 {
        0 => imm8,
        1 => imm8 << 16 | imm8,
        2 => imm8 << 24 | imm8 << 8,
        3 => imm8 * 0x0101_0101,
        _ => (0x80 | imm12 & 0x7F).rotate_right(imm12 >> 7),
    }
}

/// The immediate of the `cmp` or `subs` (16-bit) or `cmp.w` or `sub.w` (32-bit) at `at`.
fn immediate(data: &[u8], at: usize) -> Option<u32> {
    let h = halfword(data, at)?;
    if matches!(h & 0xF800, 0x2800 | 0x3800) {
        return Some(u32::from(h & 0xFF));
    }
    let h2 = halfword(data, at + 2)?;
    let cmp = h & 0xFBF0 == 0xF1B0 && h2 & 0x8F00 == 0x0F00;
    let sub = h & 0xFBE0 == 0xF1A0 && h2 & 0x8000 == 0;
    (cmp || sub).then(|| expand_imm(u32::from(h & 0x0400) << 1 | u32::from(h2 & 0x7000) >> 4 | u32::from(h2 & 0xFF)))
}

/// The double a `vldr dN, [pc, #±imm]` at `at` loads. Code sits at its own file offset.
fn vldr_literal(data: &[u8], at: usize) -> Option<f64> {
    let (h, h2) = (halfword(data, at)?, halfword(data, at + 2)?);
    if h & 0xFF3F != 0xED1F || h2 & 0x0F00 != 0x0B00 {
        return None;
    }
    let base = (at + 4) & !3;
    let imm = usize::from(h2 & 0xFF) * 4;
    let from = if h & 0x0080 != 0 {
        base + imm
    } else {
        base.checked_sub(imm)?
    };
    Some(f64::from_le_bytes(data.get(from..from + 8)?.try_into().ok()?))
}

/// Context fields the mood tests read.
const PRED_FIELDS: [usize; 35] = [
    0x10, 0x14, 0x18, 0x20, 0x24, 0x28, 0x2C, 0x40, 0x48, 0x4C, 0x50, 0x5C, 0x60, 0x64, 0x68, 0x6C, 0x70, 0x74, 0x80,
    0x84, 0x88, 0xA8, 0xC0, 0xCC, 0xD0, 0xD4, 0xDC, 0xE0, 0xE4, 0xE8, 0x138, 0x13C, 0x140, 0x144, 0x14C,
];

/// What a track's analysis says, as the index keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SensMe {
    /// The analyser's tempo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f32>,
    /// Where the chorus starts: the player begins a SensMe track there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chorus_ms: Option<u32>,
    /// The values the channels come from, as the player decodes them, in hex.
    pub features: String,
    /// The channels the player puts the track in. None until the firmware's table is
    /// known, and for a mix of missing values the port does not follow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<Vec<u8>>,
}

/// The player's analysis context from word 4 on; 255 is "missing".
struct Context([u8; FEATURES]);

impl Context {
    /// The field at a byte offset of the player's context, as its code reads it.
    fn at(&self, offset: usize) -> u32 {
        u32::from(self.0[offset / 4 - 4])
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Decodes an SMFMF block the way the player does; None where the player would
/// reject it.
pub fn decode(blob: &[u8]) -> Option<SensMe> {
    let n = blob.len();
    if n < CHUNK_HEADER {
        return None;
    }
    let mut w = [MISSING; FEATURES];
    let mut chorus = 0u32;
    let mut bpm = None;
    let mut pos = 0;
    while n - pos >= CHUNK_HEADER {
        let len = be32(&blob[pos + 16..]) as usize;
        if len > n - pos - CHUNK_HEADER {
            return None;
        }
        let d = &blob[pos + CHUNK_HEADER..pos + CHUNK_HEADER + len];
        match &blob[pos..pos + 4] {
            b"STBF" => {
                if len != 36 {
                    return None;
                }
                w[50..].copy_from_slice(d);
            }
            b"STSA" if len >= 4 => {
                let v = be32(d);
                if v < 0x8000_0000 {
                    chorus = v;
                }
            }
            b"GBPM" => {
                let f = f32::from_be_bytes(d.get(..4)?.try_into().ok()?);
                if f < 0.0 {
                    return None;
                }
                bpm = Some(f).filter(|f| f.is_finite());
            }
            b"STMO" => {
                if !len.is_multiple_of(16) {
                    return None;
                }
                for (k, rec) in d.as_chunks::<16>().0.iter().take(10).enumerate() {
                    w[k] = MISSING;
                    if rec[2] != 0 {
                        return None;
                    }
                    let id = u16::from_be_bytes([rec[0], rec[1]]);
                    if (0x100..=0x109).contains(&id) {
                        w[usize::from(id - 0x100)] = rec[3];
                    }
                }
            }
            b"STHF" => {
                if !len.is_multiple_of(4) {
                    return None;
                }
                for rec in d.as_chunks::<4>().0.iter().take(40) {
                    let id = u16::from_be_bytes([rec[0], rec[1]]);
                    if (0x800..=0x827).contains(&id) {
                        w[10 + usize::from(STHF_SLOT[usize::from(id - 0x800)])] = rec[2];
                    }
                }
            }
            _ => {}
        }
        pos += CHUNK_HEADER + len;
    }
    Some(SensMe {
        bpm,
        chorus_ms: Some(chorus).filter(|c| *c > 0),
        features: w.iter().map(|b| format!("{b:02x}")).collect(),
        channels: None,
    })
}

fn context(features: &str) -> Option<Context> {
    if features.len() != FEATURES * 2 || !features.is_ascii() {
        return None;
    }
    let mut w = [0u8; FEATURES];
    for (i, b) in w.iter_mut().enumerate() {
        *b = u8::from_str_radix(&features[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(Context(w))
}

/// What the player decides channels with, as read from the user's firmware: the 48 x 16
/// centroids, each cluster's time of day (channel 0..=4) and fallback mood (channel 5 + v
/// for v < 8), the cluster inputs' weights, and the mood tests' thresholds in [`LIMITS`]
/// order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    centroids: Vec<[f64; 16]>,
    #[serde(default)]
    class: Vec<u8>,
    #[serde(default)]
    fallback: Vec<u8>,
    #[serde(default)]
    weights: Vec<f64>,
    #[serde(default)]
    limits: Vec<u32>,
}

/// The firmware's STHF slot table as it stores it: record id, slot, as little-endian u32.
fn slot_bytes() -> Vec<u8> {
    STHF_SLOT
        .iter()
        .enumerate()
        .flat_map(|(i, s)| [(0x800 + i as u32).to_le_bytes(), u32::from(*s).to_le_bytes()].concat())
        .collect()
}

/// The firmware's three per-cluster tables: centroids, time of day, fallback mood.
type Tables = (Vec<[f64; 16]>, Vec<u8>, Vec<u8>);

/// Finds the tables by the STHF slot table, which is the block's layout and sits between
/// them: the centroids end where the time-of-day table starts, 0x118 bytes before the
/// slots, and the fallback moods start 0x260 bytes after it.
fn tables(data: &[u8]) -> Option<Tables> {
    let slots = slot_bytes();
    let at = data.windows(slots.len()).position(|w| w == slots.as_slice())?;
    let class_at = at.checked_sub(0x118)?;
    let start = class_at.checked_sub(CLUSTERS * 16 * 8)?;
    let small = |from: usize| -> Option<Vec<u8>> {
        data.get(from..from + CLUSTERS * 4)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| u8::try_from(u32::from_le_bytes(*w)).ok())
            .collect()
    };
    let centroids = data[start..class_at]
        .as_chunks::<{ 16 * 8 }>()
        .0
        .iter()
        .map(|row| {
            let cells = row.as_chunks::<8>().0;
            std::array::from_fn(|k| f64::from_le_bytes(cells[k]))
        })
        .collect();
    Some((centroids, small(class_at)?, small(class_at + 0x260)?))
}

/// Reads the weights and the thresholds out of the clustering and mood-test code.
fn code(data: &[u8]) -> Option<(Vec<f64>, Vec<u32>)> {
    let clustering = CLUSTERING.find(data)?;
    let tests = MOOD_TESTS.find(data)?;
    let weights = CLUSTER_INPUTS
        .iter()
        .map(|(_, at)| at.map_or(Some(1.0), |at| vldr_literal(data, clustering + at)))
        .collect::<Option<_>>()?;
    let limits = LIMITS
        .iter()
        .map(|at| immediate(data, tests + usize::from(*at)))
        .collect::<Option<_>>()?;
    Some((weights, limits))
}

impl Table {
    /// Reads the firmware's `libMediaStoreService.so`, or a JSON copy written by
    /// [`Table::to_json`].
    pub fn from_bytes(data: &[u8]) -> Result<Table> {
        if data.first() == Some(&b'{') {
            let t: Table = serde_json::from_slice(data).map_err(|e| {
                Error::unsupported(Why::NotSensmeTable, format!("not a SensMe table: {e}")).with_detail(e.to_string())
            })?;
            return t.checked();
        }
        let (centroids, class, fallback) = tables(data).ok_or_else(|| {
            Error::unsupported(
                Why::NoSensmeTable,
                "this file has no SensMe table; choose libMediaStoreService.so from the Walkman firmware",
            )
        })?;
        let (weights, limits) = code(data).ok_or_else(|| {
            Error::unsupported(
                Why::SensmeTableVersion,
                "this firmware's SensMe code differs from the builds this program knows (NW-A50 1.02)",
            )
        })?;
        Table {
            centroids,
            class,
            fallback,
            weights,
            limits,
        }
        .checked()
    }

    pub fn load(path: &Path) -> Result<Table> {
        Table::from_bytes(&std::fs::read(path)?)
    }

    fn checked(self) -> Result<Table> {
        if self.class.is_empty() && self.weights.is_empty() && self.limits.is_empty() {
            return Err(Error::unsupported(
                Why::SensmeTableOld,
                "this SensMe table was saved by an older version; choose libMediaStoreService.so again",
            ));
        }
        let complete = self.centroids.len() == CLUSTERS
            && self.class.len() == CLUSTERS
            && self.fallback.len() == CLUSTERS
            && self.weights.len() == CLUSTER_INPUTS.len()
            && self.limits.len() == LIMITS.len();
        let sane = self.class.iter().all(|c| *c < 5)
            && self.fallback.iter().all(|v| *v < 10)
            && self
                .centroids
                .iter()
                .flatten()
                .chain(&self.weights)
                .all(|v| v.is_finite());
        if !complete || !sane {
            return Err(Error::unsupported(
                Why::SensmeTableDamaged,
                "the SensMe table is damaged",
            ));
        }
        Ok(self)
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// The threshold the mood-test code holds at offset `at`.
    fn limit(&self, at: u16) -> u32 {
        LIMITS.binary_search(&at).map_or(u32::MAX, |i| self.limits[i])
    }

    /// The nearest centroid, 1..=48, or None when an input is missing.
    fn cluster(&self, ctx: &Context) -> Option<usize> {
        let mut vals = [0f64; 16];
        for ((v, (off, _)), weight) in vals.iter_mut().zip(CLUSTER_INPUTS).zip(&self.weights) {
            let x = ctx.at(off);
            if x == u32::from(MISSING) {
                return None;
            }
            *v = weight * f64::from(x);
        }
        let mut best: Option<(f64, usize)> = None;
        for (i, row) in self.centroids.iter().enumerate() {
            let mut d = (vals[0] - row[0]).abs();
            for k in 1..16 {
                d += (vals[k] - row[k]).abs();
            }
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, i + 1));
            }
        }
        best.map(|(_, c)| c)
    }

    /// The channels the player puts a track with these features in, ascending.
    pub fn channels(&self, features: &str) -> Option<Vec<u8>> {
        let ctx = context(features)?;
        let Some(c) = self.cluster(&ctx) else {
            return Some(Vec::new());
        };
        let class = self.class[c - 1];
        let fallback = self.fallback[c - 1];
        let f = |off| ctx.at(off);
        let l = |at| self.limit(at);
        let missing = u32::from(MISSING);
        if PRED_FIELDS.iter().any(|o| f(*o) == missing) {
            // STMO and STHF both absent leave every mood test undecided; any other mix
            // takes the firmware's three-valued logic, which is not ported.
            if !ctx.0[..50].iter().all(|v| *v == MISSING) {
                return None;
            }
            if f(0x138) == 0 || c as u32 == l(0x3C) {
                return Some(Vec::new());
            }
            let mut out = vec![class];
            if fallback < 8 {
                out.push(5 + fallback);
            }
            return Some(out);
        }
        let Some(moods) = moods(&f, c, &l) else {
            return Some(Vec::new());
        };
        let mut out = vec![class];
        out.extend((0..8u8).filter(|k| moods[usize::from(*k)]).map(|k| 5 + k));
        if out.len() == 1 && fallback < 8 {
            out.push(5 + fallback);
        }
        out.sort_unstable();
        Some(out)
    }
}

/// The eight mood tests, channels 5..=12, or None when the gate turns every channel off.
/// `l(at)` is the threshold at offset `at` of the firmware's mood-test code; a range is a
/// subtract and an unsigned compare there.
fn moods(f: &impl Fn(usize) -> u32, c: usize, l: &impl Fn(u16) -> u32) -> Option<[bool; 8]> {
    let c = c as u32;
    let gated = f(0x138) == 0
        || c == l(0x3C)
        || (!(f(0x60) > l(0x52) || f(0x80) > l(0x56)) && f(0x88) >= l(0x5A))
        || (f(0x50) >= l(0x70) && (f(0xDC) > l(0x78) || f(0x88) >= l(0x7C)));
    if gated {
        return None;
    }
    let dc = f(0xDC);
    let p7 = f(0xD0) > l(0x166) && f(0x140) > l(0x15A);
    let p2 = f(0x18) > l(0xE8) && dc < l(0xDE) && f(0xE8) < l(0xF6) && f(0x6C) < l(0x104);
    let p4 = c.wrapping_sub(l(0x122)) < l(0x124)
        || (f(0x138) > l(0x236) && f(0x4C) > l(0x228) && f(0x2C) < l(0x21E) && dc < l(0x244));
    let p6 = !p7 && (c == l(0x26C) || (f(0x4C) <= l(0x302) && (f(0x60) > l(0x310) || f(0xD4) > l(0x306))));
    let p1 = !p2
        && ((f(0x4C) > l(0x346) && dc < l(0x33C) && f(0x18) < l(0x354))
            || (f(0x138) < l(0x1DA) && f(0x10) > l(0x1CC) && f(0xE4) > l(0x1C2))
            || (f(0x2C) > l(0x2AA) && f(0x68) < l(0x29E)));
    let p0 = !p7 && !p6 && {
        let t = f(0x74) > l(0x5D0) && dc > l(0x5C2) && f(0x13C) > l(0x5B8);
        t || (f(0x20) >= l(0x56A)
            && dc >= l(0x56E)
            && f(0x13C) >= l(0x572)
            && f(0x4C) <= l(0x576)
            && (f(0x28) > l(0x594) || f(0xE0) > l(0x586) || f(0x14) > l(0x57C)))
    };
    let p3 = !(p7 || p4 || p1 || p2)
        && ((f(0x138) > l(0x532) && dc < l(0x51A) && f(0x24) > l(0x524) && f(0x6C) > l(0x540) && f(0x80) < l(0x54E))
            || (f(0x144) > l(0x47C)
                && f(0x24) > l(0x422)
                && f(0x18) < l(0x418)
                && dc.wrapping_sub(l(0x428)) < l(0x430)
                && f(0xE8) > l(0x43E)
                && f(0xD0) < l(0x44C)
                && f(0x70) < l(0x45A)
                && f(0x14C) < l(0x46A)));
    let p5 = c != l(0x12C) && {
        let x =
            f(0x60) < l(0x644) && (f(0xCC) > l(0x61A) || f(0x84) > l(0x610)) && dc < l(0x628) && f(0x13C) < l(0x636);
        x || ((f(0xCC) > l(0x4D6) || f(0x5C) > l(0x4E0))
            && f(0x48) >= l(0x4F2)
            && f(0x64) <= l(0x4F8)
            && f(0x50) <= l(0x4FE)
            && (f(0xC0) > l(0x502) || f(0xA8) > l(0x5E0)))
    };
    Some([p0, p1, p2, p3, p4, p5, p6, p7])
}

/// Fills in each analysed track's channels, or clears them without a table.
pub fn classify<'a>(analyses: impl IntoIterator<Item = &'a mut SensMe>, table: Option<&Table>) {
    for s in analyses {
        s.channels = table.and_then(|t| t.channels(&s.features));
    }
}

fn syncsafe(b: &[u8]) -> usize {
    (usize::from(b[0] & 0x7f) << 21)
        | (usize::from(b[1] & 0x7f) << 14)
        | (usize::from(b[2] & 0x7f) << 7)
        | usize::from(b[3] & 0x7f)
}

fn unsync(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        out.push(b[i]);
        i += if b[i] == 0xff && b.get(i + 1) == Some(&0) { 2 } else { 1 };
    }
    out
}

/// Skips a NUL-terminated string (a double NUL on a UTF-16 boundary when `wide`).
fn after_text(b: &[u8], pos: usize, wide: bool) -> usize {
    if wide {
        let mut i = pos;
        while i + 1 < b.len() && !(b[i] == 0 && b[i + 1] == 0) {
            i += 2;
        }
        (i + 2).min(b.len())
    } else {
        b[pos.min(b.len())..]
            .iter()
            .position(|c| *c == 0)
            .map_or(b.len(), |i| pos + i + 1)
    }
}

/// An ID3v2 tag's frames as (id, body).
pub(super) type Id3Frames = Vec<(Vec<u8>, Vec<u8>)>;

/// The major version and frames of the ID3v2 tag that starts at `data[0]`, with
/// unsynchronisation undone. Ids are three bytes in ID3v2.2 and four otherwise.
pub(super) fn id3_frames(data: &[u8]) -> Option<(u8, Id3Frames)> {
    if data.len() < 10 || &data[..3] != b"ID3" {
        return None;
    }
    let (major, flags) = (data[3], data[5]);
    let size = syncsafe(&data[6..10]);
    let raw = &data[10..(10 + size).min(data.len())];
    let tag = if flags & 0x80 != 0 && major < 4 {
        unsync(raw)
    } else {
        raw.to_vec()
    };
    let mut pos = 0;
    if flags & 0x40 != 0 && tag.len() >= 4 {
        pos = match major {
            3 => 4 + be32(&tag) as usize,
            4 => syncsafe(&tag),
            _ => 0,
        };
    }
    let (id_len, header) = if major == 2 { (3, 6) } else { (4, 10) };
    let mut frames = Vec::new();
    while pos + header <= tag.len() {
        let id = &tag[pos..pos + id_len];
        if id[0] == 0 {
            break;
        }
        let size = match major {
            2 => (usize::from(tag[pos + 3]) << 16) | (usize::from(tag[pos + 4]) << 8) | usize::from(tag[pos + 5]),
            4 => syncsafe(&tag[pos + 4..]),
            _ => be32(&tag[pos + 4..]) as usize,
        };
        let body = &tag[(pos + header).min(tag.len())..(pos + header).saturating_add(size).min(tag.len())];
        let body = if major == 4 && flags & 0x80 != 0 {
            unsync(body)
        } else {
            body.to_vec()
        };
        frames.push((id.to_vec(), body));
        pos += header + size;
    }
    Some((major, frames))
}

/// Whether a `GEOB` body holds analysis: its MIME type is `application/smfmf` in any case.
pub(super) fn geob_is_smfmf(body: &[u8]) -> bool {
    !body.is_empty()
        && body[1..after_text(body, 1, false).saturating_sub(1).max(1)].eq_ignore_ascii_case(b"application/smfmf")
}

/// The SMFMF block in an ID3v2 tag that starts at `data[0]`: the first `GEOB` frame
/// whose MIME type is `application/smfmf` in any case. The description does not matter
/// to the player.
pub fn from_id3(data: &[u8]) -> Option<Vec<u8>> {
    let (_, frames) = id3_frames(data)?;
    frames.into_iter().find_map(|(id, body)| {
        if (id != b"GEOB" && id != b"GEO") || !geob_is_smfmf(&body) {
            return None;
        }
        let wide = matches!(body[0], 1 | 2);
        let p = after_text(&body, after_text(&body, after_text(&body, 1, false), wide), wide);
        Some(body[p..].to_vec())
    })
}

/// The body of a FLAC `APPLICATION` block with the id `SMFM`, without the id.
pub fn from_flac_block(body: &[u8]) -> Option<Vec<u8>> {
    body.strip_prefix(b"SMFM").map(<[u8]>::to_vec)
}

fn read_exact_at(f: &mut File, at: u64, len: u64) -> Option<Vec<u8>> {
    if len > MAX_TAG {
        return None;
    }
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut buf = vec![0; len as usize];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn flac_blob(f: &mut File) -> Option<Vec<u8>> {
    let mut pos = 4u64;
    loop {
        let h = read_exact_at(f, pos, 4)?;
        let len = (u64::from(h[1]) << 16) | (u64::from(h[2]) << 8) | u64::from(h[3]);
        if h[0] & 0x7f == 2 && len >= 4 && read_exact_at(f, pos + 4, 4)? == b"SMFM" {
            return read_exact_at(f, pos + 8, len - 4);
        }
        if h[0] & 0x80 != 0 {
            return None;
        }
        pos += 4 + len;
    }
}

/// Child boxes of an MP4 box body: (type, body start, end).
fn mp4_children(data: &[u8], start: usize, end: usize) -> Vec<([u8; 4], usize, usize)> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos + 8 <= end {
        let mut size = be32(&data[pos..]) as usize;
        let kind: [u8; 4] = data[pos + 4..pos + 8].try_into().unwrap();
        let mut header = 8;
        if size == 1 {
            let Some(b) = data.get(pos + 8..pos + 16) else { break };
            size = u64::from_be_bytes(b.try_into().unwrap()) as usize;
            header = 16;
        } else if size == 0 {
            size = end - pos;
        }
        if size < header || pos + size > end {
            break;
        }
        out.push((kind, pos + header, pos + size));
        pos += size;
    }
    out
}

/// The ID3v2 tag the player reads from `moov/meta/ID32`. No analysed MP4 was at hand,
/// so the layout inside `ID32` follows the ISO form, not a sample.
fn mp4_blob(f: &mut File) -> Option<Vec<u8>> {
    let len = f.metadata().ok()?.len();
    let mut pos = 0u64;
    while pos + 8 <= len {
        let h = read_exact_at(f, pos, 16.min(len - pos))?;
        let mut size = u64::from(be32(&h));
        let mut header = 8;
        if size == 1 {
            size = u64::from_be_bytes(h.get(8..16)?.try_into().ok()?);
            header = 16;
        } else if size == 0 {
            size = len - pos;
        }
        if size < header {
            return None;
        }
        if &h[4..8] == b"moov" {
            let moov = read_exact_at(f, pos + header, size - header)?;
            for (kind, s, e) in mp4_children(&moov, 0, moov.len()) {
                if &kind != b"meta" {
                    continue;
                }
                for skip in [4, 0] {
                    for (kind, s, e) in mp4_children(&moov, s + skip, e) {
                        if &kind == b"ID32" {
                            let body = &moov[s..e];
                            let tag = if body.get(6..9) == Some(b"ID3") {
                                &body[6..]
                            } else {
                                body
                            };
                            return from_id3(tag);
                        }
                    }
                }
            }
            return None;
        }
        pos += size;
    }
    None
}

/// The SMFMF block in a music file: MP3 (ID3v2 at the start), FLAC or MP4.
pub fn extract(path: &Path) -> Option<Vec<u8>> {
    let mut f = File::open(path).ok()?;
    let mut head = [0u8; 10];
    f.read_exact(&mut head).ok()?;
    if &head[..3] == b"ID3" {
        let tag = read_exact_at(&mut f, 0, 10 + syncsafe(&head[6..10]) as u64)?;
        return from_id3(&tag);
    }
    if &head[..4] == b"fLaC" {
        return flac_blob(&mut f);
    }
    if &head[4..8] == b"ftyp" {
        return mp4_blob(&mut f);
    }
    None
}

/// The analysis in a music file, if it has any the player would accept.
pub fn read(path: &Path) -> Option<SensMe> {
    decode(&extract(path)?)
}

/// What the player shell prints for each file, in order: `@@<n>`, then `I<len>` and the
/// ID3 tag, or `F<len>` and the FLAC block, when the file may hold analysis data.
/// Walkman OS has busybox in /sbin, Android toybox; both take these forms. mksh
/// predefines `r` as an alias, so the function has a longer name.
const PLAYER_SCRIPT: &str = r#"if [ -x /sbin/busybox ]; then b=/sbin/busybox; else b=; fi
uwm_smfmf() {
i=$1; f=$2; z=$3
echo "@@$i"
set -- $($b head -c 10 "$f" 2>/dev/null | $b od -An -tu1)
if [ "$1" = 73 ] && [ "$2" = 68 ] && [ "$3" = 51 ] && [ $# -ge 10 ]; then
s=$(( (($7 & 127) << 21) | (($8 & 127) << 14) | (($9 & 127) << 7) | (${10} & 127) ))
s=$((s + 10))
[ "$s" -le "$z" ] || return
echo "I$s"; $b head -c "$s" "$f"
elif [ "$1" = 102 ] && [ "$2" = 76 ] && [ "$3" = 97 ] && [ "$4" = 67 ]; then
p=4
while [ "$p" -lt "$z" ]; do
set -- $($b tail -c +$((p + 1)) "$f" | $b head -c 8 | $b od -An -tu1)
[ $# -ge 8 ] || return
l=$(( ($2 << 16) | ($3 << 8) | $4 ))
if [ $(( $1 & 127 )) = 2 ] && [ "$5" = 83 ] && [ "$6" = 77 ] && [ "$7" = 70 ] && [ "$8" = 77 ] && [ $((p + 4 + l)) -le "$z" ]; then
echo "F$l"; $b tail -c +$((p + 5)) "$f" | $b head -c "$l"
return
fi
[ $(( $1 & 128 )) = 0 ] || return
p=$((p + 4 + l))
done
fi
}
"#;

/// Files read per round trip; small enough that stopping takes a few seconds.
const BATCH: usize = 200;

/// One file to read on a player.
pub struct PlayerFile<'a> {
    pub path: &'a str,
    pub size: u64,
}

fn line(out: &[u8], pos: &mut usize) -> Option<String> {
    let end = out[*pos..].iter().position(|c| *c == b'\n')? + *pos;
    let text = String::from_utf8(out[*pos..end].to_vec()).ok();
    *pos = end + 1;
    text
}

/// Reads one batch's output: what each file the player answered for holds, in order.
/// A file the output breaks off in counts as holding nothing.
fn parse_batch(out: &[u8]) -> Vec<Option<SensMe>> {
    let mut found = Vec::new();
    let mut pos = 0;
    while pos < out.len() {
        let Some(n) = line(out, &mut pos).and_then(|l| l.strip_prefix("@@")?.parse::<usize>().ok()) else {
            break;
        };
        if n != found.len() {
            break;
        }
        found.push(None);
        if !matches!(out.get(pos), Some(b'I' | b'F')) {
            continue;
        }
        let kind = out[pos];
        let Some(len) = line(out, &mut pos).and_then(|l| l[1..].parse::<usize>().ok()) else {
            break;
        };
        let Some(body) = out.get(pos..pos + len) else {
            break;
        };
        pos += len;
        let blob = if kind == b'I' {
            from_id3(body)
        } else {
            from_flac_block(body)
        };
        *found.last_mut().unwrap() = blob.and_then(|b| decode(&b));
    }
    found
}

/// The first ten bytes of every file, in one process. None when the player's `head`
/// cannot do it or a file did not answer, so the caller reads file by file.
fn heads(adb: &Adb, serial: &str, files: &[PlayerFile]) -> Option<Vec<[u8; 10]>> {
    let mut script = "if [ -x /sbin/busybox ]; then b=/sbin/busybox; else b=; fi; $b head -q -c 10".to_owned();
    for f in files {
        script.push(' ');
        script.push_str(&shell_quote(f.path));
    }
    let out = adb.exec_out(serial, &script, Duration::from_secs(60)).ok()?;
    (out.len() == files.len() * 10).then(|| out.as_chunks::<10>().0.to_vec())
}

/// The shell work for a batch whose headers are known: ID3 tags are copied out whole.
/// FLAC files answer nothing here; [`flac_windows`] walks them afterwards.
fn known_heads_script(files: &[PlayerFile], heads: &[[u8; 10]]) -> String {
    let mut script = PLAYER_SCRIPT.to_owned();
    for (i, (f, h)) in files.iter().zip(heads).enumerate() {
        if &h[..3] == b"ID3" {
            let len = 10 + syncsafe(&h[6..10]) as u64;
            if len <= f.size.min(MAX_TAG) {
                let path = shell_quote(f.path);
                script.push_str(&format!("echo @@{i}; echo I{len}; $b head -c {len} {path}\n"));
                continue;
            }
        }
        script.push_str(&format!("echo @@{i}\n"));
    }
    script
}

/// FLAC metadata is read from the computer a window at a time; most files need one,
/// a large picture block costs one more.
const WINDOW: u64 = 64 << 10;
const FLAC_ROUNDS: usize = 12;

#[derive(Debug, PartialEq)]
enum Walk {
    /// Read `len` bytes at `at` next.
    Need {
        at: u64,
        len: u64,
    },
    Found(Vec<u8>),
    Nothing,
}

/// Walks FLAC metadata blocks through `window`, which starts at file offset `at`.
fn flac_step(window: &[u8], at: u64, size: u64) -> Walk {
    let need = |p: u64, least: u64| Walk::Need {
        at: p,
        len: WINDOW.max(least).min(size.saturating_sub(p)),
    };
    // The first window starts at the `fLaC` mark.
    let mut p = at.max(4);
    loop {
        let rel = (p - at) as usize;
        let Some(h) = window.get(rel..rel + 4) else {
            return need(p, 8);
        };
        let len = (u64::from(h[1]) << 16) | (u64::from(h[2]) << 8) | u64::from(h[3]);
        if h[0] & 0x7f == 2 && len >= 4 {
            let Some(id) = window.get(rel + 4..rel + 8) else {
                return if p == at { Walk::Nothing } else { need(p, 8) };
            };
            if id == b"SMFM" {
                if len > MAX_TAG {
                    return Walk::Nothing;
                }
                return match window.get(rel + 8..rel + 4 + len as usize) {
                    Some(body) => Walk::Found(body.to_vec()),
                    None if p == at && window.len() as u64 >= 4 + len => Walk::Nothing,
                    None => need(p, 4 + len),
                };
            }
        }
        p += 4 + len;
        if h[0] & 0x80 != 0 || p + 4 > size {
            return Walk::Nothing;
        }
    }
}

/// Reads `@@<n>`, `W<len>` and the bytes, for each file asked.
fn parse_windows(out: &[u8]) -> Vec<(usize, Vec<u8>)> {
    let mut found = Vec::new();
    let mut pos = 0;
    while pos < out.len() {
        let Some(n) = line(out, &mut pos).and_then(|l| l.strip_prefix("@@")?.parse::<usize>().ok()) else {
            break;
        };
        let Some(len) = line(out, &mut pos).and_then(|l| l.strip_prefix('W')?.parse::<usize>().ok()) else {
            break;
        };
        let Some(body) = out.get(pos..pos + len) else {
            break;
        };
        pos += len;
        found.push((n, body.to_vec()));
    }
    found
}

/// Fills in the FLAC files of a batch, one round trip per window depth.
fn flac_windows(
    adb: &Adb,
    serial: &str,
    files: &[PlayerFile],
    todo: Vec<usize>,
    found: &mut [Option<SensMe>],
) -> Result<()> {
    let mut asks: Vec<(usize, u64, u64)> = todo.into_iter().map(|i| (i, 0, WINDOW.min(files[i].size))).collect();
    for _ in 0..FLAC_ROUNDS {
        if asks.is_empty() {
            break;
        }
        let mut script = "if [ -x /sbin/busybox ]; then b=/sbin/busybox; else b=; fi\n".to_owned();
        for (i, at, len) in &asks {
            let path = shell_quote(files[*i].path);
            let read = if *at == 0 {
                format!("$b head -c {len} {path}")
            } else {
                format!("$b tail -c +{} {path} | $b head -c {len}", at + 1)
            };
            script.push_str(&format!("echo @@{i}; echo W{len}; {read}\n"));
        }
        let bytes = adb.exec_out(serial, &script, Duration::from_secs(120))?;
        let windows = parse_windows(&bytes);
        let mut next = Vec::new();
        for (i, at, len) in asks {
            let Some((_, w)) = windows.iter().find(|(n, w)| *n == i && w.len() as u64 == len) else {
                continue;
            };
            match flac_step(w, at, files[i].size) {
                Walk::Found(blob) => found[i] = decode(&blob),
                Walk::Need { at: p, len } if p > at || len > w.len() as u64 => next.push((i, p, len)),
                _ => {}
            }
        }
        asks = next;
    }
    Ok(())
}

fn each_script(files: &[PlayerFile]) -> String {
    let mut script = PLAYER_SCRIPT.to_owned();
    for (i, f) in files.iter().enumerate() {
        script.push_str(&format!("uwm_smfmf {i} {} {}\n", shell_quote(f.path), f.size));
    }
    script
}

/// Reads the analysis of files on a player, in the order given, a batch at a time.
/// Cancelling returns what was read so far. Older adbd takes commands of about 4 KB,
/// so Walkman OS gets small batches.
pub fn read_player(
    adb: &Adb,
    serial: &str,
    files: &[PlayerFile],
    small_batches: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize),
) -> Result<Vec<Option<SensMe>>> {
    let limit = if small_batches { 3800 } else { 60_000 };
    let mut out = Vec::with_capacity(files.len());
    while out.len() < files.len() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let start = out.len();
        // Each file costs its quoted path and some shell around it, in either form.
        let mut end = start;
        let mut length = PLAYER_SCRIPT.len();
        while end < files.len() && end - start < BATCH {
            let cost = files[end].path.len() + 64;
            if end > start && length + cost > limit {
                break;
            }
            length += cost;
            end += 1;
        }
        let batch = &files[start..end];
        let heads = heads(adb, serial, batch);
        let script = match &heads {
            Some(h) => known_heads_script(batch, h),
            None => each_script(batch),
        };
        let bytes = adb.exec_out(serial, &script, Duration::from_secs(120))?;
        // A file the shell broke off in is skipped, so one bad file cannot stall the rest;
        // a shell that never reached the first file is an error.
        let found = parse_batch(&bytes);
        if found.is_empty() {
            let said = String::from_utf8_lossy(&bytes[..bytes.len().min(300)])
                .trim()
                .to_owned();
            return Err(Error::Failed {
                tool: "adb",
                detail: if said.is_empty() {
                    "the player printed nothing".to_owned()
                } else {
                    said
                },
            });
        }
        let mut found = found;
        if let Some(h) = &heads {
            let flac = (0..found.len()).filter(|i| &h[*i][..4] == b"fLaC").collect();
            flac_windows(adb, serial, batch, flac, &mut found)?;
        }
        let taken = found.len().clamp(1, end - start);
        out.extend(found.into_iter().take(taken));
        out.resize(start + taken, None);
        progress(out.len());
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn chunk(tag: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut c = tag.to_vec();
        c.extend_from_slice(b"STAEMMLW\x01\x00\x80\x00");
        c.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        c.extend_from_slice(payload);
        c
    }

    /// A blob laid out like Sony's: STMO as four records per id, STHF by record id.
    pub fn blob(stbf: &[u8; 36], sthf: &[u8; 40], stmo: &[u8; 10], bpm: f32, chorus: u32) -> Vec<u8> {
        let mut b = chunk(b"GBPM", &bpm.to_be_bytes());
        b.extend(chunk(b"STBF", stbf));
        b.extend(chunk(
            b"STSA",
            &[chorus.to_be_bytes(), chorus.to_be_bytes(), 0u32.to_be_bytes()].concat(),
        ));
        let mo: Vec<u8> = (0..10u16)
            .flat_map(|i| {
                (0..4u8).flat_map(move |j| [((0x100 + i) >> 8) as u8, (0x100 + i) as u8, j, stmo[usize::from(i)]])
            })
            .collect();
        b.extend(chunk(b"STMO", &mo));
        let hf: Vec<u8> = (0..40u16)
            .flat_map(|i| {
                let slot = usize::from(STHF_SLOT[usize::from(i)]);
                [((0x800 + i) >> 8) as u8, (0x800 + i) as u8, sthf[slot], 0]
            })
            .collect();
        b.extend(chunk(b"STHF", &hf));
        b.extend(chunk(b"STMM", &[0; 8]));
        b
    }

    /// A made-up table: row c is cluster c + 1's own weighted inputs, so `stbf_for(c + 1)`
    /// falls in it; time of day c % 5, fallback mood c % 10, every threshold 50.
    fn table() -> Table {
        let weights: Vec<f64> = CLUSTER_INPUTS
            .iter()
            .enumerate()
            .map(|(k, (_, at))| if at.is_some() { 0.5 + k as f64 / 8.0 } else { 1.0 })
            .collect();
        Table {
            centroids: (0..CLUSTERS)
                .map(|c| std::array::from_fn(|k| weights[k] * (10 + 3 * c + k) as f64))
                .collect(),
            class: (0..CLUSTERS).map(|c| (c % 5) as u8).collect(),
            fallback: (0..CLUSTERS).map(|c| (c % 10) as u8).collect(),
            weights,
            limits: vec![50; LIMITS.len()],
        }
    }

    fn set(t: &mut Table, at: u16, v: u32) {
        t.limits[LIMITS.binary_search(&at).unwrap()] = v;
    }

    fn stbf_for(cluster: usize) -> [u8; 36] {
        let mut stbf = [50u8; 36];
        stbf[28] = MISSING;
        for (k, (off, _)) in CLUSTER_INPUTS.iter().enumerate() {
            stbf[off / 4 - 54] = (10 + 3 * (cluster - 1) + k) as u8;
        }
        stbf
    }

    /// The firmware's layout around the slot table: centroids, time of day, slots, fallback.
    fn library(t: &Table) -> Vec<u8> {
        let mut lib = vec![0x7fu8; 1000];
        for v in t.centroids.iter().flatten() {
            lib.extend_from_slice(&v.to_le_bytes());
        }
        let class_at = lib.len();
        lib.extend(t.class.iter().flat_map(|c| u32::from(*c).to_le_bytes()));
        lib.resize(class_at + 0x118, 0);
        lib.extend(slot_bytes());
        lib.resize(class_at + 0x260, 0);
        lib.extend(t.fallback.iter().flat_map(|v| u32::from(*v).to_le_bytes()));
        lib.extend([0x7fu8; 100]);
        lib
    }

    fn why(r: Result<Table>) -> Option<Why> {
        match r {
            Err(Error::Unsupported { why, .. }) => Some(why),
            _ => None,
        }
    }

    #[test]
    fn decodes_the_sample_layout() {
        let mut stbf = stbf_for(8);
        stbf[1] = 97;
        let s = decode(&blob(&stbf, &[50; 40], &[50; 10], 97.6282, 148_332)).unwrap();
        assert_eq!(s.chorus_ms, Some(148_332));
        assert!((s.bpm.unwrap() - 97.6282).abs() < 1e-4);
        let ctx = context(&s.features).unwrap();
        assert_eq!(ctx.at(0xDC), 97);
        assert_eq!(ctx.at(0x10), 50);
        assert_eq!(ctx.at(0x38 + 4 * 15), 50);
    }

    #[test]
    fn rejects_what_the_player_rejects() {
        let good = blob(&stbf_for(1), &[50; 40], &[50; 10], 120.0, 1000);
        assert!(decode(&good).is_some());
        assert!(decode(&good[..19]).is_none());
        let mut overrun = good.clone();
        overrun[19] = 0xff;
        assert!(decode(&overrun).is_none());
        let short_stbf = chunk(b"STBF", &[1; 35]);
        assert!(decode(&short_stbf).is_none());
        // Fewer than a header's worth of trailing bytes are ignored.
        let mut tail = good.clone();
        tail.extend_from_slice(&[0; 10]);
        assert!(decode(&tail).is_some());
    }

    #[test]
    fn stbf_alone_gives_time_of_day_and_fallback_mood() {
        let mut t = table();
        set(&mut t, 0x3C, 47);
        for c in 1..=CLUSTERS {
            let s = decode(&chunk(b"STBF", &stbf_for(c))).unwrap();
            let ch = t.channels(&s.features).unwrap();
            let mut want = Vec::new();
            if c != 47 {
                want.push(t.class[c - 1]);
                if t.fallback[c - 1] < 8 {
                    want.push(5 + t.fallback[c - 1]);
                }
            }
            assert_eq!(ch, want, "cluster {c}");
        }
    }

    #[test]
    fn mood_tests_and_gate() {
        let mut t = table();
        // No fallback channel for cluster 8, so 12 can only come from the last mood test.
        t.fallback[7] = 9;
        let flat = |stbf: &[u8; 36]| decode(&blob(stbf, &[50; 40], &[50; 10], 97.0, 1)).unwrap().features;
        // Flat 50s against thresholds of 50 meet the gate's third test.
        assert_eq!(t.channels(&flat(&stbf_for(8))), Some(vec![]));
        set(&mut t, 0x5A, 255);
        set(&mut t, 0x70, 255);
        set(&mut t, 0x15A, 0);
        set(&mut t, 0x166, 0);
        assert!(t.channels(&flat(&stbf_for(8))).unwrap().contains(&12));
        set(&mut t, 0x166, 255);
        let ch = t.channels(&flat(&stbf_for(8))).unwrap();
        assert!(!ch.contains(&12) && ch.contains(&t.class[7]), "{ch:?}");
        // B24 == 0 closes the gate, and so does the gated cluster.
        let mut stbf = stbf_for(8);
        stbf[24] = 0;
        assert_eq!(t.channels(&flat(&stbf)), Some(vec![]));
        set(&mut t, 0x3C, 8);
        assert_eq!(t.channels(&flat(&stbf_for(8))), Some(vec![]));
        // Only STHF present is a mix the port does not follow.
        let mut partial = chunk(b"STBF", &stbf_for(8));
        partial.extend(chunk(b"STHF", &[0x08, 0x00, 50, 0]));
        let s = decode(&partial).unwrap();
        assert_eq!(t.channels(&s.features), None);
    }

    #[test]
    fn every_threshold_is_read() {
        let seen = std::cell::RefCell::new(std::collections::BTreeSet::new());
        let mut x = 0x2545_f491u32;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        for _ in 0..200_000 {
            let fs: Vec<u32> = (0..FEATURES).map(|_| 20 + next() % 60).collect();
            let limits: Vec<u32> = (0..LIMITS.len()).map(|_| 20 + next() % 60).collect();
            let c = 1 + next() as usize % CLUSTERS;
            let f = |off: usize| fs[off / 4 - 4];
            let l = |at: u16| {
                seen.borrow_mut().insert(at);
                LIMITS.binary_search(&at).map_or(0, |i| limits[i])
            };
            moods(&f, c, &l);
        }
        assert_eq!(seen.into_inner().into_iter().collect::<Vec<_>>(), LIMITS);
    }

    #[test]
    fn finds_the_tables_in_a_library() {
        let t = table();
        let lib = library(&t);
        let (centroids, class, fallback) = tables(&lib).unwrap();
        assert_eq!(
            (centroids, class, fallback),
            (t.centroids.clone(), t.class.clone(), t.fallback.clone())
        );
        // Tables without the code this program knows are another firmware version.
        assert_eq!(why(Table::from_bytes(&lib)), Some(Why::SensmeTableVersion));
        assert_eq!(
            why(Table::from_bytes(b"\x7fELF nothing here")),
            Some(Why::NoSensmeTable)
        );
        let again = Table::from_bytes(&t.to_json()).unwrap();
        assert_eq!((again.weights, again.limits), (t.weights.clone(), t.limits.clone()));
        let old = serde_json::to_vec(&serde_json::json!({ "centroids": t.centroids })).unwrap();
        assert_eq!(why(Table::from_bytes(&old)), Some(Why::SensmeTableOld));
        let mut short = t.clone();
        short.limits.pop();
        assert_eq!(why(Table::from_bytes(&short.to_json())), Some(Why::SensmeTableDamaged));
    }

    #[test]
    fn decodes_thumb_immediates() {
        let at = |code: &[u16]| -> Vec<u8> { code.iter().flat_map(|h| h.to_le_bytes()).collect() };
        // cmp r0, #0x2b; subs r2, #0x11
        assert_eq!(immediate(&at(&[0x282B]), 0), Some(0x2B));
        assert_eq!(immediate(&at(&[0x3A11]), 0), Some(0x11));
        // cmp.w r3, #0x1fe; subs.w r2, r5, #0x47
        assert_eq!(immediate(&at(&[0xF5B3, 0x7FFF]), 0), Some(0x1FE));
        assert_eq!(immediate(&at(&[0xF1B5, 0x0247]), 0), Some(0x47));
        // bx lr; nop
        assert_eq!(immediate(&at(&[0x4770, 0xBF00]), 0), None);
        assert_eq!(
            [0x0AB, 0x1AB, 0x2AB, 0x3AB, 0xFFF].map(expand_imm),
            [0xAB, 0x00AB_00AB, 0xAB00_AB00, 0xABAB_ABAB, 0x1FE]
        );
    }

    #[test]
    fn reads_vldr_literals() {
        let mut data = vec![0u8; 48];
        // vldr d8, [pc, #8] at 2: pc is 6, aligned down to 4.
        data[2..6].copy_from_slice(&[0x9F, 0xED, 0x02, 0x8B]);
        data[12..20].copy_from_slice(&1.25f64.to_le_bytes());
        assert_eq!(vldr_literal(&data, 2), Some(1.25));
        // vldr d0, [pc, #-16] at 40
        data[40..44].copy_from_slice(&[0x1F, 0xED, 0x04, 0x0B]);
        data[28..36].copy_from_slice(&(-3.5f64).to_le_bytes());
        assert_eq!(vldr_literal(&data, 40), Some(-3.5));
        assert_eq!(vldr_literal(&data, 6), None);
    }

    #[test]
    fn finds_code_by_its_shape() {
        let at = |code: &[u16]| -> Vec<u8> { code.iter().flat_map(|h| h.to_le_bytes()).collect() };
        // push.w {r4-r11, lr}; cmp r0, #5; bne; movw r1, #0x123; bl; mov r1, r2
        let one = at(&[0xE92D, 0x4FF0, 0x2805, 0xD104, 0xF240, 0x1123, 0xF000, 0xF812, 0x4611]);
        let moved = at(&[0xE92D, 0x4FF0, 0x2809, 0xD108, 0xF240, 0x2134, 0xF7FF, 0xFF80, 0x4611]);
        let other = at(&[0xE92D, 0x4FF0, 0x2805, 0xD104, 0xF240, 0x1123, 0xF000, 0xF812, 0x4613]);
        assert_eq!(shape(&one), shape(&moved));
        assert_ne!(shape(&one), shape(&other));
        let code = Code {
            prologue: [0x2D, 0xE9, 0xF0, 0x4F],
            len: one.len(),
            shape: format!("{:x}", Sha256::digest(shape(&one))).leak(),
        };
        let mut data = vec![0u8; 64];
        data.extend(&other);
        data.extend(&moved);
        data.extend([0u8; 8]);
        assert_eq!(code.find(&data), Some(64 + other.len()));
        assert_eq!(code.find(&data[..64 + other.len() + 4]), None);
    }

    /// The real library, when one is at hand: `UWM_SENSME_LIB=<libMediaStoreService.so>`.
    #[test]
    fn reads_a_real_library() {
        let Some(path) = std::env::var_os("UWM_SENSME_LIB") else {
            return;
        };
        let t = Table::load(Path::new(&path)).unwrap();
        let s = decode(&blob(&stbf_for(8), &[50; 40], &[50; 10], 97.0, 1)).unwrap();
        assert!(t.channels(&s.features).is_some());
    }

    fn geob(blob: &[u8], mime: &[u8]) -> Vec<u8> {
        let mut body = vec![0u8];
        body.extend_from_slice(mime);
        body.extend_from_slice(b"\0\0USR_SMFMF\0");
        body.extend_from_slice(blob);
        body
    }

    pub fn id3_with(frames: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut tag = Vec::new();
        for (id, body) in frames {
            tag.extend_from_slice(*id);
            tag.extend_from_slice(&(body.len() as u32).to_be_bytes());
            tag.extend_from_slice(&[0, 0]);
            tag.extend_from_slice(body);
        }
        let n = tag.len();
        let mut out = b"ID3\x03\x00\x00".to_vec();
        out.extend([
            (n >> 21) as u8 & 0x7f,
            (n >> 14) as u8 & 0x7f,
            (n >> 7) as u8 & 0x7f,
            n as u8 & 0x7f,
        ]);
        out.extend(tag);
        out
    }

    #[test]
    fn finds_the_geob_frame() {
        let b = blob(&stbf_for(3), &[50; 40], &[50; 10], 100.0, 5);
        let tag = id3_with(&[
            (b"TIT2", b"\0Song".to_vec()),
            (b"GEOB", geob(b"other", b"application/x-other")),
            (b"GEOB", geob(&b, b"application/SMFMF")),
        ]);
        assert_eq!(from_id3(&tag), Some(b.clone()));
        let lower = id3_with(&[(b"GEOB", geob(&b, b"application/smfmf"))]);
        assert_eq!(from_id3(&lower), Some(b));
        assert_eq!(from_id3(&id3_with(&[(b"TIT2", b"\0Song".to_vec())])), None);
    }

    #[test]
    fn reads_player_output() {
        let b = blob(&stbf_for(5), &[50; 40], &[50; 10], 128.0, 42);
        let tag = id3_with(&[(b"GEOB", geob(&b, b"application/SMFMF"))]);
        let mut out = b"@@0\n@@1\n".to_vec();
        out.extend(format!("I{}\n", tag.len()).bytes());
        out.extend(&tag);
        let mut flac = b"SMFM".to_vec();
        flac.extend(&b);
        out.extend(format!("@@2\nF{}\n", flac.len()).bytes());
        out.extend(&flac);
        let got = parse_batch(&out);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], None);
        assert_eq!(got[1].as_ref().unwrap().chorus_ms, Some(42));
        assert_eq!(got[2], got[1]);
    }

    fn flac(blocks: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut out = b"fLaC".to_vec();
        for (i, (kind, body)) in blocks.iter().enumerate() {
            let last = if i + 1 == blocks.len() { 0x80 } else { 0 };
            let n = body.len();
            out.extend([kind | last, (n >> 16) as u8, (n >> 8) as u8, n as u8]);
            out.extend(body);
        }
        out
    }

    /// Feeds `flac_step` the windows the player would send until it settles.
    fn walk(file: &[u8]) -> (Walk, usize) {
        let size = file.len() as u64;
        let (mut at, mut len) = (0u64, WINDOW.min(size));
        for round in 1..=FLAC_ROUNDS {
            match flac_step(&file[at as usize..(at + len) as usize], at, size) {
                Walk::Need { at: a, len: l } => (at, len) = (a, l),
                done => return (done, round),
            }
        }
        panic!("no answer in {FLAC_ROUNDS} rounds");
    }

    #[test]
    fn walks_flac_metadata_in_windows() {
        let b = blob(&stbf_for(4), &[50; 40], &[50; 10], 100.0, 9);
        let mut app = b"SMFM".to_vec();
        app.extend(&b);
        let small = flac(&[
            (0, vec![0; 34]),
            (4, vec![1; 300]),
            (2, app.clone()),
            (1, vec![0; 8192]),
        ]);
        assert_eq!(walk(&small), (Walk::Found(b.clone()), 1));
        // A 200 KB picture before the analysis takes one more window.
        let picture = flac(&[
            (0, vec![0; 34]),
            (6, vec![7; 200_000]),
            (2, app.clone()),
            (1, vec![0; 100]),
        ]);
        assert_eq!(walk(&picture), (Walk::Found(b.clone()), 2));
        // The analysis straddling the first window's end is read again whole.
        let straddle = flac(&[
            (0, vec![0; 34]),
            (1, vec![0; 65_000]),
            (2, app.clone()),
            (1, vec![0; 100]),
        ]);
        assert_eq!(walk(&straddle), (Walk::Found(b), 2));
        let none = flac(&[(0, vec![0; 34]), (2, b"abcd1234".to_vec()), (1, vec![0; 70_000])]);
        assert_eq!(walk(&none).0, Walk::Nothing);
    }

    #[test]
    fn copies_id3_tags_out_whole() {
        let files = [
            PlayerFile {
                path: "/m/a.mp3",
                size: 5000,
            },
            PlayerFile {
                path: "/m/b.flac",
                size: 9000,
            },
            PlayerFile {
                path: "/m/c.wav",
                size: 9000,
            },
            PlayerFile {
                path: "/m/d.mp3",
                size: 100,
            },
        ];
        let heads = [
            *b"ID3\x03\x00\x00\x00\x00\x08\x00",
            *b"fLaC\x00\x00\x00\x22\x10\x00",
            *b"RIFF\x00\x00\x00\x00WA",
            *b"ID3\x03\x00\x00\x00\x00\x08\x00",
        ];
        let script = known_heads_script(&files, &heads);
        let calls: Vec<&str> = script.lines().skip_while(|l| *l != "}").skip(1).collect();
        assert_eq!(
            calls,
            [
                "echo @@0; echo I1034; $b head -c 1034 '/m/a.mp3'",
                "echo @@1",
                "echo @@2",
                "echo @@3",
            ]
        );
    }

    #[test]
    fn extracts_from_files() {
        let dir = std::env::temp_dir().join(format!("uwm-sensme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let b = blob(&stbf_for(9), &[50; 40], &[50; 10], 90.0, 7);
        let mp3 = dir.join("a.mp3");
        let mut data = id3_with(&[(b"GEOB", geob(&b, b"application/SMFMF"))]);
        data.extend_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
        std::fs::write(&mp3, &data).unwrap();
        assert_eq!(extract(&mp3), Some(b.clone()));

        let flac = dir.join("a.flac");
        let mut data = b"fLaC".to_vec();
        data.extend([0x00, 0x00, 0x00, 0x22]);
        data.extend([0u8; 0x22]);
        let len = b.len() + 4;
        data.extend([0x82, (len >> 16) as u8, (len >> 8) as u8, len as u8]);
        data.extend(b"SMFM");
        data.extend(&b);
        std::fs::write(&flac, &data).unwrap();
        assert_eq!(extract(&flac), Some(b));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
