use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::library::format::Playback;

const SOURCES: &[(&str, &str)] = &[
    ("nw-a50", include_str!("../../../profiles/nw-a50.toml")),
    ("nw-zx500", include_str!("../../../profiles/nw-zx500.toml")),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Profile {
    pub id: String,
    pub family: String,
    pub art: String,
    pub soc: String,
    pub dac: String,
    pub ram_mb: u32,
    /// Width, height, depth with projecting parts, from Sony's Help Guide.
    pub body_mm: [f32; 3],
    pub display: String,
    pub mass_g: u32,
    pub stock_os: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_firmware_latest: Option<String>,
    pub recovery: Vec<String>,
    pub variants: Vec<Variant>,
    #[serde(default, skip_serializing)]
    pub identify: Vec<BTreeMap<String, String>>,
    #[serde(default)]
    pub usb: Vec<UsbSignature>,
    pub playback: Playback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Variant {
    pub model: String,
    pub storage_gb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct UsbSignature {
    pub vid: u16,
    pub pid: u16,
    pub seen: String,
}

pub fn all() -> &'static [Profile] {
    static PROFILES: OnceLock<Vec<Profile>> = OnceLock::new();
    PROFILES.get_or_init(|| {
        SOURCES
            .iter()
            .map(|(name, src)| toml::from_str(src).unwrap_or_else(|e| panic!("profiles/{name}.toml: {e}")))
            .collect()
    })
}

pub fn get(id: &str) -> Option<&'static Profile> {
    all().iter().find(|p| p.id == id)
}

/// The profile whose identify rules match these system properties.
pub fn by_props(props: &BTreeMap<String, String>) -> Option<&'static Profile> {
    all().iter().find(|p| {
        p.identify
            .iter()
            .any(|rule| !rule.is_empty() && rule.iter().all(|(k, v)| props.get(k) == Some(v)))
    })
}

pub fn by_usb(vid: u16, pid: u16) -> Option<&'static Profile> {
    all()
        .iter()
        .find(|p| p.usb.iter().any(|s| s.vid == vid && s.pid == pid))
}

impl Profile {
    /// Picks the smallest variant whose rated capacity holds the user storage the
    /// device reports; rated "16 GB" parts expose about 13 GB after formatting.
    pub fn variant_for_storage(&self, user_storage_bytes: u64) -> Option<&Variant> {
        let mut by_size: Vec<&Variant> = self.variants.iter().collect();
        by_size.sort_by_key(|v| v.storage_gb);
        by_size
            .into_iter()
            .find(|v| u64::from(v.storage_gb) * 1_000_000_000 >= user_storage_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_parse_and_have_unique_ids() {
        let ids: Vec<_> = all().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["nw-a50", "nw-zx500"]);
        assert!(all().iter().all(|p| !p.identify.is_empty() && !p.variants.is_empty()));
    }

    #[test]
    fn variant_by_capacity() {
        let a50 = get("nw-a50").unwrap();
        assert_eq!(a50.variant_for_storage(12_855_712 * 1024).unwrap().model, "NW-A55");
        let zx = get("nw-zx500").unwrap();
        assert_eq!(zx.variant_for_storage(52_267_164 * 1024).unwrap().model, "NW-ZX507");
        assert_eq!(zx.variant_for_storage(13_000_000_000).unwrap().model, "NW-ZX505");
        assert!(zx.variant_for_storage(200_000_000_000).is_none());
    }

    #[test]
    fn usb_lookup() {
        assert_eq!(by_usb(0x054c, 0x0bc1).unwrap().id, "nw-a50");
        assert_eq!(by_usb(0x054c, 0x0cf2).unwrap().id, "nw-zx500");
        assert!(by_usb(0x054c, 0x0001).is_none());
    }
}
