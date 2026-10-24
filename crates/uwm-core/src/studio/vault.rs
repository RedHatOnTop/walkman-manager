//! The files the owner added. They stay where the owner keeps them: the vault records the
//! path, the size and time it was hashed at, and what it is, and a kit reads it from there.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::build::Variant;
use super::catalog::{self, FormatPin, KnownFile, SetKind};
use super::identify::{self, Format, Identified};
use crate::Result;
use crate::library::hex_id;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    /// Seconds since the epoch.
    pub added: i64,
    #[serde(flatten)]
    pub file: Identified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Presence {
    Here,
    /// Size or time moved since it was hashed, so what it is has to be looked at again.
    Changed,
    Gone,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    #[serde(flatten)]
    pub entry: Entry,
    pub presence: Presence,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitView {
    pub id: String,
    pub profile: String,
    pub inputs: Vec<InputView>,
    /// Every input that is not optional has a file.
    pub ready: bool,
    /// The kit has build steps the Studio can run.
    pub buildable: bool,
    /// The kit can make a set of the stock firmware, and the files that takes are here; None
    /// when it cannot.
    pub restore: Option<bool>,
    /// The kit makes debug builds too and keeps root on request.
    pub variants: bool,
    pub set: SetKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputView {
    pub id: String,
    pub optional: bool,
    pub accepts: Vec<&'static KnownFile>,
    pub formats: &'static [FormatPin],
    /// The guide has a section on making or getting this input's file.
    pub guided: bool,
    /// The vault entry that fills it.
    pub filled_by: Option<String>,
}

impl FormatPin {
    pub fn matches(&self, format: &Format) -> bool {
        format.kind() == self.kind
            && match (format, &self.release) {
                (_, None) => true,
                (Format::KernelImage { release }, Some(want)) => release.as_ref() == Some(want),
                _ => false,
            }
    }
}

pub struct Vault {
    file: PathBuf,
    entries: Vec<Entry>,
}

impl Vault {
    /// A vault file that does not parse is kept aside as `vault.json.bad-<secs>`, so the next
    /// save does not overwrite what it held.
    pub fn open(file: PathBuf) -> Vault {
        let entries = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let mut aside = file.as_os_str().to_owned();
                aside.push(format!(".bad-{secs}"));
                tracing::warn!("vault: {} does not parse ({e}); kept as {:?}", file.display(), aside);
                let _ = std::fs::rename(&file, aside);
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        Vault { file, entries }
    }

    pub fn view(&self) -> Vec<EntryView> {
        self.entries
            .iter()
            .map(|e| EntryView {
                entry: e.clone(),
                presence: presence(e),
            })
            .collect()
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Takes a looked-at file in, in place of an earlier look at the same path.
    pub fn add(&mut self, mut file: Identified) -> Result<Entry> {
        // The app and the CLI run from different directories; a relative path names
        // another file, or none, from the other one.
        file.path = std::path::absolute(&file.path)?;
        let entry = Entry {
            id: entry_id(&file.path),
            added: now(),
            file,
        };
        self.entries.retain(|e| e.id != entry.id);
        self.entries.push(entry.clone());
        self.save()?;
        Ok(entry)
    }

    pub fn remove(&mut self, id: &str) -> Result<bool> {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        if self.entries.len() == before {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    pub fn kits(&self) -> Vec<KitView> {
        let usable: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.file.damage.is_none() && presence(e) == Presence::Here)
            .collect();
        catalog::kits()
            .iter()
            .map(|k| {
                let inputs: Vec<InputView> = k
                    .inputs
                    .iter()
                    .map(|i| InputView {
                        id: i.id.clone(),
                        optional: i.optional,
                        accepts: i.accepts.iter().filter_map(|a| catalog::file(a)).collect(),
                        formats: &i.formats,
                        guided: i.guide.is_some(),
                        filled_by: usable
                            .iter()
                            .find(|e| {
                                e.file.known.as_ref().is_some_and(|k| i.accepts.contains(k))
                                    || i.formats.iter().any(|f| f.matches(&e.file.format))
                            })
                            .map(|e| e.id.clone()),
                    })
                    .collect();
                let restore = k.source.as_ref().filter(|s| s.restore.is_some()).map(|s| {
                    s.inputs(Variant::Stock)
                        .iter()
                        .all(|id| inputs.iter().any(|i| i.id == *id && i.filled_by.is_some()))
                });
                KitView {
                    id: k.id.clone(),
                    profile: k.profile.clone(),
                    ready: inputs.iter().all(|i| i.optional || i.filled_by.is_some()),
                    buildable: k.source.is_some(),
                    restore,
                    variants: k.source.as_ref().is_none_or(|s| s.variants),
                    set: k.set,
                    inputs,
                }
            })
            .collect()
    }

    fn save(&self) -> Result<()> {
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&self.entries).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

pub fn entry_id(path: &Path) -> String {
    hex_id(path.as_os_str().as_encoded_bytes())
}

fn presence(e: &Entry) -> Presence {
    match identify::stamp(&e.file.path) {
        Ok((bytes, modified)) if bytes == e.file.bytes && modified == e.file.modified => Presence::Here,
        Ok(_) => Presence::Changed,
        Err(_) => Presence::Gone,
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::studio::identify::identify;

    #[test]
    fn keeps_a_vault_file_that_does_not_parse() {
        let dir = std::env::temp_dir().join(format!("uwm-vault-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("vault.json"), "[{\"id\": ").unwrap();
        assert!(Vault::open(dir.join("vault.json")).view().is_empty());
        let kept: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].starts_with("vault.json.bad-"), "{kept:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_kit_is_ready_when_known_files_fill_its_inputs() {
        let dir = std::env::temp_dir().join(format!("uwm-vault-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut vault = Vault::open(dir.join("vault.json"));
        let zx = || vault_kit(&Vault::open(dir.join("vault.json")), "nw-zx500-android13");
        assert!(!zx().ready);

        let mut fake = |name: &str, known: Option<&str>| {
            let p = dir.join(name);
            std::fs::write(&p, name).unwrap();
            let mut f = identify(&p, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
            f.known = known.map(str::to_string);
            vault.add(f).unwrap()
        };
        fake("gsi.img.xz", Some("td-ci-20230905-arm64-ab-vanilla-xz"));
        assert!(!zx().ready);
        assert_eq!(zx().restore, Some(false));
        let stock = fake("stock.UPG", Some("nw-zx500-0000-4.06.00"));
        assert!(!zx().ready, "no kernel yet");
        assert_eq!(zx().restore, Some(true), "going back takes only the UPG");
        let mut image = vec![0u8; 4096];
        image[56..60].copy_from_slice(b"ARM\x64");
        image[1000..1040].copy_from_slice(b"Linux version 4.14.78-g69dd21e (b@h) #1 ");
        std::fs::write(dir.join("Image"), &image).unwrap();
        let kernel = vault
            .add(identify(&dir.join("Image"), &AtomicBool::new(false), &mut |_, _| {}).unwrap())
            .unwrap();
        let kit = zx();
        assert!(kit.ready);
        assert_eq!(kit.inputs[1].filled_by.as_deref(), Some(stock.id.as_str()));
        assert_eq!(kit.inputs[2].filled_by.as_deref(), Some(kernel.id.as_str()));
        assert_eq!(kit.inputs[3].filled_by, None);
        let mut relative = identify(&dir.join("gsi.img.xz"), &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        relative.path = PathBuf::from("gsi.img.xz");
        let kept = vault.add(relative).unwrap();
        assert!(kept.file.path.is_absolute(), "{:?}", kept.file.path);
        assert!(vault.remove(&kept.id).unwrap());

        std::fs::write(dir.join("stock.UPG"), "grown since").unwrap();
        assert!(!zx().ready, "a file changed after hashing fills nothing");
        assert!(vault.remove(&stock.id).unwrap());
        assert!(!vault.remove(&stock.id).unwrap());
    }

    fn vault_kit(v: &Vault, id: &str) -> KitView {
        v.kits().into_iter().find(|k| k.id == id).unwrap()
    }
}
