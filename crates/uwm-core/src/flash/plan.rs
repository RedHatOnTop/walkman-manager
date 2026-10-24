//! What an install will do to a player, from what the player says about itself: the stages in
//! order, the gates that must be open first, and the word the owner types to start it.
//!
//! The ZX500 sequence is the one walkman-zx507 ran by hand (its docs/install.md): unlocking
//! wipes the player and resets the A/B metadata to slot a, so a clean install sets slot b
//! active again after it, and `fastboot -w` formats userdata for the new system. Going back
//! to stock writes Sony's images to slot b the same way and formats userdata for Android 9.

use serde::Serialize;

use super::set::InstallSet;
use crate::studio::build::Variant;

/// Below this the bootloader's ten-minute erase could outlast the battery.
pub const CLEAN_BATTERY: u8 = 50;
pub const UPDATE_BATTERY: u8 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Unlock if needed, write the set, erase userdata: the player starts over.
    Clean,
    /// Write the set over an earlier one of the same kind and keep the owner's data.
    Update,
    /// Write a stock set and format userdata: Sony's firmware starts over. The bootloader
    /// stays unlocked.
    Restore,
}

impl Mode {
    /// A stock set only goes back, and going back takes a stock set.
    pub fn fits(self, set: &InstallSet) -> bool {
        (self == Mode::Restore) == (set.variant == Variant::Stock)
    }
}

/// What the player said before the install, over adb (or fastboot, which knows less).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub serial: String,
    pub profile: Option<String>,
    /// Booted with adb answering; false when it is already in fastboot.
    pub booted: bool,
    /// Sony's firmware version from the vendor side, "4.06".
    pub vendor_firmware: Option<String>,
    pub api_level: Option<u32>,
    pub slot: Option<String>,
    pub unlocked: Option<bool>,
    /// Developer options' "OEM unlocking" switch.
    pub oem_unlock_allowed: Option<bool>,
    /// The running system is a build of this kit, and which kind.
    pub running: Option<Variant>,
    pub battery: Option<u8>,
    /// A microSD card is in the player.
    pub card: bool,
    /// What this computer's fastboot can do.
    pub fastboot: Flasher,
}

/// The fastboot the install runs on this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Flasher {
    #[default]
    Missing,
    /// Writes images, but `-w` cannot make the userdata image with it.
    Writes,
    /// Also formats userdata.
    Formats,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Stage {
    RebootBootloader,
    /// `fastboot flashing unlock`: the bootloader erases userdata on its own, about ten
    /// minutes with only the logo on the screen.
    Unlock,
    Flash {
        partition: String,
        file: String,
        flags: Vec<String>,
        bytes: u64,
    },
    SetActive {
        slot: String,
    },
    /// `fastboot -w`.
    Wipe,
    Reboot,
    /// The player comes back on USB, in whatever mode.
    WaitBoot,
    /// adb answers again. After a clean install that takes the owner: setup, then USB
    /// debugging and this computer's approval.
    WaitAdb,
    Verify,
    /// `cmd package bg-dexopt-job` after an update. The odex PackageManager compiled into
    /// /data were made against the system before it, and a kit build keeps the GSI's
    /// fingerprint, so to Android the update is no OTA and nothing compiles them again until
    /// the next idle charge.
    Dexopt,
    /// One file of a storage set onto the WALKMAN drive.
    Copy {
        file: String,
        bytes: u64,
    },
    /// A file the drive already holds and keeps: the owner's data.
    Keep {
        file: String,
    },
    /// The drive mounted afresh and every copied file hashed from it.
    ReadBack {
        bytes: u64,
    },
    /// The picker's remembered choice set to Walkman, so the player comes back as the drive
    /// after the update.
    PickWalkman {
        file: String,
    },
    /// The drive unmounted and Sony's update command sent: the player restarts into its
    /// updater, which runs the UPG.
    Trigger,
    /// The updater ran and the player came back as the drive.
    WaitUpdate,
    /// The UPG's log on the drive holds the lines the kit expects.
    CheckLog,
}

impl Stage {
    /// How long the stage usually takes: unlock and erase about 570 s each (hand runs, devlog
    /// 2026-10-14); system_b at 22 MB/s and an update's boot in 78 s (the app, 2026-10-20).
    /// The WALKMAN drive's speeds are guesses for USB 2.0 until a run on the unit measures them.
    pub fn expected_secs(&self) -> Option<u64> {
        Some(match self {
            Stage::RebootBootloader => 15,
            Stage::Unlock => 600,
            Stage::Flash { bytes, .. } => 2 + bytes / 20_000_000,
            Stage::SetActive { .. } => 2,
            Stage::Wipe => 600,
            Stage::Reboot => 5,
            Stage::WaitBoot => 90,
            Stage::WaitAdb => return None,
            Stage::Verify => 10,
            // 58 s on the ZX507 with the kit's apps (devlog 2026-10-23).
            Stage::Dexopt => 60,
            Stage::Copy { bytes, .. } => 1 + bytes / 15_000_000,
            Stage::Keep { .. } | Stage::PickWalkman { .. } => 0,
            Stage::ReadBack { bytes } => 5 + bytes / 30_000_000,
            // pkexec waits for the owner.
            Stage::Trigger => return None,
            Stage::WaitUpdate => 150,
            Stage::CheckLog => 5,
        })
    }

    /// Stopping during this stage could leave the player without a system.
    pub fn uninterruptible(&self) -> bool {
        matches!(
            self,
            Stage::Unlock | Stage::Flash { .. } | Stage::Wipe | Stage::WaitUpdate
        )
    }
}

/// A condition the install needs. `ok` is None when the player cannot tell in its current
/// mode (fastboot knows no firmware version or battery level).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gate {
    pub id: &'static str,
    pub ok: Option<bool>,
    /// What the player reported, for the UI to show beside the gate.
    pub value: Option<String>,
    /// What it must be.
    pub want: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub mode: Mode,
    pub stages: Vec<Stage>,
    pub gates: Vec<Gate>,
    /// The owner's data on the player is erased.
    pub erases: bool,
    /// What the owner types to start: the partition the install cannot take back.
    pub confirm: String,
    /// No gate is closed. Unknown gates are the owner's to vouch for, except an update's two:
    /// what runs on the slot, and which kind it is, are known only while Android runs.
    pub ready: bool,
}

pub fn plan(set: &InstallSet, facts: &Facts, mode: Mode) -> Plan {
    let mut gates = vec![model_gate(set, facts), firmware_gate(set, facts)];
    let mut stages = Vec::new();
    if facts.booted {
        stages.push(Stage::RebootBootloader);
    }
    let flash = set.flash.iter().map(|f| Stage::Flash {
        partition: f.partition.clone(),
        file: f.file.clone(),
        flags: f.flags.clone(),
        bytes: set.files.get(&f.file).map_or(0, |s| s.bytes),
    });
    match mode {
        Mode::Clean => {
            if facts.unlocked != Some(true) {
                gates.push(Gate {
                    id: "oemUnlock",
                    ok: facts.booted.then_some(facts.oem_unlock_allowed == Some(true)),
                    value: facts.oem_unlock_allowed.map(|b| b.to_string()),
                    want: Some("true".into()),
                });
                stages.push(Stage::Unlock);
            }
            gates.push(battery_gate(facts, CLEAN_BATTERY));
            gates.push(card_gate(facts));
            let mut flash = flash.peekable();
            if let Some(first) =
                flash.next_if(|s| matches!(s, Stage::Flash { partition, .. } if partition.starts_with("vbmeta")))
            {
                stages.push(first);
            }
            stages.push(Stage::SetActive { slot: set.slot.clone() });
            stages.extend(flash);
            stages.push(Stage::Wipe);
        }
        Mode::Update => {
            let ok = if facts.booted {
                Some(
                    facts.running.is_some()
                        && facts.unlocked == Some(true)
                        && facts.slot.as_deref() == Some(set.slot.as_str()),
                )
            } else if facts.unlocked == Some(false) || facts.slot.as_deref().is_some_and(|s| s != set.slot) {
                // Fastboot knows the lock and the slot, not the system on it.
                Some(false)
            } else {
                None
            };
            gates.push(Gate {
                id: "runsKit",
                ok,
                value: facts.slot.clone(),
                want: Some(set.slot.clone()),
            });
            gates.push(Gate {
                id: "sameVariant",
                ok: facts.booted.then(|| facts.running == Some(set.variant)),
                value: facts.running.map(variant_word),
                want: Some(variant_word(set.variant)),
            });
            gates.push(battery_gate(facts, UPDATE_BATTERY));
            stages.extend(flash);
        }
        Mode::Restore => {
            // A locked bootloader takes no writes, and the player it locks already runs stock.
            gates.push(Gate {
                id: "unlocked",
                ok: facts.unlocked,
                value: facts.unlocked.map(|b| b.to_string()),
                want: Some("true".into()),
            });
            gates.push(battery_gate(facts, CLEAN_BATTERY));
            gates.push(card_gate(facts));
            stages.extend(flash);
            stages.push(Stage::SetActive { slot: set.slot.clone() });
            stages.push(Stage::Wipe);
        }
    }
    gates.push(fastboot_gate(facts, mode != Mode::Update));
    stages.extend([Stage::Reboot, Stage::WaitBoot, Stage::WaitAdb, Stage::Verify]);
    if mode == Mode::Update {
        stages.push(Stage::Dexopt);
    }
    let known = |id: &str| gates.iter().any(|g| g.id == id && g.ok == Some(true));
    let ready = mode.fits(set)
        && gates.iter().all(|g| g.ok != Some(false))
        && match mode {
            Mode::Clean => true,
            Mode::Update => known("runsKit") && known("sameVariant"),
            Mode::Restore => known("unlocked"),
        };
    Plan {
        mode,
        stages,
        gates,
        erases: mode != Mode::Update,
        confirm: match mode {
            Mode::Clean | Mode::Restore => "userdata".into(),
            Mode::Update => format!("system_{}", set.slot),
        },
        ready,
    }
}

fn variant_word(v: Variant) -> String {
    match v {
        Variant::Release => "release".into(),
        Variant::Debug => "debug".into(),
        Variant::Stock => "stock".into(),
    }
}

fn model_gate(set: &InstallSet, facts: &Facts) -> Gate {
    Gate {
        id: "model",
        ok: Some(facts.profile.as_deref() == Some(profile_for(set))),
        value: facts.profile.clone(),
        want: Some(profile_for(set).to_owned()),
    }
}

/// The player profile a set's stock firmware is for.
fn profile_for(set: &InstallSet) -> &'static str {
    match set.model.as_deref() {
        Some("NW-ZX500") => "nw-zx500",
        _ => "unknown",
    }
}

fn firmware_gate(set: &InstallSet, facts: &Facts) -> Gate {
    let want = set.firmware_line().map(str::to_owned);
    Gate {
        id: "firmware",
        ok: facts
            .booted
            .then(|| want.is_some() && facts.vendor_firmware.as_deref() == want.as_deref()),
        value: facts.vendor_firmware.clone(),
        want,
    }
}

fn battery_gate(facts: &Facts, least: u8) -> Gate {
    Gate {
        id: "battery",
        ok: facts.battery.map(|b| b >= least),
        value: facts.battery.map(|b| b.to_string()),
        want: Some(least.to_string()),
    }
}

fn card_gate(facts: &Facts) -> Gate {
    Gate {
        id: "card",
        ok: facts.booted.then_some(!facts.card),
        value: None,
        want: None,
    }
}

/// Every install writes with the computer's fastboot; the two that erase also format
/// userdata with it, and one that cannot would stop with the player's data already gone.
fn fastboot_gate(facts: &Facts, formats: bool) -> Gate {
    let word = |f: Flasher| match f {
        Flasher::Missing => "missing",
        Flasher::Writes => "writes",
        Flasher::Formats => "formats",
    };
    let want = if formats { Flasher::Formats } else { Flasher::Writes };
    Gate {
        id: "fastboot",
        ok: Some(facts.fastboot == Flasher::Formats || facts.fastboot == want),
        value: Some(word(facts.fastboot).into()),
        want: Some(word(want).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> InstallSet {
        serde_json::from_str(include_str!("../../tests/fixtures/install.json")).unwrap()
    }

    fn stock_set() -> InstallSet {
        serde_json::from_str(include_str!("../../tests/fixtures/restore.json")).unwrap()
    }

    fn stock() -> Facts {
        Facts {
            serial: "0000test0zx50700".into(),
            profile: Some("nw-zx500".into()),
            booted: true,
            vendor_firmware: Some("4.06".into()),
            api_level: Some(28),
            slot: Some("b".into()),
            unlocked: Some(false),
            oem_unlock_allowed: Some(true),
            running: None,
            battery: Some(80),
            card: false,
            fastboot: Flasher::Formats,
        }
    }

    fn kinds(p: &Plan) -> Vec<String> {
        p.stages
            .iter()
            .map(|s| match s {
                Stage::Flash { partition, .. } => partition.clone(),
                Stage::SetActive { slot } => format!("active {slot}"),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_locked_stock_player_is_unlocked_then_written_then_erased() {
        let p = plan(&set(), &stock(), Mode::Clean);
        assert_eq!(
            kinds(&p),
            [
                "RebootBootloader",
                "Unlock",
                "vbmeta_b",
                "active b",
                "boot_b",
                "system_b",
                "Wipe",
                "Reboot",
                "WaitBoot",
                "WaitAdb",
                "Verify"
            ]
        );
        assert!(p.ready && p.erases);
        assert_eq!(p.confirm, "userdata");
    }

    #[test]
    fn closed_gates_hold_a_clean_install() {
        let mut f = stock();
        f.oem_unlock_allowed = Some(false);
        f.card = true;
        f.battery = Some(31);
        f.vendor_firmware = Some("4.04".into());
        let p = plan(&set(), &f, Mode::Clean);
        let closed: Vec<&str> = p.gates.iter().filter(|g| g.ok == Some(false)).map(|g| g.id).collect();
        assert_eq!(closed, ["firmware", "oemUnlock", "battery", "card"]);
        assert!(!p.ready);
    }

    #[test]
    fn an_unlocked_player_in_fastboot_skips_the_unlock_and_the_reboot() {
        let f = Facts {
            serial: "0000test0zx50700".into(),
            profile: Some("nw-zx500".into()),
            unlocked: Some(true),
            fastboot: Flasher::Formats,
            ..Facts::default()
        };
        let p = plan(&set(), &f, Mode::Clean);
        assert_eq!(kinds(&p)[..2], ["vbmeta_b", "active b"]);
        let unknown: Vec<&str> = p.gates.iter().filter(|g| g.ok.is_none()).map(|g| g.id).collect();
        assert_eq!(unknown, ["firmware", "battery", "card"]);
        assert!(p.ready);
    }

    #[test]
    fn an_update_keeps_data_and_needs_the_same_kind_of_build() {
        let mut f = stock();
        f.unlocked = Some(true);
        f.api_level = Some(33);
        f.running = Some(Variant::Debug);
        let p = plan(&set(), &f, Mode::Update);
        assert_eq!(
            kinds(&p),
            [
                "RebootBootloader",
                "vbmeta_b",
                "boot_b",
                "system_b",
                "Reboot",
                "WaitBoot",
                "WaitAdb",
                "Verify",
                "Dexopt"
            ]
        );
        assert!(!p.erases);
        assert_eq!(p.confirm, "system_b");
        let closed: Vec<&str> = p.gates.iter().filter(|g| g.ok == Some(false)).map(|g| g.id).collect();
        assert_eq!(closed, ["sameVariant"], "a release set over a debug build");
        f.running = Some(Variant::Release);
        assert!(plan(&set(), &f, Mode::Update).ready);
        f.running = None;
        assert!(
            !plan(&set(), &f, Mode::Update).ready,
            "stock is not updated, it is installed"
        );
    }

    #[test]
    fn an_update_from_fastboot_waits_for_android() {
        let mut f = Facts {
            serial: "0000test0zx50700".into(),
            profile: Some("nw-zx500".into()),
            unlocked: Some(true),
            slot: Some("b".into()),
            fastboot: Flasher::Formats,
            ..Facts::default()
        };
        let p = plan(&set(), &f, Mode::Update);
        let unknown: Vec<&str> = p.gates.iter().filter(|g| g.ok.is_none()).map(|g| g.id).collect();
        assert_eq!(unknown, ["firmware", "runsKit", "sameVariant", "battery"]);
        assert!(!p.ready, "nothing says slot b holds this kit's Android");
        f.slot = Some("a".into());
        let p = plan(&set(), &f, Mode::Update);
        assert_eq!(p.gates.iter().find(|g| g.id == "runsKit").unwrap().ok, Some(false));
    }

    #[test]
    fn going_back_writes_stock_to_slot_b_then_formats_userdata() {
        let mut f = stock();
        f.unlocked = Some(true);
        f.api_level = Some(33);
        f.running = Some(Variant::Release);
        let p = plan(&stock_set(), &f, Mode::Restore);
        assert_eq!(
            kinds(&p),
            [
                "RebootBootloader",
                "vbmeta_b",
                "boot_b",
                "system_b",
                "vendor_b",
                "active b",
                "Wipe",
                "Reboot",
                "WaitBoot",
                "WaitAdb",
                "Verify"
            ]
        );
        assert!(p.ready && p.erases);
        assert_eq!(p.confirm, "userdata");
        assert!(!p.stages.contains(&Stage::Unlock));

        f.unlocked = Some(false);
        let p = plan(&stock_set(), &f, Mode::Restore);
        assert_eq!(p.gates.iter().find(|g| g.id == "unlocked").unwrap().ok, Some(false));
        assert!(!p.ready, "a locked player already runs stock");
        f.unlocked = None;
        assert!(
            !plan(&stock_set(), &f, Mode::Restore).ready,
            "writes need an unlock that is known"
        );
    }

    #[test]
    fn going_back_reads_sony_firmware_from_the_vendor_side() {
        let mut f = stock();
        f.unlocked = Some(true);
        f.api_level = Some(33);
        f.running = Some(Variant::Release);
        let firmware = |f: &Facts| {
            plan(&stock_set(), f, Mode::Restore)
                .gates
                .into_iter()
                .find(|g| g.id == "firmware")
                .unwrap()
        };
        assert_eq!(firmware(&f).ok, Some(true), "the kit's Android keeps Sony's vendor");
        assert_eq!(firmware(&f).want.as_deref(), Some("4.06"));
        f.vendor_firmware = Some("4.04".into());
        assert_eq!(firmware(&f).ok, Some(false));
        assert!(!plan(&stock_set(), &f, Mode::Restore).ready);
    }

    #[test]
    fn the_installs_that_erase_take_a_fastboot_that_formats() {
        let closed =
            |p: &Plan| -> Vec<&'static str> { p.gates.iter().filter(|g| g.ok == Some(false)).map(|g| g.id).collect() };
        let mut f = stock();
        f.fastboot = Flasher::Writes;
        let p = plan(&set(), &f, Mode::Clean);
        assert_eq!(closed(&p), ["fastboot"]);
        assert!(!p.ready);
        let gate = p.gates.iter().find(|g| g.id == "fastboot").unwrap();
        assert_eq!(
            (gate.value.as_deref(), gate.want.as_deref()),
            (Some("writes"), Some("formats"))
        );

        f.unlocked = Some(true);
        f.running = Some(Variant::Release);
        assert!(!plan(&stock_set(), &f, Mode::Restore).ready);
        assert!(plan(&set(), &f, Mode::Update).ready, "an update formats nothing");
        f.fastboot = Flasher::Missing;
        assert_eq!(closed(&plan(&set(), &f, Mode::Update)), ["fastboot"]);
    }

    #[test]
    fn a_stock_set_only_goes_back() {
        let mut f = stock();
        f.unlocked = Some(true);
        f.running = Some(Variant::Release);
        assert!(Mode::Restore.fits(&stock_set()) && !Mode::Restore.fits(&set()));
        assert!(!Mode::Clean.fits(&stock_set()) && !Mode::Update.fits(&stock_set()));
        assert!(!plan(&stock_set(), &f, Mode::Clean).ready);
        assert!(!plan(&set(), &f, Mode::Restore).ready);
    }
}
