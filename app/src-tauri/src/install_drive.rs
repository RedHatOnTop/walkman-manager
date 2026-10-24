//! Installing a storage set on an NW-A50 series player: every file onto its WALKMAN drive in the
//! kit's order, read back from a fresh mount, then Sony's update command, which has the player
//! restart into its updater and run the kit's UPG. The job waits for the drive to come back and
//! reads what the UPG logged there. Like a fastboot install it reports through the app's one
//! install job.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use uwm_core::flash::drive::{self, Drive};
use uwm_core::flash::plan::{Mode, Stage};
use uwm_core::flash::storage::{self, BACKUP_MANIFEST, CHOICE};
use uwm_core::flash::verify::FileVerdict;
use uwm_core::flash::{DriveFacts, DriveVerdict, StorageSet, upg};
use uwm_core::library::pull::free_bytes;
use uwm_core::studio::build::Record;
use uwm_core::studio::catalog;
use uwm_core::{Error, ErrorReport, Why, usb};

use crate::devices::DeviceService;
use crate::install::{self, End, InstallService, JobView, PlanView, Seen, err};

const POLL: Duration = Duration::from_millis(1500);
/// How long a drive that came back may take to be mounted.
const SETTLE: Duration = Duration::from_secs(30);

fn profile(set: &StorageSet) -> &'static str {
    catalog::kit(&set.kit).map_or("unknown", |k| k.profile.as_str())
}

/// A storage set installs fresh or over an earlier one; there is no stock set to go back with.
fn fits(mode: Mode) -> Result<(), ErrorReport> {
    if mode == Mode::Restore {
        return Err(ErrorReport::refused(
            "mode",
            Why::ModeNotForSet,
            "a storage set does not install as Restore",
        ));
    }
    Ok(())
}

/// What the player and its drive say. The drive is mounted, as the desktop would on its own, to
/// see what it holds and how much room it has.
fn facts(app: &AppHandle, device: &str, set: &StorageSet, record: &Record) -> Result<DriveFacts, ErrorReport> {
    let summary = app
        .state::<DeviceService>()
        .summary(device)
        .ok_or_else(|| err(Error::NoSuchDevice(device.to_owned())))?;
    let serial = summary
        .serial
        .clone()
        .ok_or_else(|| err(Error::NotReachable(device.to_owned())))?;
    let mut facts = DriveFacts {
        profile: summary.profile_id.clone(),
        backup_built: record
            .inputs
            .iter()
            .find(|i| i.input == "backup")
            .map(|i| i.sha256.clone()),
        elevate: upg::can_elevate(),
        drive: drive::for_serial(&serial).map_err(err)?,
        serial,
        ..DriveFacts::default()
    };
    if let Some(d) = facts.drive.as_mut().filter(|d| d.bytes > 0) {
        let mount = d.mount().map_err(err)?;
        facts.free = free_bytes(&mount);
        for p in &set.copy {
            if let Some(n) = drive::size(&mount, p) {
                facts.present.insert(p.clone(), n);
            }
        }
        facts.choice = std::fs::read_to_string(mount.join(CHOICE)).ok();
        if drive::size(&mount, BACKUP_MANIFEST).is_some() {
            let never = std::sync::atomic::AtomicBool::new(false);
            facts.backup_on_drive = Some(drive::hash(&mount, BACKUP_MANIFEST, &never, &mut |_| {}).map_err(err)?);
        }
    }
    Ok(facts)
}

pub(crate) fn plan_view(
    app: &AppHandle,
    device: &str,
    record: Record,
    set: StorageSet,
    mode: Mode,
) -> Result<PlanView, ErrorReport> {
    fits(mode)?;
    let facts = facts(app, device, &set, &record)?;
    Ok(PlanView {
        plan: storage::plan(&set, &facts, mode, profile(&set)),
        facts: None,
        drive: Some(facts),
        set: None,
        storage: Some(set),
        build: record,
        stock: None,
    })
}

pub(crate) fn start(
    app: &AppHandle,
    device: String,
    record: Record,
    set: StorageSet,
    mode: Mode,
    confirm: &str,
) -> Result<(), ErrorReport> {
    fits(mode)?;
    let facts = facts(app, &device, &set, &record)?;
    let plan = storage::plan(&set, &facts, mode, profile(&set));
    install::may_start(&plan, confirm)?;
    let Some(drive) = facts.drive.clone() else {
        return Err(err(Error::unsupported(
            Why::NoDrive,
            "the WALKMAN drive is not on this computer",
        )));
    };
    install::begin(
        app,
        JobView::new(
            record.id.clone(),
            device,
            facts.serial.clone(),
            mode,
            record.options.variant,
            &plan,
        ),
    );
    let worker = app.clone();
    thread::Builder::new()
        .name("install".into())
        .spawn(move || {
            let mut run = Run {
                app: &worker,
                serial: facts.serial.clone(),
                dir: record.set_dir(),
                set,
                drive,
                log_from: 0,
                last_emit: Instant::now(),
            };
            let end = run.all(&plan.stages);
            install::finish(&worker, &mut run.last_emit, end);
        })
        .map(drop)
        .map_err(ErrorReport::internal)
}

struct Run<'a> {
    app: &'a AppHandle,
    serial: String,
    dir: PathBuf,
    set: StorageSet,
    drive: Drive,
    /// How long the UPG's log was before the trigger; this run's lines follow.
    log_from: u64,
    last_emit: Instant,
}

impl Run<'_> {
    fn update(&mut self, force: bool, f: impl FnOnce(&mut JobView)) {
        install::update(self.app, &mut self.last_emit, force, f);
    }

    fn log(&mut self, line: &str) {
        install::log(self.app, &mut self.last_emit, line);
    }

    fn all(&mut self, stages: &[Stage]) -> End {
        if let Err(e) = self.check_files() {
            return End::Failed(e);
        }
        let copied: Vec<String> = stages
            .iter()
            .filter_map(|s| match s {
                Stage::Copy { file, .. } => Some(file.clone()),
                _ => None,
            })
            .collect();
        for (i, stage) in stages.iter().enumerate() {
            if install::cancelled(self.app) && !matches!(stage, Stage::WaitUpdate | Stage::CheckLog) {
                return End::Cancelled;
            }
            self.update(true, |j| {
                j.index = i;
                j.stage_started = install::now_ms();
                j.progress = None;
                j.waiting = false;
            });
            let result = match stage {
                Stage::Copy { file, bytes } => self.copy(file, *bytes),
                Stage::Keep { file } => {
                    let line = format!(
                        "{file}: kept, {} bytes on the drive",
                        self.drive
                            .mount
                            .as_ref()
                            .and_then(|m| drive::size(m, file))
                            .unwrap_or(0)
                    );
                    self.log(&line);
                    Ok(())
                }
                Stage::ReadBack { .. } => self.read_back(&copied),
                Stage::PickWalkman { file } => self.pick_walkman(file),
                Stage::Trigger => self.trigger(),
                Stage::WaitUpdate => self.wait_update(),
                Stage::CheckLog => match self.check_log() {
                    Ok(true) => return End::Done(None),
                    Ok(false) => Err(Error::unsupported(
                        Why::UpgDidNotRun,
                        "the UPG's log does not hold the lines the kit expects",
                    )),
                    Err(e) => Err(e),
                },
                other => Err(Error::Failed {
                    tool: "install",
                    detail: format!("{other:?} is not a storage stage"),
                }),
            };
            match result {
                Ok(()) => {}
                Err(Error::Cancelled) => return End::Cancelled,
                Err(e) => return End::Failed(e),
            }
        }
        End::Unverified
    }

    /// The files about to be copied are the ones the build wrote.
    fn check_files(&mut self) -> Result<(), Error> {
        let set = self.set.clone();
        let dir = self.dir.clone();
        let checked = set.check_files(&dir, &mut |done, total| {
            let p = done as f64 / total.max(1) as f64;
            self.update(false, |j| j.progress = Some(p.min(1.0)));
        })?;
        self.log(&format!("{} files of the build checked", checked.len()));
        Ok(())
    }

    /// The drive, mounted: the same disk found again by the player's serial when it left USB
    /// or someone unmounted it.
    fn mounted(&mut self) -> Result<PathBuf, Error> {
        if let Some(m) = &self.drive.mount
            && self.drive.mounted_at(m)
        {
            return Ok(m.clone());
        }
        let mut found = drive::for_serial(&self.serial)?
            .filter(|d| d.bytes > 0)
            .ok_or_else(|| Error::unsupported(Why::NoDrive, "the WALKMAN drive is not on this computer"))?;
        let mount = found.mount()?;
        self.log(&format!("{} mounted at {}", found.block.display(), mount.display()));
        self.drive = found;
        Ok(mount)
    }

    fn copy(&mut self, file: &str, bytes: u64) -> Result<(), Error> {
        let mount = self.mounted()?;
        let started = Instant::now();
        let app = self.app;
        let svc = app.state::<InstallService>();
        drive::copy_in(&self.dir.join(file), &mount, file, svc.cancel(), &mut |done| {
            let p = done as f64 / bytes.max(1) as f64;
            install::update(app, &mut self.last_emit, false, |j| j.progress = Some(p.min(1.0)));
        })?;
        let secs = started.elapsed().as_secs_f64();
        self.log(&format!(
            "{file}: {bytes} bytes in {secs:.1} s{}",
            if secs >= 1.0 {
                format!(", {:.1} MB/s", bytes as f64 / secs / 1e6)
            } else {
                String::new()
            }
        ));
        Ok(())
    }

    /// Unmounts and mounts again, so every byte hashed comes from the drive.
    fn read_back(&mut self, files: &[String]) -> Result<(), Error> {
        self.log("unmounting and mounting the drive again to read from it");
        self.drive.unmount()?;
        let mount = self.mounted()?;
        let total: u64 = files
            .iter()
            .filter_map(|f| self.set.files.get(f))
            .map(|f| f.bytes)
            .sum();
        let app = self.app;
        let svc = app.state::<InstallService>();
        let mut done = 0u64;
        let mut verdicts = Vec::new();
        for file in files {
            let got = match drive::hash(&mount, file, svc.cancel(), &mut |n| {
                let p = (done + n) as f64 / total.max(1) as f64;
                install::update(app, &mut self.last_emit, false, |j| j.progress = Some(p.min(1.0)));
            }) {
                Ok(h) => Some(h),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => {
                    self.log(&format!("{file}: {e}"));
                    None
                }
            };
            let want = self.set.files.get(file).map(|f| f.sha256.as_str());
            let ok = got.is_some() && got.as_deref() == want;
            self.log(&format!(
                "{file}: sha256 {} {}",
                got.as_deref().map_or("-", |g| &g[..16]),
                if ok { "ok" } else { "differs" }
            ));
            done += self.set.files.get(file).map_or(0, |f| f.bytes);
            verdicts.push(FileVerdict {
                path: file.clone(),
                ok,
                got,
            });
        }
        let ok = verdicts.iter().all(|v| v.ok);
        self.update(true, |j| {
            j.drive = Some(DriveVerdict {
                files: verdicts,
                ..DriveVerdict::default()
            })
        });
        if ok {
            Ok(())
        } else {
            Err(Error::unsupported(
                Why::ReadBackMismatch,
                "the drive does not read back this build",
            ))
        }
    }

    /// The picker starts what it started last; after the update it must start Walkman, whose
    /// drive the job waits for.
    fn pick_walkman(&mut self, file: &str) -> Result<(), Error> {
        let mount = self.mounted()?;
        let path = mount.join(file);
        let was = std::fs::read_to_string(&path).unwrap_or_default();
        std::fs::write(&path, "1\n")?;
        std::fs::File::open(&path)?.sync_all()?;
        self.log(&format!("{file}: {} -> 1", was.trim()));
        Ok(())
    }

    fn trigger(&mut self) -> Result<(), Error> {
        let mount = self.mounted()?;
        self.log_from = drive::size(&mount, &self.set.after.log).unwrap_or(0);
        self.drive.unmount()?;
        self.log(&format!(
            "drive unmounted; pkexec sends Sony's update command to {}",
            self.drive.disk.display()
        ));
        self.update(true, |j| j.waiting = true);
        let app = self.app;
        let svc = app.state::<InstallService>();
        let flags = upg::trigger(&self.drive.disk, svc.cancel())?;
        self.update(true, |j| {
            j.waiting = false;
            j.touched = true;
        });
        self.log(&format!("the player took the command (flags {flags:#04x})"));
        Ok(())
    }

    fn see(&mut self) -> Seen {
        let seen = if drive::for_serial(&self.serial).is_ok_and(|d| d.is_some_and(|d| d.bytes > 0)) {
            Seen::Drive
        } else if usb::list().is_ok_and(|l| l.iter().any(|d| d.serial.as_deref() == Some(self.serial.as_str()))) {
            Seen::Usb
        } else {
            Seen::Gone
        };
        self.update(false, |j| j.seen = Some(seen));
        seen
    }

    /// The player leaves USB for its updater, then comes back through the picker as the drive.
    fn wait_update(&mut self) -> Result<(), Error> {
        let left_by = Instant::now() + Duration::from_secs(90);
        while self.see() == Seen::Drive {
            if Instant::now() >= left_by {
                return Err(Error::unsupported(
                    Why::UpgDidNotRun,
                    "the player did not restart into its updater",
                ));
            }
            thread::sleep(POLL);
        }
        self.log("the player left USB for its updater");
        let back_by = Instant::now() + Duration::from_secs(15 * 60);
        while self.see() != Seen::Drive {
            if Instant::now() >= back_by {
                return Err(Error::unsupported(
                    Why::NoDriveReturn,
                    "the player did not come back as the WALKMAN drive",
                ));
            }
            thread::sleep(POLL);
        }
        self.update(true, |j| j.seen = Some(Seen::Drive));
        self.log("the WALKMAN drive is back");
        self.app.state::<DeviceService>().wake();
        Ok(())
    }

    /// Reads what this run of the UPG logged, and whether the updater took the UPG.
    fn check_log(&mut self) -> Result<bool, Error> {
        // The drive that just came back is in lsblk before udisks knows it ("Error looking up
        // object for device"), and the desktop may be mounting it meanwhile.
        self.drive.mount = None;
        let settle_by = Instant::now() + SETTLE;
        let mount = loop {
            match self.mounted() {
                Ok(m) => break m,
                Err(e) if Instant::now() >= settle_by => return Err(e),
                Err(_) => thread::sleep(POLL),
            }
        };
        let appended = drive::log_since(&mount, &self.set.after.log, self.log_from)?.unwrap_or_default();
        for line in appended.lines() {
            self.log(&format!("{}: {line}", self.set.after.log));
        }
        let upg = self.set.upg.file.clone();
        let upg_done = drive::size(&mount, &upg).is_none() && drive::size(&mount, &format!("{upg}.done")).is_some();
        let lines = self.set.judge_log(&appended);
        let mut ok = false;
        self.update(true, |j| {
            let v = j.drive.get_or_insert_with(DriveVerdict::default);
            v.lines = lines;
            v.upg_done = Some(upg_done);
            ok = v.ok();
        });
        Ok(ok)
    }
}
