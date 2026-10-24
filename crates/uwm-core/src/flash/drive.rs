//! The WALKMAN drive an NW-A50 series player shows on USB while Walkman OS runs: found with
//! lsblk, mounted and unmounted through udisks the way the desktop does, so nothing here needs
//! root. A storage set is copied onto it and read back from it, and the UPG's log read off it.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Result, Why, process};

const COLUMNS: &str = "PATH,PKNAME,TYPE,FSTYPE,LABEL,MOUNTPOINT,SERIAL,VENDOR,MODEL,TRAN,HCTL,SIZE";
const CHUNK: usize = 4 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    /// The whole disk, which Sony's vendor command goes to.
    pub disk: PathBuf,
    /// What holds the file system: the disk itself, or a partition of it.
    pub block: PathBuf,
    /// The USB serial number; adb shows the same one for the player.
    pub serial: Option<String>,
    pub label: Option<String>,
    /// 0 while the player lends this computer no medium.
    pub bytes: u64,
    pub mount: Option<PathBuf>,
}

/// Every WALKMAN drive on this computer: the player's internal storage, LUN 0. A card in the
/// player is another LUN and is left out.
pub fn find() -> Result<Vec<Drive>> {
    let out = process::run(
        "lsblk",
        Command::new("lsblk").args(["-J", "-b", "-o", COLUMNS]),
        Duration::from_secs(10),
    )?;
    if !out.success() {
        return Err(Error::Failed {
            tool: "lsblk",
            detail: out.stderr.trim().to_owned(),
        });
    }
    parse(&out.stdout)
}

/// The drive with the player's USB serial, or the only one there is when lsblk knows no
/// serial for it.
pub fn for_serial(serial: &str) -> Result<Option<Drive>> {
    let mut all = find()?;
    if let Some(i) = all.iter().position(|d| d.serial.as_deref() == Some(serial)) {
        return Ok(Some(all.swap_remove(i)));
    }
    Ok(match all.as_slice() {
        [only] if only.serial.is_none() => Some(only.clone()),
        _ => None,
    })
}

impl Drive {
    /// Mounts the drive where the desktop would, unless it is mounted, and returns where.
    pub fn mount(&mut self) -> Result<PathBuf> {
        if let Some(m) = &self.mount
            && self.mounted_at(m)
        {
            return Ok(m.clone());
        }
        // The desktop may mount it at the same moment; then udisks refuses, and it is mounted.
        let mounted = udisks("mount", &self.block);
        self.mount = find()?
            .into_iter()
            .find(|d| d.block == self.block)
            .and_then(|d| d.mount);
        if let Some(m) = &self.mount {
            return Ok(m.clone());
        }
        mounted?;
        Err(Error::unsupported(
            Why::DriveNotMounted,
            "udisks mounted the WALKMAN drive, but lsblk shows it nowhere",
        )
        .with_detail(self.block.to_string_lossy()))
    }

    /// `path` is this drive's file system, not a directory left behind once it was unmounted,
    /// where a copy would land on this computer's own disk.
    #[cfg(unix)]
    pub fn mounted_at(&self, path: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        match (fs::metadata(path), fs::metadata(&self.block)) {
            (Ok(dir), Ok(block)) => block.rdev() != 0 && dir.dev() == block.rdev(),
            _ => false,
        }
    }

    #[cfg(not(unix))]
    pub fn mounted_at(&self, path: &Path) -> bool {
        path.is_dir()
    }

    /// Unmounts it, which writes out everything this computer still holds for it.
    pub fn unmount(&mut self) -> Result<()> {
        if self.mount.is_some() {
            udisks("unmount", &self.block)?;
            self.mount = None;
        }
        Ok(())
    }
}

fn udisks(verb: &str, block: &Path) -> Result<()> {
    let out = process::run(
        "udisksctl",
        Command::new("udisksctl")
            .args([verb, "--no-user-interaction", "-b"])
            .arg(block),
        Duration::from_secs(60),
    )?;
    if out.success() {
        Ok(())
    } else {
        Err(Error::Failed {
            tool: "udisksctl",
            detail: out.stderr.trim().to_owned(),
        })
    }
}

/// Copies `from` to `rel` under `mount`: under a temporary name first, flushed to the drive,
/// then renamed, so the real name never holds half a file. `on_bytes` hears how much is copied.
pub fn copy_in(from: &Path, mount: &Path, rel: &str, cancel: &AtomicBool, on_bytes: &mut dyn FnMut(u64)) -> Result<()> {
    let to = mount.join(rel);
    let dir = to.parent().unwrap_or(mount).to_owned();
    fs::create_dir_all(&dir)?;
    let name = to
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let part = dir.join(format!(".{name}.part"));
    let copied = (|| -> Result<()> {
        let mut src = File::open(from)?;
        let mut dst = File::create(&part)?;
        let mut buf = vec![0u8; CHUNK];
        let mut done = 0u64;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(Error::Cancelled);
            }
            let n = src.read(&mut buf)?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n])?;
            done += n as u64;
            on_bytes(done);
        }
        dst.sync_all()?;
        Ok(())
    })();
    if let Err(e) = copied {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    fs::rename(&part, &to)?;
    File::open(&dir)?.sync_all()?;
    Ok(())
}

/// sha256 of a file on the drive. Read after a fresh mount, it comes from the drive and not
/// from what this computer kept while copying.
pub fn hash(mount: &Path, rel: &str, cancel: &AtomicBool, on_bytes: &mut dyn FnMut(u64)) -> Result<String> {
    let mut file = File::open(mount.join(rel))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        on_bytes(done);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Bytes the file at `rel` takes on the drive, when it is there.
pub fn size(mount: &Path, rel: &str) -> Option<u64> {
    fs::metadata(mount.join(rel))
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

/// The UPG's log from byte `from` on: what the UPG appended since then. The log grows by one
/// run each time; earlier runs' lines stay above.
pub fn log_since(mount: &Path, rel: &str, from: u64) -> Result<Option<String>> {
    let bytes = match fs::read(mount.join(rel)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let tail = bytes.get(from as usize..).unwrap_or_default();
    Ok(Some(String::from_utf8_lossy(tail).into_owned()))
}

#[derive(Deserialize)]
struct Listing {
    blockdevices: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    path: String,
    pkname: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    fstype: Option<String>,
    label: Option<String>,
    mountpoint: Option<String>,
    serial: Option<String>,
    vendor: Option<String>,
    model: Option<String>,
    tran: Option<String>,
    hctl: Option<String>,
    /// A number since util-linux 2.33's JSON, a string before.
    #[serde(default, deserialize_with = "number")]
    size: Option<u64>,
    #[serde(default)]
    children: Vec<Node>,
}

fn number<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<u64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum N {
        Int(u64),
        Text(String),
    }
    Ok(match Option::<N>::deserialize(d)? {
        Some(N::Int(n)) => Some(n),
        Some(N::Text(s)) => s.trim().parse().ok(),
        None => None,
    })
}

fn flatten(nodes: Vec<Node>, out: &mut Vec<Node>) {
    for mut n in nodes {
        let children = std::mem::take(&mut n.children);
        out.push(n);
        flatten(children, out);
    }
}

fn parse(json: &str) -> Result<Vec<Drive>> {
    let listing: Listing = serde_json::from_str(json).map_err(|e| Error::Failed {
        tool: "lsblk",
        detail: e.to_string(),
    })?;
    let mut nodes = Vec::new();
    flatten(listing.blockdevices, &mut nodes);
    let trimmed = |s: &Option<String>| s.as_deref().map(str::trim).unwrap_or_default().to_owned();
    let mut drives = Vec::new();
    for disk in nodes.iter().filter(|n| n.kind == "disk") {
        let walkman = trimmed(&disk.tran) == "usb"
            && trimmed(&disk.vendor) == "SONY"
            && trimmed(&disk.model).starts_with("WALKMAN")
            && disk.hctl.as_deref().is_some_and(|h| h.ends_with(":0"));
        if !walkman {
            continue;
        }
        let name = Path::new(&disk.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        let holder = if disk.fstype.is_some() {
            disk
        } else {
            nodes
                .iter()
                .find(|n| n.kind == "part" && n.pkname.is_some() && n.pkname == name && n.fstype.is_some())
                .unwrap_or(disk)
        };
        drives.push(Drive {
            disk: PathBuf::from(&disk.path),
            block: PathBuf::from(&holder.path),
            serial: disk.serial.clone().filter(|s| !s.trim().is_empty()),
            label: holder.label.clone(),
            bytes: disk.size.unwrap_or(0),
            mount: holder.mountpoint.as_ref().filter(|m| !m.is_empty()).map(PathBuf::from),
        });
    }
    Ok(drives)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LSBLK: &str = r#"{"blockdevices": [
        {"path": "/dev/nvme0n1", "pkname": null, "type": "disk", "fstype": null, "label": null,
         "mountpoint": null, "serial": "S1", "vendor": null, "model": "WD PC SN560", "tran": "nvme",
         "hctl": null, "size": 1024209543168},
        {"path": "/dev/sdb", "pkname": null, "type": "disk", "fstype": "vfat", "label": "WALKMAN",
         "mountpoint": "/run/media/owner/WALKMAN", "serial": "TESTA55SERIAL01", "vendor": "SONY    ",
         "model": "WALKMAN         ", "tran": "usb", "hctl": "6:0:0:0", "size": "12855712768"},
        {"path": "/dev/sdc", "pkname": null, "type": "disk", "fstype": null, "label": null,
         "mountpoint": null, "serial": "TESTA55SERIAL01", "vendor": "SONY", "model": "WALKMAN",
         "tran": "usb", "hctl": "6:0:0:1", "size": 0},
        {"path": "/dev/sdd", "pkname": null, "type": "disk", "fstype": null, "label": null,
         "mountpoint": null, "serial": null, "vendor": "SONY", "model": "WALKMAN", "tran": "usb",
         "hctl": "7:0:0:0", "size": 31914983424},
        {"path": "/dev/sdd1", "pkname": "sdd", "type": "part", "fstype": "vfat", "label": "WALKMAN",
         "mountpoint": null, "serial": null, "vendor": null, "model": null, "tran": "usb",
         "hctl": null, "size": 31913934848}
    ]}"#;

    #[test]
    fn finds_lun_0_of_each_walkman() {
        let drives = parse(LSBLK).unwrap();
        assert_eq!(drives.len(), 2, "not the NVMe disk, not the card LUN");
        let a = &drives[0];
        assert_eq!(
            a.block, a.disk,
            "a drive without partitions holds the file system itself"
        );
        assert_eq!(a.serial.as_deref(), Some("TESTA55SERIAL01"));
        assert_eq!(a.bytes, 12_855_712_768, "lsblk before 2.33 writes sizes as strings");
        assert_eq!(a.mount.as_deref(), Some(Path::new("/run/media/owner/WALKMAN")));
        let b = &drives[1];
        assert_eq!(
            (b.disk.as_path(), b.block.as_path()),
            (Path::new("/dev/sdd"), Path::new("/dev/sdd1"))
        );
        assert_eq!((b.serial.as_deref(), b.mount.as_deref()), (None, None));
    }

    #[test]
    fn reads_nested_children_too() {
        let nested = r#"{"blockdevices": [{"path": "/dev/sdd", "type": "disk", "fstype": null,
            "label": null, "mountpoint": null, "serial": "X", "vendor": "SONY", "model": "WALKMAN",
            "tran": "usb", "hctl": "7:0:0:0", "size": 1, "pkname": null,
            "children": [{"path": "/dev/sdd1", "pkname": "sdd", "type": "part", "fstype": "vfat",
            "label": "WALKMAN", "mountpoint": "/m", "serial": null, "vendor": null, "model": null,
            "tran": null, "hctl": null, "size": 1}]}]}"#;
        let d = &parse(nested).unwrap()[0];
        assert_eq!(
            (d.block.as_path(), d.mount.as_deref()),
            (Path::new("/dev/sdd1"), Some(Path::new("/m")))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_left_behind_is_not_the_mount() {
        let left = std::env::temp_dir();
        let gone = Drive {
            disk: "/dev/sdz".into(),
            block: "/dev/sdz".into(),
            serial: None,
            label: None,
            bytes: 1,
            mount: Some(left.clone()),
        };
        assert!(!gone.mounted_at(&left), "no such block device");
        let other = Drive {
            block: "/dev/null".into(),
            ..gone
        };
        assert!(!other.mounted_at(&left), "a device that does not hold this directory");
    }

    #[test]
    fn copies_through_a_temporary_name_and_reads_back() {
        let root = std::env::temp_dir().join(format!("uwm-drive-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let src = root.join("src.img");
        fs::write(&src, vec![7u8; CHUNK + 5]).unwrap();
        let mount = root.join("drive");
        fs::create_dir_all(&mount).unwrap();
        let never = AtomicBool::new(false);
        let mut seen = Vec::new();
        copy_in(&src, &mount, "wmmod-android/fire-system.img", &never, &mut |n| {
            seen.push(n)
        })
        .unwrap();
        assert_eq!(seen, [CHUNK as u64, CHUNK as u64 + 5]);
        assert_eq!(size(&mount, "wmmod-android/fire-system.img"), Some(CHUNK as u64 + 5));
        assert_eq!(
            fs::read_dir(mount.join("wmmod-android")).unwrap().count(),
            1,
            "no temporary file is left"
        );
        let want = format!("{:x}", Sha256::digest(fs::read(&src).unwrap()));
        assert_eq!(
            hash(&mount, "wmmod-android/fire-system.img", &never, &mut |_| {}).unwrap(),
            want
        );

        let stop = AtomicBool::new(true);
        assert!(matches!(
            copy_in(&src, &mount, "NW_WM_FW.UPG", &stop, &mut |_| {}),
            Err(Error::Cancelled)
        ));
        assert_eq!(size(&mount, "NW_WM_FW.UPG"), None);
        assert!(!mount.join(".NW_WM_FW.UPG.part").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_log_since_an_offset_is_what_one_run_appended() {
        let root = std::env::temp_dir().join(format!("uwm-drive-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert_eq!(log_since(&root, "PICKERUPG.log", 0).unwrap(), None);
        fs::write(root.join("PICKERUPG.log"), "old run\nnew run\n").unwrap();
        assert_eq!(
            log_since(&root, "PICKERUPG.log", 8).unwrap().as_deref(),
            Some("new run\n")
        );
        assert_eq!(log_since(&root, "PICKERUPG.log", 99).unwrap().as_deref(), Some(""));
        fs::remove_dir_all(&root).unwrap();
    }
}
