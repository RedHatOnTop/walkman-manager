//! The files the Studio knows and the kits that take them, from `kits/` in the repository.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::build::Variant;
use super::identify::FormatKind;

const FILES: &str = include_str!("../../../../kits/files.toml");
const KITS: &[(&str, &str)] = &[
    (
        "nw-zx500-android13",
        include_str!("../../../../kits/nw-zx500-android13.toml"),
    ),
    ("nw-a50-android5", include_str!("../../../../kits/nw-a50-android5.toml")),
];

/// The languages the app speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ko,
}

/// A text in each language the app speaks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Localized {
    pub en: String,
    pub ko: String,
}

impl Localized {
    pub fn get(&self, lang: Lang) -> &str {
        match lang {
            Lang::En => &self.en,
            Lang::Ko => &self.ko,
        }
    }
}

/// Where the owner reads how to get the files the app does not download, in their language.
pub fn guide(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "https://github.com/RedHatOnTop/walkman-manager/blob/main/docs/guides/firmware-files.md",
        Lang::Ko => "https://github.com/RedHatOnTop/walkman-manager/blob/main/docs/guides/firmware-files.ko.md",
    }
}

/// A file someone else publishes, and how the Studio knows a copy is that file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct KnownFile {
    pub id: String,
    pub title: Localized,
    pub format: FormatKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
    /// A direct download from the publisher, which the app may fetch for the owner. Only for
    /// files whose licence lets anyone pass them on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The heading in the guide, in each language, that says where the publisher put this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guide: Option<Localized>,
    /// The known file this one is unpacked from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

impl KnownFile {
    pub fn is(&self, sha256: &str, md5: &str) -> bool {
        self.sha256.as_deref() == Some(sha256) || self.md5.as_deref() == Some(md5)
    }

    pub fn guide_url(&self, lang: Lang) -> Option<String> {
        self.guide
            .as_ref()
            .map(|anchor| format!("{}#{}", guide(lang), anchor.get(lang)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Kit {
    pub id: String,
    /// The player profile the kit builds for.
    pub profile: String,
    /// The repository the build steps live in. A kit without one only names its inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<KitSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<HostNeeds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallNeeds>,
    /// What the kit's own downloads come under, which the owner agrees to before a build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<KitLicence>,
    /// How the set the kit makes gets onto the player.
    #[serde(default)]
    pub set: SetKind,
    #[serde(rename(deserialize = "input"))]
    pub inputs: Vec<KitInput>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SetKind {
    /// Partitions written over Fastboot (`flash::InstallSet`).
    #[default]
    Fastboot,
    /// Files copied onto the player's own storage and taken in by a UPG its updater runs
    /// (`flash::StorageSet`).
    Storage,
}

/// A licence the publisher keeps the text of, and how the kit is told the owner agreed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct KitLicence {
    /// The licence's id in the publisher's manifest.
    pub id: String,
    /// The manifest that holds its text: an Android SDK repository XML.
    pub from: String,
    /// The environment variable the kit reads; the Studio sets it to `accepted`.
    pub env: String,
}

/// How the installer tells a player that already runs one of this kit's builds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct InstallNeeds {
    /// A file only the kit's builds have.
    pub marker: String,
}

/// A device repository's build entry point at a fixed commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct KitSource {
    /// The repository's name, which the owner's checkout of it is told by.
    pub repository: String,
    pub commit: String,
    /// The script the Studio runs, relative to the repository.
    pub entry: String,
    /// What it takes, in order: `input:<id>` for a vault file, `key` for the unit's UPG key,
    /// `stock` for the folder the stock partitions are kept in. The folder for the install set
    /// comes last.
    pub args: Vec<String>,
    /// The repository at `commit` as published (`git archive`), which the Studio downloads.
    /// Without it the owner names a checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<KitArchive>,
    /// The entry point that makes an install set of the stock firmware, for going back to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore: Option<KitSteps>,
    /// The entry point makes a debug build as well and keeps root on request; without these
    /// it makes one kind of build.
    #[serde(default = "yes")]
    pub variants: bool,
}

fn yes() -> bool {
    true
}

/// A second entry point in the kit's repository, its arguments named as `KitSource::args`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct KitSteps {
    pub entry: String,
    pub args: Vec<String>,
}

impl KitSource {
    /// The script that makes a set of this kind and its arguments: the restore entry for
    /// stock, the main one for the kit's own builds.
    pub fn steps(&self, variant: Variant) -> Option<(&str, &[String])> {
        match variant {
            Variant::Stock => self.restore.as_ref().map(|r| (r.entry.as_str(), r.args.as_slice())),
            Variant::Release | Variant::Debug => Some((self.entry.as_str(), self.args.as_slice())),
        }
    }

    /// The kit inputs the steps for `variant` take.
    pub fn inputs(&self, variant: Variant) -> Vec<&str> {
        self.steps(variant)
            .map(|(_, args)| args.iter().filter_map(|a| a.strip_prefix("input:")).collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct KitArchive {
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
}

/// What a kit's steps run on the owner's computer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct HostNeeds {
    /// Programs that must be on `PATH`.
    pub tools: Vec<String>,
    /// Modules `python3` must import.
    #[serde(default)]
    pub python: Vec<String>,
    /// The lowest Java feature release.
    pub java: Option<u32>,
    /// The Android NDK's directory name, under the usual places.
    pub ndk: Option<String>,
    /// The Android SDK's build-tools and platform the kit compiles against.
    pub build_tools: Option<String>,
    pub platform: Option<String>,
    /// Free space the build needs where the Studio keeps its files.
    pub free_gb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct KitInput {
    pub id: String,
    /// Known file ids, any one of which fills this input.
    #[serde(default)]
    pub accepts: Vec<String>,
    /// Files of the owner's own making, which no pin can name: they fill the input by what
    /// they are.
    #[serde(default)]
    pub formats: Vec<FormatPin>,
    /// The guide's heading, in each language, on how to make or get a file for this input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guide: Option<Localized>,
    #[serde(default)]
    pub optional: bool,
}

/// A structure an input takes, with what its content must say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct FormatPin {
    pub kind: FormatKind,
    /// A kernel's release string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
}

impl KitInput {
    pub fn guide_url(&self, lang: Lang) -> Option<String> {
        self.guide
            .as_ref()
            .map(|anchor| format!("{}#{}", guide(lang), anchor.get(lang)))
    }
}

pub fn kit(id: &str) -> Option<&'static Kit> {
    kits().iter().find(|k| k.id == id)
}

#[derive(Deserialize)]
struct Files {
    file: Vec<KnownFile>,
}

pub fn files() -> &'static [KnownFile] {
    static FILES_LIST: OnceLock<Vec<KnownFile>> = OnceLock::new();
    FILES_LIST.get_or_init(|| {
        toml::from_str::<Files>(FILES)
            .unwrap_or_else(|e| panic!("kits/files.toml: {e}"))
            .file
    })
}

pub fn kits() -> &'static [Kit] {
    static KIT_LIST: OnceLock<Vec<Kit>> = OnceLock::new();
    KIT_LIST.get_or_init(|| {
        KITS.iter()
            .map(|(name, src)| toml::from_str(src).unwrap_or_else(|e| panic!("kits/{name}.toml: {e}")))
            .collect()
    })
}

pub fn file(id: &str) -> Option<&'static KnownFile> {
    files().iter().find(|f| f.id == id)
}

/// The known file with this content.
pub fn recognise(sha256: &str, md5: &str) -> Option<&'static KnownFile> {
    files().iter().find(|f| f.is(sha256, md5))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn hex(s: &str, len: usize) -> bool {
        s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    /// The anchors GitHub gives a guide's second-level headings.
    fn anchors(guide: &str) -> HashSet<String> {
        guide
            .lines()
            .filter_map(|l| l.strip_prefix("## "))
            .map(|h| {
                h.to_lowercase()
                    .chars()
                    .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
                    .map(|c| if c == ' ' { '-' } else { c })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_kit_without_a_restore_entry_makes_no_stock_set() {
        let source = KitSource {
            repository: "walkman-zx507".into(),
            commit: "0".repeat(40),
            entry: "scripts/make-install.sh".into(),
            args: vec!["input:gsi".into(), "key".into(), "stock".into()],
            archive: None,
            restore: None,
            variants: true,
        };
        assert!(source.steps(Variant::Stock).is_none() && source.inputs(Variant::Stock).is_empty());
        assert_eq!(source.inputs(Variant::Release), ["gsi"]);
    }

    #[test]
    fn every_file_is_pinned_and_every_kit_input_is_known() {
        let en = anchors(include_str!("../../../../docs/guides/firmware-files.md"));
        let ko = anchors(include_str!("../../../../docs/guides/firmware-files.ko.md"));
        let mut ids = HashSet::new();
        for f in files() {
            assert!(ids.insert(f.id.as_str()), "{} twice", f.id);
            assert!(f.sha256.is_some() || f.md5.is_some(), "{} has no pin", f.id);
            assert!(f.sha256.as_deref().is_none_or(|s| hex(s, 64)), "{} sha256", f.id);
            assert!(f.md5.as_deref().is_none_or(|s| hex(s, 32)), "{} md5", f.id);
            assert!(
                f.from.as_deref().is_none_or(|from| file(from).is_some()),
                "{} from",
                f.id
            );
            assert!(
                f.url.as_deref().is_none_or(|u| u.starts_with("https://")),
                "{} url",
                f.id
            );
            assert!(
                f.guide
                    .as_ref()
                    .is_none_or(|g| en.contains(&g.en) && ko.contains(&g.ko)),
                "{} guide",
                f.id
            );
            assert!(!f.title.en.is_empty() && !f.title.ko.is_empty(), "{} title", f.id);
            assert!(
                f.url.is_some() || f.guide.is_some() || f.from.is_some(),
                "{} says nowhere how to get it",
                f.id
            );
        }
        for k in kits() {
            assert!(
                crate::profile::all().iter().any(|p| p.id == k.profile),
                "{} profile",
                k.id
            );
            assert!(k.inputs.iter().any(|i| !i.optional), "{} takes nothing", k.id);
            for i in &k.inputs {
                for a in &i.accepts {
                    assert!(file(a).is_some(), "{}/{} accepts unknown {a}", k.id, i.id);
                }
                assert!(
                    !i.accepts.is_empty() || !i.formats.is_empty(),
                    "{}/{} takes nothing",
                    k.id,
                    i.id
                );
                assert!(
                    i.guide
                        .as_ref()
                        .is_none_or(|g| en.contains(&g.en) && ko.contains(&g.ko)),
                    "{}/{} guide",
                    k.id,
                    i.id
                );
            }
            if let Some(src) = &k.source {
                assert!(hex(&src.commit, 40), "{} commit", k.id);
                for v in [Variant::Release, Variant::Debug, Variant::Stock] {
                    let Some((entry, args)) = src.steps(v) else { continue };
                    assert!(!entry.starts_with('/') && !entry.contains(".."), "{} entry", k.id);
                    for a in args {
                        let known = match a.strip_prefix("input:") {
                            Some(id) => k.inputs.iter().any(|i| i.id == id && !i.optional),
                            None => a == "key" || a == "stock",
                        };
                        assert!(known, "{} arg {a}", k.id);
                    }
                }
            }
        }
    }
}
