//! `install.json` of a kit whose set is copied onto the player's own storage, the WALKMAN drive
//! of an NW-A50 series player, and taken in by a UPG the player's updater runs: what goes where
//! in which order, the hashes, the UPG and the lines its log must hold afterwards.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use super::drive::Drive;
use super::plan::{Gate, Mode, Plan, Stage};
use super::set::{SetFile, check_files};
use super::verify::FileVerdict;
use crate::{Error, Result, Why};

/// The picker's remembered choice, beside the boot set: "1" starts Walkman, "2" Android.
pub const CHOICE: &str = "wmmod-android/boot-choice";
/// The backup backup-upg writes to the drive.
pub const BACKUP_MANIFEST: &str = "WMBACKUP/manifest.txt";
/// The one file of the player's system the picker UPG replaces; Sony's copy stays beside it.
pub const HOOK: &str = "bootswitcher.sh";
/// Room beyond the set's own, for the UPG's log and the file system's bookkeeping.
const SPARE: u64 = 16 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageSet {
    pub kit: String,
    /// The kit's commit, when the copy it ran from knew it.
    pub commit: Option<String>,
    /// The kit ran from a working tree with changes no commit holds.
    pub dirty: bool,
    /// The Sony build the kit took the owner's files from, as the kit's pins name it.
    pub firmware: String,
    /// Every file the set copies, by its path on the drive.
    pub files: BTreeMap<String, SetFile>,
    /// The order they go onto the drive: the kit puts the file that makes a half-copied set
    /// refuse to start after the rest, and the UPG last.
    pub copy: Vec<String>,
    /// Files a player that already has one keeps: the owner's data.
    #[serde(default)]
    pub keep: Vec<String>,
    pub upg: Upg,
    pub after: StorageAfter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Upg {
    /// The UPG's path on the drive; the updater looks for it at the root.
    pub file: String,
    /// The key the player's updater takes, read from the owner's own backup.
    pub key: UpgKey,
    /// sha256 of the recovery image the kit read from the backup: the image the trigger boots.
    pub recovery: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpgKey {
    /// Sony's own updater.
    Stock,
    /// Walkman One's, which swaps the updater's key for its own.
    W1,
}

/// What the drive shows once the UPG ran: its log at the root and lines that log must hold.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageAfter {
    pub log: String,
    pub expect: Vec<String>,
}

impl StorageSet {
    /// Every path stays on the drive: relative, no `..`, no backslash. The UPG and its log sit at
    /// the root, where the updater reads and writes them. The set copies exactly the files it
    /// describes, and keeps only files it copies.
    pub fn validate(&self) -> Result<()> {
        let bad = |what: String| {
            Err(Error::unsupported(Why::BadInstallSet, format!("install.json: {what}")).with_detail(what))
        };
        for path in self.copy.iter().chain(self.files.keys()) {
            if !on_drive(path) {
                return bad(format!("path {path:?}"));
            }
        }
        let named: BTreeSet<&String> = self.copy.iter().collect();
        if named.len() != self.copy.len()
            || self.copy.len() != self.files.len()
            || self.copy.iter().any(|p| !self.files.contains_key(p))
        {
            return bad("copy and files name different files".into());
        }
        if let Some(k) = self.keep.iter().find(|k| !self.copy.contains(k)) {
            return bad(format!("keeps {k:?}, which it does not copy"));
        }
        let at_root = |p: &str| on_drive(p) && !p.contains('/');
        if !at_root(&self.upg.file) || !self.upg.file.ends_with(".UPG") || self.copy.last() != Some(&self.upg.file) {
            return bad(format!("UPG {:?}", self.upg.file));
        }
        if !at_root(&self.after.log) || self.after.expect.is_empty() {
            return bad(format!("log {:?}", self.after.log));
        }
        Ok(())
    }

    /// Hashes every file the set copies and checks it against what the kit wrote, so what is
    /// copied is what was built. Returns each path and hash, in copy order.
    pub fn check_files(&self, dir: &Path, on_progress: &mut dyn FnMut(u64, u64)) -> Result<Vec<(String, String)>> {
        let names: Vec<&str> = self.copy.iter().map(String::as_str).collect();
        check_files(dir, &names, &self.files, "copies", on_progress)
    }

    /// Bytes the drive needs free, besides what kept files already take there.
    pub fn bytes(&self) -> u64 {
        self.files.values().map(|f| f.bytes).sum()
    }

    fn bytes_of(&self, path: &str) -> u64 {
        self.files.get(path).map_or(0, |f| f.bytes)
    }

    /// The lines the kit expects in the UPG's log, and whether what this run appended holds
    /// each.
    pub fn judge_log(&self, appended: &str) -> Vec<(String, bool)> {
        self.after
            .expect
            .iter()
            .map(|want| (want.clone(), appended.lines().any(|l| l.trim() == want)))
            .collect()
    }
}

/// What the player and its drive say before a storage install.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFacts {
    pub serial: String,
    pub profile: Option<String>,
    /// The WALKMAN drive, mounted, when this computer sees it.
    pub drive: Option<Drive>,
    pub free: Option<u64>,
    /// Bytes each of the set's paths already takes on the drive.
    pub present: BTreeMap<String, u64>,
    /// The picker's remembered choice on the drive.
    pub choice: Option<String>,
    /// sha256 of the backup manifest on the drive, when there is one.
    pub backup_on_drive: Option<String>,
    /// sha256 of the backup manifest the build was made from.
    pub backup_built: Option<String>,
    /// pkexec is there to send the trigger with.
    pub elevate: bool,
}

/// What a storage install will do: copy the set in its order, or keep what an update keeps;
/// read every copied file back from a fresh mount; have the picker start Walkman once; send
/// the trigger; wait out the updater; read its log.
pub fn plan(set: &StorageSet, facts: &DriveFacts, mode: Mode, profile: &str) -> Plan {
    let has = |p: &str| facts.present.contains_key(p);
    let kept = |p: &str| mode == Mode::Update && set.keep.iter().any(|k| k == p) && has(p);
    let copied: Vec<&str> = set.copy.iter().map(String::as_str).filter(|p| !kept(p)).collect();
    let mut stages: Vec<Stage> = set
        .copy
        .iter()
        .map(|p| {
            if kept(p) {
                Stage::Keep { file: p.clone() }
            } else {
                Stage::Copy {
                    file: p.clone(),
                    bytes: set.bytes_of(p),
                }
            }
        })
        .collect();
    stages.push(Stage::ReadBack {
        bytes: copied.iter().map(|p| set.bytes_of(p)).sum(),
    });
    if facts.choice.as_deref().is_some_and(|c| c.trim() != "1") {
        stages.push(Stage::PickWalkman { file: CHOICE.into() });
    }
    stages.extend([Stage::Trigger, Stage::WaitUpdate, Stage::CheckLog]);

    let mounted = facts.drive.as_ref().is_some_and(|d| d.bytes > 0 && d.mount.is_some());
    let need = room(set, facts, &copied);
    let mut gates = vec![
        Gate {
            id: "model",
            ok: Some(facts.profile.as_deref() == Some(profile)),
            value: facts.profile.clone(),
            want: Some(profile.into()),
        },
        Gate {
            id: "drive",
            ok: Some(mounted),
            value: facts.drive.as_ref().map(|d| d.block.to_string_lossy().into_owned()),
            want: None,
        },
        Gate {
            id: "room",
            ok: facts.free.map(|f| f >= need),
            value: facts.free.map(|f| f.to_string()),
            want: Some(need.to_string()),
        },
        // A backup on the drive other than the one the build read is a newer one: the build
        // would put back an older recovery's key and Sony files.
        Gate {
            id: "backup",
            ok: facts
                .backup_on_drive
                .as_ref()
                .map(|b| Some(b) == facts.backup_built.as_ref()),
            value: None,
            want: None,
        },
        Gate {
            id: "elevate",
            ok: Some(facts.elevate),
            value: None,
            want: None,
        },
    ];
    if mode == Mode::Update {
        gates.push(Gate {
            id: "hasData",
            ok: Some(!set.keep.is_empty() && set.keep.iter().all(|k| has(k))),
            value: None,
            want: None,
        });
    }
    let erased = (mode == Mode::Clean)
        .then(|| set.keep.iter().find(|k| has(k)))
        .flatten();
    Plan {
        mode,
        stages,
        ready: mode != Mode::Restore && mounted && facts.free.is_some() && gates.iter().all(|g| g.ok != Some(false)),
        gates,
        erases: erased.is_some(),
        confirm: erased.map_or(HOOK, |k| k.rsplit('/').next().unwrap_or(k)).to_owned(),
    }
}

/// The most the drive holds at once beyond what it has now: each file goes to a temporary
/// name before the old one gives way.
fn room(set: &StorageSet, facts: &DriveFacts, copied: &[&str]) -> u64 {
    let (mut grown, mut peak) = (0i64, 0i64);
    for p in copied {
        let new = set.bytes_of(p) as i64;
        let old = facts.present.get(*p).copied().unwrap_or(0) as i64;
        peak = peak.max(grown + new);
        grown += new - old;
    }
    peak as u64 + SPARE
}

/// After a storage install: the copied files as read back, and what the UPG's log says.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveVerdict {
    pub files: Vec<FileVerdict>,
    /// The lines the kit expects in the UPG's log, and whether this run wrote each.
    pub lines: Vec<(String, bool)>,
    /// The updater renamed the UPG to `.done`, which it does once it ran it.
    pub upg_done: Option<bool>,
}

impl DriveVerdict {
    /// The UPG's own `.done` counts too: a drive read while the updater still ran holds a log
    /// that may already show this run's lines.
    pub fn ok(&self) -> bool {
        self.files.iter().all(|f| f.ok)
            && !self.lines.is_empty()
            && self.lines.iter().all(|(_, ok)| *ok)
            && self.upg_done == Some(true)
    }
}

fn on_drive(path: &str) -> bool {
    !path.is_empty() && !path.contains('\\') && Path::new(path).components().all(|c| matches!(c, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    fn good() -> StorageSet {
        serde_json::from_str(include_str!("../../tests/fixtures/storage.json")).unwrap()
    }

    #[test]
    fn reads_the_a50_kit_install_json() {
        let set = good();
        set.validate().unwrap();
        assert_eq!(set.upg.key, UpgKey::W1);
        assert_eq!(set.copy.last().map(String::as_str), Some("NW_WM_FW.UPG"));
        assert_eq!(set.keep, ["wmmod-android/android-data.img"]);
        assert_eq!(set.after.log, "PICKERUPG.log");
    }

    #[test]
    fn refuses_paths_off_the_drive_and_a_upg_not_last() {
        let refused = |change: &dyn Fn(&mut StorageSet)| {
            let mut s = good();
            change(&mut s);
            s.validate().is_err()
        };
        fn moved(from: &'static str, to: &'static str) -> impl Fn(&mut StorageSet) {
            move |s| {
                let f = s.files.remove(from).unwrap();
                s.files.insert(to.into(), f);
                for p in s.copy.iter_mut().filter(|p| *p == from) {
                    *p = to.into();
                }
            }
        }
        assert!(refused(&moved("wmmod-android/boot/pivotinit", "../pivotinit")));
        assert!(refused(&moved("wmmod-android/boot/pivotinit", "/etc/pivotinit")));
        assert!(refused(&moved(
            "wmmod-android/boot/pivotinit",
            "wmmod-android\\boot\\pivotinit"
        )));
        assert!(refused(&moved("NW_WM_FW.UPG", "wmmod-android/NW_WM_FW.UPG")));
        assert!(refused(&|s| s.copy.rotate_right(1)), "the UPG goes last");
        assert!(refused(&|s| {
            s.copy.pop();
        }));
        assert!(
            refused(&|s| s.copy[1] = s.copy[0].clone()),
            "one file twice, another never"
        );
        assert!(refused(&|s| s.keep.push("elsewhere.img".into())));
        assert!(refused(&|s| s.after.log = "../PICKERUPG.log".into()));
        assert!(refused(&|s| s.after.expect.clear()));
    }

    const DATA: &str = "wmmod-android/android-data.img";

    fn fresh() -> DriveFacts {
        DriveFacts {
            serial: "TESTA55SERIAL01".into(),
            profile: Some("nw-a50".into()),
            drive: Some(Drive {
                disk: "/dev/sdb".into(),
                block: "/dev/sdb".into(),
                serial: Some("TESTA55SERIAL01".into()),
                label: Some("WALKMAN".into()),
                bytes: 12_855_712_768,
                mount: Some("/run/media/owner/WALKMAN".into()),
            }),
            free: Some(10_000_000_000),
            backup_on_drive: Some("ab".repeat(32)),
            backup_built: Some("ab".repeat(32)),
            elevate: true,
            ..DriveFacts::default()
        }
    }

    fn installed() -> DriveFacts {
        let mut f = fresh();
        for p in good().copy {
            f.present.insert(p.clone(), good().bytes_of(&p));
        }
        f.choice = Some("2\n".into());
        f
    }

    fn kinds(p: &Plan) -> Vec<String> {
        p.stages
            .iter()
            .map(|s| match s {
                Stage::Copy { file, .. } => format!("copy {}", file.rsplit('/').next().unwrap()),
                Stage::Keep { file } => format!("keep {}", file.rsplit('/').next().unwrap()),
                other => format!("{other:?}").split([' ', '{']).next().unwrap().to_owned(),
            })
            .collect()
    }

    fn closed(p: &Plan) -> Vec<&'static str> {
        p.gates.iter().filter(|g| g.ok == Some(false)).map(|g| g.id).collect()
    }

    #[test]
    fn a_fresh_player_gets_every_file_then_the_upg() {
        let p = plan(&good(), &fresh(), Mode::Clean, "nw-a50");
        assert_eq!(
            kinds(&p),
            [
                "copy android-boot.sh",
                "copy pivotinit",
                "copy wlan_mt_bp.ko",
                "copy MANIFEST",
                "copy fire-system.img",
                "copy android-data.img",
                "copy NW_WM_FW.UPG",
                "ReadBack",
                "Trigger",
                "WaitUpdate",
                "CheckLog"
            ]
        );
        assert!(p.ready && !p.erases, "{:?}", p.gates);
        assert_eq!(p.confirm, "bootswitcher.sh");
        assert!(matches!(p.stages[7], Stage::ReadBack { bytes } if bytes == good().bytes()));
    }

    #[test]
    fn an_update_keeps_android_data_and_has_the_picker_start_walkman() {
        let p = plan(&good(), &installed(), Mode::Update, "nw-a50");
        assert_eq!(kinds(&p)[5], "keep android-data.img");
        assert_eq!(kinds(&p)[8..], ["PickWalkman", "Trigger", "WaitUpdate", "CheckLog"]);
        assert!(p.ready && !p.erases);
        assert!(
            matches!(p.stages[7], Stage::ReadBack { bytes } if bytes == good().bytes() - good().bytes_of(DATA)),
            "the kept image is not read back"
        );

        let mut f = installed();
        f.choice = Some("1".into());
        assert!(!kinds(&plan(&good(), &f, Mode::Update, "nw-a50")).contains(&"PickWalkman".to_owned()));
    }

    #[test]
    fn a_clean_install_over_android_data_erases_it_and_names_it() {
        let p = plan(&good(), &installed(), Mode::Clean, "nw-a50");
        assert_eq!(kinds(&p)[5], "copy android-data.img");
        assert!(p.erases && p.ready);
        assert_eq!(p.confirm, "android-data.img");
    }

    #[test]
    fn an_update_needs_data_to_keep() {
        let p = plan(&good(), &fresh(), Mode::Update, "nw-a50");
        assert_eq!(closed(&p), ["hasData"]);
        assert!(!p.ready);
    }

    #[test]
    fn closed_gates_hold_a_storage_install() {
        let mut f = fresh();
        f.profile = Some("nw-zx500".into());
        f.free = Some(1_000_000_000);
        f.backup_on_drive = Some("cd".repeat(32));
        f.elevate = false;
        let p = plan(&good(), &f, Mode::Clean, "nw-a50");
        assert_eq!(closed(&p), ["model", "room", "backup", "elevate"]);
        assert!(!p.ready);

        let mut f = fresh();
        f.drive.as_mut().unwrap().mount = None;
        f.free = None;
        f.backup_on_drive = None;
        let p = plan(&good(), &f, Mode::Clean, "nw-a50");
        assert_eq!(closed(&p), ["drive"]);
        let backup = p.gates.iter().find(|g| g.id == "backup").unwrap();
        assert_eq!(
            backup.ok, None,
            "a backup moved off the drive is the owner's to vouch for"
        );
        assert!(!p.ready);
    }

    #[test]
    fn room_counts_the_temporary_copy_beside_the_old_one() {
        let set = good();
        let mut f = installed();
        let need = |f: &DriveFacts| -> u64 {
            plan(&set, f, Mode::Update, "nw-a50")
                .gates
                .into_iter()
                .find(|g| g.id == "room")
                .unwrap()
                .want
                .unwrap()
                .parse()
                .unwrap()
        };
        assert_eq!(need(&f), set.bytes_of("wmmod-android/fire-system.img") + SPARE);
        f.present.clear();
        assert_eq!(need(&f), set.bytes() + SPARE);
    }

    #[test]
    fn the_log_must_hold_every_line_this_run_wrote() {
        let set = good();
        let run = "====== PICKER UPG START ======\nhook md5 9e86b9772b897b2c467685f427c0d474\n====== PICKER UPG DONE ======\n";
        assert!(set.judge_log(run).iter().all(|(_, ok)| *ok));
        let v = DriveVerdict {
            files: Vec::new(),
            lines: set.judge_log("====== PICKER UPG START ======\nmount /emmc@android failed; nothing written\n"),
            upg_done: Some(true),
        };
        assert!(!v.ok());
        let early = DriveVerdict {
            files: Vec::new(),
            lines: set.judge_log(run),
            upg_done: Some(false),
        };
        assert!(!early.ok(), "the log, but the UPG not yet renamed");
        assert!(
            DriveVerdict {
                upg_done: Some(true),
                ..early
            }
            .ok()
        );
        assert!(!DriveVerdict::default().ok(), "no log read is no verdict");
    }

    #[test]
    fn checks_the_files_it_copies() {
        let dir = std::env::temp_dir().join(format!("uwm-storage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut set = good();
        for path in set.copy.clone() {
            let body = path.repeat(3);
            std::fs::create_dir_all(dir.join(&path).parent().unwrap()).unwrap();
            std::fs::write(dir.join(&path), &body).unwrap();
            set.files.insert(
                path,
                SetFile {
                    bytes: body.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(body.as_bytes())),
                },
            );
        }
        let checked = set.check_files(&dir, &mut |_, _| {}).unwrap();
        assert_eq!(checked.len(), set.copy.len());
        assert_eq!(checked.last().unwrap().0, "NW_WM_FW.UPG");
        std::fs::write(dir.join("wmmod-android/boot/MANIFEST"), "torn").unwrap();
        assert!(matches!(
            set.check_files(&dir, &mut |_, _| {}).unwrap_err(),
            Error::Unsupported {
                why: Why::SetChanged,
                ..
            }
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
