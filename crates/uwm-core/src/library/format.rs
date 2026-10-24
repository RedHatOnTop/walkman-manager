use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Codec {
    Flac,
    Alac,
    Wav,
    Aiff,
    Ape,
    Dsf,
    Dff,
    Mp3,
    Aac,
    Wma,
    Vorbis,
    Opus,
    WavPack,
    Musepack,
}

impl Codec {
    pub const ALL: [Codec; 14] = [
        Codec::Flac,
        Codec::Alac,
        Codec::Wav,
        Codec::Aiff,
        Codec::Ape,
        Codec::Dsf,
        Codec::Dff,
        Codec::Mp3,
        Codec::Aac,
        Codec::Wma,
        Codec::Vorbis,
        Codec::Opus,
        Codec::WavPack,
        Codec::Musepack,
    ];

    /// The codec a file name points at. `.m4a` reads as AAC until its sample
    /// description says ALAC.
    pub fn from_path(path: &Path) -> Option<Codec> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "flac" => Codec::Flac,
            "wav" => Codec::Wav,
            "aif" | "aiff" | "aifc" => Codec::Aiff,
            "ape" => Codec::Ape,
            "dsf" => Codec::Dsf,
            "dff" => Codec::Dff,
            "mp3" => Codec::Mp3,
            "m4a" | "mp4" | "aac" | "3gp" => Codec::Aac,
            "wma" => Codec::Wma,
            "ogg" | "oga" => Codec::Vorbis,
            "opus" => Codec::Opus,
            "wv" => Codec::WavPack,
            "mpc" => Codec::Musepack,
            _ => return None,
        })
    }

    pub fn key(self) -> &'static str {
        match self {
            Codec::Flac => "flac",
            Codec::Alac => "alac",
            Codec::Wav => "wav",
            Codec::Aiff => "aiff",
            Codec::Ape => "ape",
            Codec::Dsf => "dsf",
            Codec::Dff => "dff",
            Codec::Mp3 => "mp3",
            Codec::Aac => "aac",
            Codec::Wma => "wma",
            Codec::Vorbis => "vorbis",
            Codec::Opus => "opus",
            Codec::WavPack => "wavpack",
            Codec::Musepack => "musepack",
        }
    }

    pub fn from_key(key: &str) -> Option<Codec> {
        Codec::ALL.into_iter().find(|c| c.key() == key)
    }

    pub fn is_dsd(self) -> bool {
        matches!(self, Codec::Dsf | Codec::Dff)
    }

    pub fn is_lossless(self) -> bool {
        matches!(
            self,
            Codec::Flac | Codec::Alac | Codec::Wav | Codec::Aiff | Codec::Ape | Codec::WavPack
        ) || self.is_dsd()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFormat {
    pub codec: Codec,
    /// Samples per second; the one-bit rate for DSD (2_822_400 for DSD64).
    pub sample_rate: Option<u32>,
    pub bits: Option<u8>,
    pub channels: Option<u8>,
    pub bitrate_kbps: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Quality {
    Lossy,
    Lossless,
    HiRes,
    Dsd,
}

const HIRES_MIN_KBPS: u32 = 1536;

impl AudioFormat {
    /// Sony's line: lossless above 44.1 kHz/16 bit and 48 kHz/16 bit is High-Resolution.
    pub fn quality(&self) -> Quality {
        if self.codec.is_dsd() {
            Quality::Dsd
        } else if !self.codec.is_lossless() {
            Quality::Lossy
        } else if self.sample_rate.is_some_and(|r| r > 48_000) || self.bits.is_some_and(|b| b > 16) {
            Quality::HiRes
        } else if self.sample_rate.is_none()
            && self.bits.is_none()
            && self.bitrate_kbps.is_some_and(|k| k > HIRES_MIN_KBPS)
        {
            // A player's media store gives only the bit rate. Stereo 16 bit at 48 kHz
            // is 1536 kbps uncompressed, so anything above it carries more.
            Quality::HiRes
        } else {
            Quality::Lossless
        }
    }
}

/// What a player's own music app plays, from the "Supported formats" page of its
/// Help Guide. Rates are upper limits per codec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Playback {
    /// Files at this size or larger do not play.
    pub max_file_bytes: u64,
    pub codecs: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unplayable {
    Codec,
    SampleRate,
    FileSize,
}

impl Playback {
    pub fn check(&self, format: &AudioFormat, size: u64) -> Result<(), Unplayable> {
        let Some(&max_rate) = self.codecs.get(format.codec.key()) else {
            return Err(Unplayable::Codec);
        };
        if format.sample_rate.is_some_and(|r| r > max_rate) {
            return Err(Unplayable::SampleRate);
        }
        if size >= self.max_file_bytes {
            return Err(Unplayable::FileSize);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(codec: Codec, rate: u32, bits: Option<u8>) -> AudioFormat {
        AudioFormat {
            codec,
            sample_rate: Some(rate),
            bits,
            channels: Some(2),
            bitrate_kbps: None,
        }
    }

    #[test]
    fn quality_classes() {
        assert_eq!(fmt(Codec::Flac, 44_100, Some(16)).quality(), Quality::Lossless);
        assert_eq!(fmt(Codec::Flac, 48_000, Some(16)).quality(), Quality::Lossless);
        assert_eq!(fmt(Codec::Flac, 44_100, Some(24)).quality(), Quality::HiRes);
        assert_eq!(fmt(Codec::Alac, 96_000, Some(24)).quality(), Quality::HiRes);
        assert_eq!(fmt(Codec::Mp3, 96_000, None).quality(), Quality::Lossy);
        assert_eq!(fmt(Codec::Dsf, 2_822_400, Some(1)).quality(), Quality::Dsd);
        let rate_only = |codec, kbps| AudioFormat {
            codec,
            sample_rate: None,
            bits: None,
            channels: None,
            bitrate_kbps: Some(kbps),
        };
        assert_eq!(rate_only(Codec::Flac, 2304).quality(), Quality::HiRes);
        assert_eq!(rate_only(Codec::Flac, 905).quality(), Quality::Lossless);
        assert_eq!(rate_only(Codec::Wav, 1411).quality(), Quality::Lossless);
        assert_eq!(rate_only(Codec::Mp3, 3000).quality(), Quality::Lossy);
    }

    #[test]
    fn extensions() {
        assert_eq!(Codec::from_path(Path::new("a/b/Track.FLAC")), Some(Codec::Flac));
        assert_eq!(Codec::from_path(Path::new("x.m4a")), Some(Codec::Aac));
        assert_eq!(Codec::from_path(Path::new("cover.jpg")), None);
        assert_eq!(Codec::from_key("wavpack"), Some(Codec::WavPack));
    }

    #[test]
    fn playback_limits() {
        let p = Playback {
            max_file_bytes: 4 << 30,
            codecs: [("flac".to_owned(), 384_000), ("dsf".to_owned(), 11_289_600)].into(),
        };
        assert_eq!(p.check(&fmt(Codec::Flac, 192_000, Some(24)), 1 << 20), Ok(()));
        assert_eq!(
            p.check(&fmt(Codec::Flac, 768_000, Some(32)), 1 << 20),
            Err(Unplayable::SampleRate)
        );
        assert_eq!(
            p.check(&fmt(Codec::Opus, 48_000, None), 1 << 20),
            Err(Unplayable::Codec)
        );
        assert_eq!(
            p.check(&fmt(Codec::Dsf, 22_579_200, Some(1)), 1 << 20),
            Err(Unplayable::SampleRate)
        );
        assert_eq!(
            p.check(&fmt(Codec::Flac, 96_000, Some(24)), 4 << 30),
            Err(Unplayable::FileSize)
        );
    }
}
