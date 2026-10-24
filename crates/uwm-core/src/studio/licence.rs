//! The licence a kit's downloads come under: its text from where the publisher keeps it, and
//! the owner's agreement to that text. Google's Android SDK repository manifest carries its
//! licences in full (`<license id=… type="text">`), the text Android Studio's SDK manager
//! shows before it downloads.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::fetch;
use crate::{Error, Result, Why};

const AGREED: &str = "licences.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenceText {
    pub id: String,
    pub text: String,
    pub sha256: String,
}

/// Licence `id` as the manifest at `from` holds it: a kit's
/// [`KitLicence`](super::catalog::KitLicence), or the one Google's platform-tools come under.
pub fn read(id: &str, from: &str) -> Result<LicenceText> {
    let manifest = fetch::text(from)?;
    let text = text_in(&manifest, id).ok_or_else(|| {
        Error::unsupported(Why::LicenceNotFound, format!("no licence {id} in {from}")).with_detail(from.to_owned())
    })?;
    Ok(LicenceText {
        id: id.to_owned(),
        sha256: hex(&Sha256::digest(text.as_bytes())),
        text,
    })
}

/// The text of licence `id` in an SDK repository manifest, its entities resolved.
pub fn text_in(manifest: &str, id: &str) -> Option<String> {
    let start = manifest.find(&format!("<license id=\"{id}\""))?;
    let body = start + manifest[start..].find('>')? + 1;
    let end = body + manifest[body..].find("</license>")?;
    Some(unescape(manifest[body..end].trim()))
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(semi) = rest.find(';').filter(|&i| i <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let ch = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .map(|h| u32::from_str_radix(h, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse))
                .and_then(|n| n.ok())
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text the owner agreed to for each licence, by its sha256, in `<dir>/licences.json`.
pub fn agreed(dir: &Path) -> BTreeMap<String, String> {
    std::fs::read(dir.join(AGREED))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn agree(dir: &Path, text: &LicenceText) -> Result<()> {
    let mut all = agreed(dir);
    all.insert(text.id.clone(), text.sha256.clone());
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{AGREED}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&all).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, dir.join(AGREED))?;
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sdk:sdk-repository>
<license id="android-sdk-license" type="text">Terms and Conditions

1.1 The SDK (the &quot;SDK&quot;) &amp; its &lt;files&gt; are Google&apos;s &#169; &#x2014; ok.
</license><license id="android-sdk-preview-license" type="text">Preview</license>
<remotePackage path="build-tools;34.0.0"><uses-license ref="android-sdk-license"/></remotePackage>
</sdk:sdk-repository>"#;

    #[test]
    fn reads_one_licence_with_its_entities() {
        assert_eq!(
            text_in(MANIFEST, "android-sdk-license").as_deref(),
            Some("Terms and Conditions\n\n1.1 The SDK (the \"SDK\") & its <files> are Google's © — ok.")
        );
        assert_eq!(
            text_in(MANIFEST, "android-sdk-preview-license").as_deref(),
            Some("Preview")
        );
        assert_eq!(text_in(MANIFEST, "android-googletv-license"), None);
    }

    #[test]
    fn keeps_what_is_not_an_entity() {
        assert_eq!(unescape("a & b &unknown; &#zz; c"), "a & b &unknown; &#zz; c");
    }

    #[test]
    fn remembers_what_was_agreed() {
        let dir = std::env::temp_dir().join(format!("uwm-licence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(agreed(&dir).is_empty());
        let text = LicenceText {
            id: "android-sdk-license".into(),
            text: "Terms".into(),
            sha256: "ab".into(),
        };
        agree(&dir, &text).unwrap();
        assert_eq!(agreed(&dir).get("android-sdk-license").map(String::as_str), Some("ab"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
