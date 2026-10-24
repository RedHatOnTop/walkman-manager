//! Folds USB enumeration and adb's device list into one entry per player.
//!
//! The registry only reads. It never claims a USB interface, never runs
//! fastboot on its own and never restarts adb, because other tools may be
//! talking to the same player at the same time.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::adb::{Adb, AdbDevice, AdbState};
use crate::error::{Error, Result};
use crate::probe::{self, Snapshot};
use crate::profile;
use crate::usb::{self, Function, UsbDevice};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Link {
    Adb,
    AdbNetwork,
    Fastboot,
    MassStorage,
    Mtp,
    MtkBrom,
    MtkPreloader,
    NxpSerialDownload,
}

/// What the player is doing, as far as the connection reveals it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Booted with ADB answering.
    Running,
    /// ADB is there but the player has not accepted this computer yet.
    Unauthorized,
    /// ADB lists it offline: booting, or a stale connection.
    Offline,
    /// Booted with only USB storage or MTP, no ADB.
    StorageOnly,
    Bootloader,
    /// A boot ROM or preloader download mode: the rescue entry point.
    Download,
    /// USB descriptors exist but the host may not access the device.
    NoPermission,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownDevice {
    pub serial: String,
    pub profile_id: String,
    pub family: String,
    pub model: Option<String>,
    pub model_basis: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbLink {
    pub port: String,
    pub vid: u16,
    pub pid: u16,
    pub speed: Option<&'static str>,
    pub product: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSummary {
    pub id: String,
    pub serial: Option<String>,
    pub profile_id: Option<String>,
    pub family: Option<String>,
    pub model: Option<String>,
    pub model_basis: Option<String>,
    pub mode: Mode,
    pub links: Vec<Link>,
    pub usb: Option<UsbLink>,
    /// The name adb knows the player by (a serial, or host:port over Wi-Fi).
    pub adb_target: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    pub devices: Vec<DeviceSummary>,
    /// Problems that kept part of the scan from running, worded for people.
    pub issues: Vec<String>,
}

const IDENTIFY_SCRIPT: &str = "for k in ro.serialno ro.boot.serialno ro.hardware ro.boot.hardware ro.product.model ro.model.number; do echo \"[$k]: [$(getprop $k)]\"; done";

pub struct Registry {
    adb: Option<Adb>,
    known: BTreeMap<String, KnownDevice>,
    /// Network adb targets ("host:port") and the player serial behind each.
    aliases: HashMap<String, String>,
    /// adb targets that answered identification without matching a profile.
    foreign: HashMap<String, ()>,
}

impl Registry {
    pub fn new(adb: Option<Adb>, known: Vec<KnownDevice>) -> Self {
        Registry {
            adb,
            known: known.into_iter().map(|k| (k.serial.clone(), k)).collect(),
            aliases: HashMap::new(),
            foreign: HashMap::new(),
        }
    }

    pub fn set_adb(&mut self, adb: Option<Adb>) {
        self.adb = adb;
        self.aliases.clear();
        self.foreign.clear();
    }

    pub fn adb(&self) -> Option<&Adb> {
        self.adb.as_ref()
    }

    pub fn known(&self) -> Vec<KnownDevice> {
        self.known.values().cloned().collect()
    }

    pub fn scan(&mut self) -> Scan {
        let mut issues = Vec::new();
        let usb = usb::list().unwrap_or_else(|e| {
            issues.push(e.to_string());
            Vec::new()
        });
        let adb_devices = match &self.adb {
            Some(adb) => adb.devices().unwrap_or_else(|e| {
                issues.push(e.to_string());
                Vec::new()
            }),
            None => Vec::new(),
        };
        self.identify_unknown(&adb_devices);
        Scan {
            devices: self.merge(&usb, &adb_devices),
            issues,
        }
    }

    fn identify_unknown(&mut self, adb_devices: &[AdbDevice]) {
        let Some(adb) = self.adb.clone() else { return };
        for d in adb_devices.iter().filter(|d| d.state == AdbState::Device) {
            if self.known.contains_key(&d.serial)
                || self.aliases.contains_key(&d.serial)
                || self.foreign.contains_key(&d.serial)
            {
                continue;
            }
            let Ok(text) = adb.shell(&d.serial, IDENTIFY_SCRIPT, Duration::from_secs(4)) else {
                continue;
            };
            let props = probe::parse_props(&text);
            match profile::by_props(&props) {
                Some(p) => {
                    let serial = props
                        .get("ro.serialno")
                        .or_else(|| props.get("ro.boot.serialno"))
                        .filter(|s| !s.is_empty())
                        .cloned()
                        .unwrap_or_else(|| d.serial.clone());
                    self.known.entry(serial.clone()).or_insert(KnownDevice {
                        serial: serial.clone(),
                        profile_id: p.id.clone(),
                        family: p.family.clone(),
                        model: (p.variants.len() == 1).then(|| p.variants[0].model.clone()),
                        model_basis: (p.variants.len() == 1).then(|| "profile".to_owned()),
                    });
                    if serial != d.serial {
                        self.aliases.insert(d.serial.clone(), serial);
                    }
                }
                None => {
                    self.foreign.insert(d.serial.clone(), ());
                }
            }
        }
    }

    fn merge(&self, usb: &[UsbDevice], adb_devices: &[AdbDevice]) -> Vec<DeviceSummary> {
        let mut out: Vec<DeviceSummary> = Vec::new();
        for u in usb {
            let id = u.serial.clone().unwrap_or_else(|| format!("usb:{}", u.port));
            let mut links: Vec<Link> = u.functions.iter().map(|f| link_for(*f)).collect();
            let adb = adb_devices
                .iter()
                .find(|a| a.usb_port.as_deref() == Some(u.port.as_str()));
            if adb.is_none() {
                links.retain(|l| *l != Link::Adb);
            }
            out.push(DeviceSummary {
                id,
                serial: u.serial.clone(),
                profile_id: None,
                family: None,
                model: None,
                model_basis: None,
                mode: mode_for(&links, adb.map(|a| a.state)),
                links,
                usb: Some(UsbLink {
                    port: u.port.clone(),
                    vid: u.vid,
                    pid: u.pid,
                    speed: u.speed,
                    product: u.product.clone(),
                }),
                adb_target: adb.map(|a| a.serial.clone()),
            });
        }
        for a in adb_devices.iter().filter(|a| a.is_network()) {
            let Some(serial) = self.aliases.get(&a.serial) else {
                continue;
            };
            if let Some(existing) = out.iter_mut().find(|d| &d.id == serial) {
                existing.links.push(Link::AdbNetwork);
                if existing.adb_target.is_none() {
                    existing.adb_target = Some(a.serial.clone());
                    existing.mode = mode_for(&existing.links, Some(a.state));
                }
                continue;
            }
            out.push(DeviceSummary {
                id: serial.clone(),
                serial: Some(serial.clone()),
                profile_id: None,
                family: None,
                model: None,
                model_basis: None,
                mode: mode_for(&[Link::AdbNetwork], Some(a.state)),
                links: vec![Link::AdbNetwork],
                usb: None,
                adb_target: Some(a.serial.clone()),
            });
        }
        for d in &mut out {
            d.links.sort();
            d.links.dedup();
            if let Some(k) = self.known.get(&d.id) {
                d.profile_id = Some(k.profile_id.clone());
                d.family = Some(k.family.clone());
                d.model = k.model.clone();
                d.model_basis = k.model_basis.clone();
            } else if let Some(p) = d.usb.as_ref().and_then(|u| profile::by_usb(u.vid, u.pid)) {
                d.profile_id = Some(p.id.clone());
                d.family = Some(p.family.clone());
            }
        }
        out.retain(|d| {
            let foreign = d.adb_target.as_ref().is_some_and(|t| self.foreign.contains_key(t));
            let walkman_usb = d
                .usb
                .as_ref()
                .is_some_and(|u| matches!(u.vid, usb::VID_SONY | usb::VID_MEDIATEK | usb::VID_NXP));
            d.profile_id.is_some() || (walkman_usb && !foreign)
        });
        out
    }

    /// The adb handle and target to probe `device` with. Probing itself happens
    /// outside the registry so a slow player does not hold up scans.
    pub fn probe_target(&self, device: &DeviceSummary) -> Result<(Adb, String)> {
        let adb = self.adb.clone().ok_or(Error::ToolMissing { tool: "adb" })?;
        let target = device
            .adb_target
            .clone()
            .filter(|_| device.mode == Mode::Running)
            .ok_or_else(|| Error::NotReachable(device.model.clone().unwrap_or_else(|| device.id.clone())))?;
        Ok((adb, target))
    }

    /// Remembers what a snapshot revealed about the player's model. Returns true
    /// when that changed what the registry knew.
    pub fn record(&mut self, target: &str, snap: &Snapshot) -> bool {
        let (Some(profile_id), Some(family)) = (&snap.identity.profile_id, &snap.identity.family) else {
            return false;
        };
        let serial = snap.identity.serial.clone().unwrap_or_else(|| target.to_owned());
        if target != serial {
            self.aliases.insert(target.to_owned(), serial.clone());
        }
        let entry = KnownDevice {
            serial: serial.clone(),
            profile_id: profile_id.clone(),
            family: family.clone(),
            model: snap.identity.model.clone(),
            model_basis: snap.identity.model_basis.map(str::to_owned),
        };
        self.known.insert(serial, entry.clone()) != Some(entry)
    }
}

fn link_for(f: Function) -> Link {
    match f {
        Function::Adb => Link::Adb,
        Function::Fastboot => Link::Fastboot,
        Function::MassStorage => Link::MassStorage,
        Function::Mtp => Link::Mtp,
        Function::MtkBrom => Link::MtkBrom,
        Function::MtkPreloader => Link::MtkPreloader,
        Function::NxpSerialDownload => Link::NxpSerialDownload,
    }
}

fn mode_for(links: &[Link], adb: Option<AdbState>) -> Mode {
    if links
        .iter()
        .any(|l| matches!(l, Link::MtkBrom | Link::MtkPreloader | Link::NxpSerialDownload))
    {
        return Mode::Download;
    }
    if links.contains(&Link::Fastboot) {
        return Mode::Bootloader;
    }
    match adb {
        Some(AdbState::Device) => Mode::Running,
        Some(AdbState::Unauthorized) => Mode::Unauthorized,
        Some(AdbState::NoPermissions) => Mode::NoPermission,
        Some(AdbState::Bootloader) => Mode::Bootloader,
        Some(_) => Mode::Offline,
        None => Mode::StorageOnly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::parse_devices;

    fn usb_dev(port: &str, vid: u16, pid: u16, serial: Option<&str>, functions: &[Function]) -> UsbDevice {
        UsbDevice {
            port: port.into(),
            vid,
            pid,
            serial: serial.map(str::to_owned),
            manufacturer: Some("Sony".into()),
            product: Some("WALKMAN".into()),
            speed: Some("480 Mb/s"),
            functions: functions.to_vec(),
        }
    }

    fn known(serial: &str, profile: &str, model: &str) -> KnownDevice {
        KnownDevice {
            serial: serial.into(),
            profile_id: profile.into(),
            family: profile::get(profile).unwrap().family.clone(),
            model: Some(model.into()),
            model_basis: Some("storage".into()),
        }
    }

    #[test]
    fn both_players_merge_with_their_adb_entries() {
        let reg = Registry::new(None, vec![known("0000test0zx50700", "nw-zx500", "NW-ZX507")]);
        let usb = [
            usb_dev("3-1", 0x054c, 0x0cf2, Some("0000test0zx50700"), &[Function::Adb]),
            usb_dev(
                "3-2",
                0x054c,
                0x0bc1,
                Some("TESTA55SERIAL01"),
                &[Function::Adb, Function::MassStorage],
            ),
        ];
        let adb = parse_devices(
            "List of devices attached\n0000test0zx50700 device usb:3-1 transport_id:1\nTESTA55SERIAL01 device usb:3-2 transport_id:2\n192.168.1.9:5555 device product:gts9 model:SM_X716N\n",
        );
        let out = reg.merge(&usb, &adb);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].model.as_deref(), Some("NW-ZX507"));
        assert_eq!(out[0].mode, Mode::Running);
        assert_eq!(out[0].adb_target.as_deref(), Some("0000test0zx50700"));
        // Not yet probed: the family comes from the USB id table.
        assert_eq!(out[1].profile_id.as_deref(), Some("nw-a50"));
        assert_eq!(out[1].model, None);
        assert_eq!(out[1].links, [Link::Adb, Link::MassStorage]);
    }

    #[test]
    fn boot_rom_and_storage_only_modes() {
        let reg = Registry::new(None, vec![]);
        let usb = [
            usb_dev("1-4", 0x0e8d, 0x0003, None, &[Function::MtkBrom]),
            usb_dev(
                "1-5",
                0x054c,
                0x0bc1,
                Some("X"),
                &[Function::Adb, Function::MassStorage],
            ),
            usb_dev("1-6", 0x18d1, 0x4ee0, Some("PHONE"), &[Function::Fastboot]),
        ];
        let out = reg.merge(&usb, &[]);
        assert_eq!(out.len(), 2, "an unknown phone in fastboot is not listed");
        assert_eq!(out[0].id, "usb:1-4");
        assert_eq!(out[0].mode, Mode::Download);
        assert_eq!(out[0].profile_id.as_deref(), Some("nw-a50"));
        // ADB interface present but adb itself does not list it: storage only.
        assert_eq!(out[1].mode, Mode::StorageOnly);
        assert_eq!(out[1].links, [Link::MassStorage]);
    }

    #[test]
    fn known_player_in_fastboot_keeps_its_name() {
        let reg = Registry::new(None, vec![known("0000test0zx50700", "nw-zx500", "NW-ZX507")]);
        let usb = [usb_dev(
            "3-1",
            0x18d1,
            0x4ee0,
            Some("0000test0zx50700"),
            &[Function::Fastboot],
        )];
        let out = reg.merge(&usb, &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].mode, Mode::Bootloader);
        assert_eq!(out[0].model.as_deref(), Some("NW-ZX507"));
    }
}
