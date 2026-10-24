//! `install.json`, which a kit writes beside the images it built: what goes to which
//! partition, the hashes of what is sent, and what the booted player must show.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::studio::build::Variant;
use crate::{Error, Result, Why};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSet {
    pub kit: String,
    /// The kit's commit, when the copy it ran from knew it.
    pub commit: Option<String>,
    /// The kit ran from a working tree with changes no commit holds.
    pub dirty: bool,
    pub variant: Variant,
    /// The image has a root shell.
    pub root: bool,
    /// "NW-ZX500", the family the stock firmware is for.
    pub model: Option<String>,
    /// The stock firmware the image is built on, "4.06.00".
    pub firmware: Option<String>,
    pub series: Option<String>,
    /// The slot the set is installed to.
    pub slot: String,
    pub files: BTreeMap<String, SetFile>,
    pub flash: Vec<FlashItem>,
    pub after: After,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetFile {
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashItem {
    pub partition: String,
    pub file: String,
    /// fastboot options for this write, such as turning vbmeta's verification off.
    #[serde(default)]
    pub flags: Vec<String>,
}

/// What a booted player shows when the install took: read as the shell user, no root.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct After {
    /// The kernel's banner, which `/proc/version` repeats.
    pub kernel: String,
    pub check: Vec<CheckFile>,
    /// Paths the running system must not have.
    pub absent: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckFile {
    pub path: String,
    pub sha256: String,
}

/// The partitions a set may write, each on the set's own slot.
const PARTITIONS: [&str; 3] = ["vbmeta", "boot", "system"];
/// Going back to stock also writes vendor, which a kit's own build leaves as Sony made it.
const STOCK_PARTITIONS: [&str; 4] = ["vbmeta", "boot", "system", "vendor"];
/// The fastboot options a write may carry.
const FLAGS: [&str; 2] = ["--disable-verity", "--disable-verification"];

impl InstallSet {
    /// What the kit asks to write stays inside what the installer offers: `vbmeta`, `boot` and
    /// `system` on the set's slot, vbmeta's two verification switches, and plain file names
    /// beside `install.json`. A stock set adds `vendor` and carries no switches: the images go
    /// back as they came. The owner confirms with a partition's name, so the kit must not be
    /// able to name another.
    pub fn validate(&self) -> Result<()> {
        let bad = |what: String| {
            Err(Error::unsupported(Why::BadInstallSet, format!("install.json: {what}")).with_detail(what))
        };
        if !matches!(self.slot.as_str(), "a" | "b") {
            return bad(format!("slot {:?}", self.slot));
        }
        let (partitions, flags): (&[&str], &[&str]) = match self.variant {
            Variant::Stock => (&STOCK_PARTITIONS, &[]),
            Variant::Release | Variant::Debug => (&PARTITIONS, &FLAGS),
        };
        for f in &self.flash {
            let ok_partition = partitions.iter().any(|p| f.partition == format!("{p}_{}", self.slot));
            if !ok_partition {
                return bad(format!("partition {:?}", f.partition));
            }
            if let Some(flag) = f.flags.iter().find(|x| !flags.contains(&x.as_str())) {
                return bad(format!("flag {flag:?}"));
            }
            let plain = !f.file.is_empty() && f.file != ".." && !f.file.contains(['/', '\\']);
            if !plain {
                return bad(format!("file {:?}", f.file));
            }
        }
        Ok(())
    }

    /// Hashes every file the set flashes and checks it against what the kit wrote, so what is
    /// sent is what was built. Returns each file's name and hash.
    pub fn check_files(&self, dir: &Path, on_progress: &mut dyn FnMut(u64, u64)) -> Result<Vec<(String, String)>> {
        let names: Vec<&str> = self.flash.iter().map(|f| f.file.as_str()).collect();
        check_files(dir, &names, &self.files, "flashes", on_progress)
    }

    /// The firmware's major and minor version, the part `ro.sony.swid` carries: "4.06".
    pub fn firmware_line(&self) -> Option<&str> {
        let f = self.firmware.as_deref()?;
        let mut dots = f.match_indices('.').map(|(i, _)| i);
        dots.next()?;
        Some(dots.next().map_or(f, |second| &f[..second]))
    }
}

/// Hashes `names` under `dir` against what the kit wrote in `files`, in order, and returns each
/// name with its hash. `verb` says what the set does with them, for the error.
pub(super) fn check_files(
    dir: &Path,
    names: &[&str],
    files: &BTreeMap<String, SetFile>,
    verb: &str,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<Vec<(String, String)>> {
    let total: u64 = names.iter().filter_map(|n| files.get(*n)).map(|f| f.bytes).sum();
    let mut done = 0u64;
    let mut out = Vec::new();
    for &name in names {
        let want = files.get(name).ok_or_else(|| {
            Error::unsupported(
                Why::BadInstallSet,
                format!("install.json {verb} {name} but does not describe it"),
            )
        })?;
        let path = dir.join(name);
        let changed = || {
            Error::unsupported(Why::SetChanged, format!("{name} changed since the build"))
                .with_detail(path.to_string_lossy())
        };
        let mut file = std::fs::File::open(&path)?;
        if file.metadata()?.len() != want.bytes {
            return Err(changed());
        }
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 4 << 20];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            done += n as u64;
            on_progress(done, total);
        }
        let got = format!("{:x}", h.finalize());
        if got != want.sha256 {
            return Err(changed());
        }
        out.push((name.to_owned(), got));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_kit_install_json() {
        let set: InstallSet = serde_json::from_str(include_str!("../../tests/fixtures/install.json")).unwrap();
        assert_eq!(set.variant, Variant::Release);
        assert!(!set.root);
        assert_eq!(set.firmware_line(), Some("4.06"));
        assert_eq!(set.flash.len(), 3);
        assert_eq!(set.flash[0].flags, ["--disable-verity", "--disable-verification"]);
        assert_eq!(set.after.absent, ["/system/xbin/su"]);
        assert!(set.files["system.img"].bytes > 0);
    }

    #[test]
    fn writes_only_its_own_slot_s_three_partitions() {
        let good: InstallSet = serde_json::from_str(include_str!("../../tests/fixtures/install.json")).unwrap();
        good.validate().unwrap();
        let refused = |change: &dyn Fn(&mut InstallSet)| {
            let mut s = good.clone();
            change(&mut s);
            matches!(
                s.validate(),
                Err(Error::Unsupported {
                    why: Why::BadInstallSet,
                    ..
                })
            )
        };
        assert!(refused(&|s| s.flash[0].partition = "vbmeta_a".into()));
        assert!(refused(&|s| s.flash[2].partition = "userdata".into()));
        assert!(refused(&|s| s.flash[0].flags.push("--skip-reboot".into())));
        assert!(refused(&|s| s.flash[1].file = "../boot.img".into()));
        assert!(refused(&|s| s.flash[1].file = "/etc/passwd".into()));
        assert!(refused(&|s| s.slot = "c".into()));
        assert!(
            refused(&|s| s.flash[2].partition = "vendor_b".into()),
            "a kit build leaves vendor"
        );
    }

    #[test]
    fn a_stock_set_writes_vendor_too_and_as_it_came() {
        let good: InstallSet = serde_json::from_str(include_str!("../../tests/fixtures/restore.json")).unwrap();
        assert_eq!(good.variant, Variant::Stock);
        good.validate().unwrap();
        assert_eq!(good.flash.len(), 4);
        let refused = |change: &dyn Fn(&mut InstallSet)| {
            let mut s = good.clone();
            change(&mut s);
            s.validate().is_err()
        };
        assert!(refused(&|s| s.flash[0].flags.push("--disable-verity".into())));
        assert!(refused(&|s| s.flash[3].partition = "vendor_a".into()));
        assert!(refused(&|s| s.flash[3].partition = "userdata".into()));
    }

    #[test]
    fn checks_the_files_it_sends() {
        let dir = std::env::temp_dir().join(format!("uwm-set-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut set: InstallSet = serde_json::from_str(include_str!("../../tests/fixtures/install.json")).unwrap();
        for (name, body) in [("vbmeta.img", "v"), ("boot.img", "bb"), ("system.img", "sss")] {
            std::fs::write(dir.join(name), body).unwrap();
            set.files.insert(
                name.into(),
                SetFile {
                    bytes: body.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(body.as_bytes())),
                },
            );
        }
        let mut last = (0, 0);
        let checked = set.check_files(&dir, &mut |d, t| last = (d, t)).unwrap();
        assert_eq!(checked.len(), 3);
        assert_eq!(last, (6, 6));
        std::fs::write(dir.join("boot.img"), "bx").unwrap();
        let e = set.check_files(&dir, &mut |_, _| {}).unwrap_err();
        assert!(matches!(
            e,
            Error::Unsupported {
                why: Why::SetChanged,
                ..
            }
        ));
    }
}
