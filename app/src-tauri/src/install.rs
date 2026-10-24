//! Installing a Studio build on a player: the plan from what the player says, then one job
//! that walks the plan's stages with fastboot and adb, reporting each as an event. Nothing is
//! sent before the owner typed the plan's word; the stages that cannot be taken back are not
//! stopped half way.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use uwm_core::adb::{Adb, AdbState};
use uwm_core::flash::fastboot::{self, Report};
use uwm_core::flash::plan::{self, Facts, Mode, Plan, Stage};
use uwm_core::flash::{DriveVerdict, Fastboot, InstallSet, StorageSet, Verdict, verify};
use uwm_core::probe::VolumeKind;
use uwm_core::registry::Mode as DeviceMode;
use uwm_core::studio::build::{self as kit_build, Outcome, Record, Variant};
use uwm_core::studio::catalog;
use uwm_core::{Error, ErrorReport, Why, usb};

use crate::devices::DeviceService;
use crate::install_drive;
use crate::studio::StudioService;
use crate::tools::ToolsService;

const LOG_LINES: usize = 80;
const POLL: Duration = Duration::from_millis(1500);

pub struct InstallService {
    job: Mutex<Option<JobView>>,
    /// An install is between its checks and its job; a second start waits for neither.
    starting: AtomicBool,
    cancel: AtomicBool,
    /// The owner said not to wait for adb after an install that formats userdata.
    skip: AtomicBool,
}

impl InstallService {
    pub fn new() -> Self {
        InstallService {
            job: Mutex::new(None),
            starting: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            skip: AtomicBool::new(false),
        }
    }

    /// Set when the owner asks the job to stop.
    pub(crate) fn cancel(&self) -> &AtomicBool {
        &self.cancel
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Running,
    Done,
    /// Written and booted, but the read-back was skipped.
    Unverified,
    Failed,
    Cancelled,
}

/// What the player looks like on USB right now, as the job sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Seen {
    Fastboot,
    Adb,
    Unauthorized,
    /// On USB without adb (MTP, or adb still starting).
    Usb,
    /// The WALKMAN drive of an NW-A50 series player, with its medium.
    Drive,
    Gone,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub build: String,
    pub device: String,
    pub serial: String,
    pub mode: Mode,
    pub variant: Variant,
    pub stages: Vec<Stage>,
    pub index: usize,
    pub state: JobState,
    /// ms since the epoch.
    pub started: i64,
    pub stage_started: i64,
    /// How far the current stage is, 0 to 1, when fastboot says.
    pub progress: Option<f64>,
    pub seen: Option<Seen>,
    /// The stage has been waiting on the owner (setup, USB debugging, approval).
    pub waiting: bool,
    /// Something reached the player's partitions, or the bootloader erased it: restarting
    /// before the install ends may leave Android unable to start.
    pub touched: bool,
    pub error: Option<ErrorReport>,
    pub verdict: Option<Verdict>,
    /// A storage install's read-back and the UPG's log.
    pub drive: Option<DriveVerdict>,
    pub log: VecDeque<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub(crate) plan: Plan,
    /// What a player installed over fastboot says, or what an NW-A50 and its drive say.
    pub(crate) facts: Option<Facts>,
    pub(crate) drive: Option<uwm_core::flash::DriveFacts>,
    pub(crate) set: Option<InstallSet>,
    pub(crate) storage: Option<StorageSet>,
    pub(crate) build: Record,
    /// The stock partitions this build was made from: going back to stock takes them.
    pub(crate) stock: Option<String>,
}

pub(crate) fn err(e: Error) -> ErrorReport {
    ErrorReport::from(&e)
}

fn find_build(app: &AppHandle, id: &str) -> Result<Record, ErrorReport> {
    let svc = app.state::<StudioService>();
    kit_build::find(&svc.builds_dir(), id)
        .ok_or_else(|| ErrorReport::refused("no-build", Why::BuildNotFound, format!("no build {id}")))
}

fn not_done(id: &str) -> ErrorReport {
    ErrorReport::refused("not-done", Why::BuildNotDone, format!("build {id} did not finish"))
}

fn done_build(record: Record) -> Result<(Record, InstallSet), ErrorReport> {
    let set = match (&record.outcome, &record.set) {
        (Outcome::Done, Some(set)) => set.clone(),
        _ => return Err(not_done(&record.id)),
    };
    set.validate().map_err(err)?;
    Ok((record, set))
}

/// A finished build whose set goes onto a player's WALKMAN drive.
pub(crate) fn done_storage(record: &Record) -> Result<Option<StorageSet>, ErrorReport> {
    match (&record.outcome, &record.storage) {
        (_, None) => Ok(None),
        (Outcome::Done, Some(set)) => {
            set.validate().map_err(err)?;
            Ok(Some(set.clone()))
        }
        _ => Err(not_done(&record.id)),
    }
}

fn fits(set: &InstallSet, mode: Mode) -> Result<(), ErrorReport> {
    if mode.fits(set) {
        return Ok(());
    }
    Err(ErrorReport::refused(
        "mode",
        Why::ModeNotForSet,
        format!("a {:?} set does not install as {mode:?}", set.variant),
    ))
}

/// What the player says about itself: over adb when it runs, over fastboot when it is
/// already in the bootloader.
fn facts(app: &AppHandle, device: &str, kit: &str) -> Result<Facts, ErrorReport> {
    let devices = app.state::<DeviceService>();
    let summary = devices
        .summary(device)
        .ok_or_else(|| err(Error::NoSuchDevice(device.to_owned())))?;
    let serial = summary
        .serial
        .clone()
        .ok_or_else(|| err(Error::NotReachable(device.to_owned())))?;
    match summary.mode {
        DeviceMode::Running => {
            let (adb, target, snap) = devices.fresh_snapshot(device, Duration::from_secs(5)).map_err(err)?;
            let marker = catalog::kit(kit)
                .and_then(|k| k.install.as_ref())
                .map(|i| i.marker.clone());
            let script = format!(
                "echo \"oem=$(getprop sys.oem_unlock_allowed)\"; echo \"secure=$(getprop ro.adb.secure)\"; {}",
                marker.map_or(String::new(), |m| format!("[ -e '{m}' ] && echo ours=1"))
            );
            let props = adb.shell(&target, &script, Duration::from_secs(15)).map_err(err)?;
            let prop = |k: &str| {
                props
                    .lines()
                    .find_map(|l| l.trim().strip_prefix(k)?.strip_prefix('=').map(str::to_owned))
            };
            let ours = prop("ours").as_deref() == Some("1");
            let running = ours.then(|| match prop("secure").as_deref() {
                Some("0") => Variant::Debug,
                _ => Variant::Release,
            });
            Ok(Facts {
                serial,
                profile: snap.identity.profile_id.clone().or(summary.profile_id.clone()),
                booted: true,
                vendor_firmware: snap.system.vendor_firmware.clone(),
                api_level: snap.system.api_level,
                slot: snap.system.slot.clone(),
                unlocked: snap.system.bootloader_unlocked,
                oem_unlock_allowed: prop("oem").map(|v| v == "1"),
                running,
                battery: snap.power.as_ref().and_then(|p| p.level),
                card: snap.volumes.iter().any(|v| v.kind == VolumeKind::Card),
                fastboot: app.state::<ToolsService>().flasher(),
            })
        }
        DeviceMode::Bootloader => {
            let tools = app.state::<ToolsService>();
            let fb = tools.fastboot().map_err(err)?;
            let unlocked = fb.getvar(&serial, "unlocked").map_err(err)?;
            let slot = fb.getvar(&serial, "current-slot").map_err(err)?;
            Ok(Facts {
                serial,
                profile: summary.profile_id.clone(),
                booted: false,
                slot,
                unlocked: unlocked.map(|v| v == "yes"),
                fastboot: tools.flasher(),
                ..Facts::default()
            })
        }
        _ => Err(ErrorReport::refused(
            "not-booted",
            Why::NotBooted,
            "the player is neither running with adb nor in fastboot",
        )),
    }
}

#[tauri::command(async)]
pub fn install_plan(build: String, device: String, mode: Mode, app: AppHandle) -> Result<PlanView, ErrorReport> {
    let record = find_build(&app, &build)?;
    if let Some(set) = done_storage(&record)? {
        return install_drive::plan_view(&app, &device, record, set, mode);
    }
    let (record, set) = done_build(record)?;
    fits(&set, mode)?;
    let facts = facts(&app, &device, &set.kit)?;
    let stock = record.inputs.iter().find(|i| i.input == "stock").map(|i| {
        let svc = app.state::<StudioService>();
        svc.dir
            .join("stock")
            .join(i.sha256.get(..16).unwrap_or(&i.sha256))
            .to_string_lossy()
            .into_owned()
    });
    Ok(PlanView {
        plan: plan::plan(&set, &facts, mode),
        facts: Some(facts),
        drive: None,
        set: Some(set),
        storage: None,
        build: record,
        stock,
    })
}

#[tauri::command]
pub fn install_state(app: AppHandle) -> Option<JobView> {
    app.state::<InstallService>().job.lock().unwrap().clone()
}

#[tauri::command(async)]
pub fn install_start(
    build: String,
    device: String,
    mode: Mode,
    confirm: String,
    app: AppHandle,
) -> Result<(), ErrorReport> {
    let svc = app.state::<InstallService>();
    let running = || {
        svc.job
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|j| j.state == JobState::Running)
    };
    if running() || svc.starting.swap(true, Ordering::SeqCst) {
        return Err(ErrorReport::refused("busy", Why::InstallBusy, "an install is running"));
    }
    let _starting = Starting(&svc.starting);
    let record = find_build(&app, &build)?;
    if let Some(set) = done_storage(&record)? {
        return install_drive::start(&app, device, record, set, mode, &confirm);
    }
    let (record, set) = done_build(record)?;
    fits(&set, mode)?;
    let facts = facts(&app, &device, &set.kit)?;
    let plan = plan::plan(&set, &facts, mode);
    may_start(&plan, &confirm)?;
    let fb = app.state::<ToolsService>().fastboot().map_err(err)?;
    let adb = Adb::locate(None).map_err(err)?;
    begin(
        &app,
        JobView::new(build, device, facts.serial.clone(), mode, set.variant, &plan),
    );

    let worker = app.clone();
    thread::Builder::new()
        .name("install".into())
        .spawn(move || {
            let mut run = Run {
                app: &worker,
                fb,
                adb,
                serial: facts.serial.clone(),
                dir: record.set_dir(),
                set,
                last_emit: Instant::now(),
            };
            let outcome = run.all(&plan.stages);
            run.finish(outcome);
        })
        .map(drop)
        .map_err(ErrorReport::internal)
}

/// The plan's gates are open and the owner typed its word.
pub(crate) fn may_start(plan: &Plan, confirm: &str) -> Result<(), ErrorReport> {
    if !plan.ready {
        let closed: Vec<&str> = plan
            .gates
            .iter()
            .filter(|g| g.ok == Some(false))
            .map(|g| g.id)
            .collect();
        let mut e = ErrorReport::refused("gate", Why::GateClosed, format!("closed: {}", closed.join(", ")));
        e.detail = Some(closed.join(", "));
        return Err(e);
    }
    if confirm.trim() != plan.confirm {
        return Err(ErrorReport::refused(
            "confirm",
            Why::ConfirmMismatch,
            format!("typed {confirm:?}, the plan asks for {:?}", plan.confirm),
        ));
    }
    Ok(())
}

impl JobView {
    pub(crate) fn new(
        build: String,
        device: String,
        serial: String,
        mode: Mode,
        variant: Variant,
        plan: &Plan,
    ) -> Self {
        let now = now_ms();
        JobView {
            build,
            device,
            serial,
            mode,
            variant,
            stages: plan.stages.clone(),
            index: 0,
            state: JobState::Running,
            started: now,
            stage_started: now,
            progress: None,
            seen: None,
            waiting: false,
            touched: false,
            error: None,
            verdict: None,
            drive: None,
            log: VecDeque::new(),
        }
    }
}

/// Makes `job` the app's install job and tells the page.
pub(crate) fn begin(app: &AppHandle, job: JobView) {
    let svc = app.state::<InstallService>();
    *svc.job.lock().unwrap() = Some(job.clone());
    svc.cancel.store(false, Ordering::SeqCst);
    svc.skip.store(false, Ordering::SeqCst);
    let _ = app.emit("install", &job);
}

/// Clears [`InstallService::starting`] however `install_start` returns.
struct Starting<'a>(&'a AtomicBool);

impl Drop for Starting<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Stops at the next point where stopping leaves the player whole; the writes themselves
/// and the bootloader's erase run to their end.
#[tauri::command]
pub fn install_cancel(app: AppHandle) {
    app.state::<InstallService>().cancel.store(true, Ordering::SeqCst);
}

/// After a clean install or going back to stock: do not wait for USB debugging; the
/// read-back is left out.
#[tauri::command]
pub fn install_skip(app: AppHandle) {
    app.state::<InstallService>().skip.store(true, Ordering::SeqCst);
}

/// An install that stopped in fastboot before writing anything: back to Android, and the
/// page starts from a plan again.
#[tauri::command(async)]
pub fn install_leave(app: AppHandle) -> Result<(), ErrorReport> {
    let svc = app.state::<InstallService>();
    let serial = match svc.job.lock().unwrap().as_ref() {
        None => return Ok(()),
        Some(j) if j.state == JobState::Running => {
            return Err(ErrorReport::refused("busy", Why::InstallBusy, "an install is running"));
        }
        Some(j) if j.touched => {
            return Err(ErrorReport::refused(
                "wrote",
                Why::InstallWrote,
                "the install had started writing; it is not booted half written",
            ));
        }
        Some(j) => j.serial.clone(),
    };
    let fb = app.state::<ToolsService>().fastboot().map_err(err)?;
    let adb = Adb::locate(None).map_err(err)?;
    if look(&adb, &fb, &serial) == Seen::Fastboot {
        let never = AtomicBool::new(false);
        fb.run(&serial, &["reboot"], Duration::from_secs(60), &never, &mut |_, _| {})
            .map_err(err)?;
    }
    let mut job = svc.job.lock().unwrap();
    if job.as_ref().is_some_and(|j| j.state != JobState::Running) {
        *job = None;
    }
    drop(job);
    app.state::<DeviceService>().wake();
    Ok(())
}

/// Forgets a finished job, so the page starts from a plan again.
#[tauri::command]
pub fn install_clear(app: AppHandle) {
    let svc = app.state::<InstallService>();
    let mut job = svc.job.lock().unwrap();
    if job.as_ref().is_some_and(|j| j.state != JobState::Running) {
        *job = None;
    }
}

pub(crate) enum End {
    /// Read back; a storage install's verdict is on the job already.
    Done(Option<Verdict>),
    Unverified,
    Cancelled,
    Failed(Error),
}

struct Run<'a> {
    app: &'a AppHandle,
    fb: Fastboot,
    adb: Adb,
    serial: String,
    dir: PathBuf,
    set: InstallSet,
    last_emit: Instant,
}

impl Run<'_> {
    fn svc(&self) -> tauri::State<'_, InstallService> {
        self.app.state::<InstallService>()
    }

    fn update(&mut self, force: bool, f: impl FnOnce(&mut JobView)) {
        update(self.app, &mut self.last_emit, force, f);
    }

    fn log(&mut self, line: &str) {
        log(self.app, &mut self.last_emit, line);
    }

    fn cancelled(&self) -> bool {
        self.svc().cancel.load(Ordering::SeqCst)
    }

    fn all(&mut self, stages: &[Stage]) -> End {
        if let Err(e) = self.check_files() {
            return End::Failed(e);
        }
        let mut verdict = None;
        for (i, stage) in stages.iter().enumerate() {
            if self.cancelled() {
                return verdict.map_or(End::Cancelled, |v| End::Done(Some(v)));
            }
            self.update(true, |j| {
                j.index = i;
                j.stage_started = now_ms();
                j.progress = None;
                j.waiting = false;
            });
            if matches!(stage, Stage::Unlock | Stage::Wipe | Stage::SetActive { .. }) {
                self.update(true, |j| j.touched = true);
            }
            let result = match stage {
                Stage::RebootBootloader => self.reboot_bootloader(),
                Stage::Unlock => self.unlock(),
                Stage::Flash {
                    partition, file, flags, ..
                } => self.flash(partition, file, flags),
                Stage::SetActive { slot } => self.fastboot(&[&format!("--set-active={slot}")], 60),
                Stage::Wipe => self.wipe(),
                Stage::Reboot => self.fastboot(&["reboot"], 60),
                Stage::WaitBoot => self.wait_boot(),
                Stage::WaitAdb => match self.wait_adb() {
                    Ok(true) => Ok(()),
                    Ok(false) => return End::Unverified,
                    Err(e) => Err(e),
                },
                Stage::Verify => match self.verify() {
                    Ok(v) if v.ok => {
                        verdict = Some(v);
                        Ok(())
                    }
                    Ok(v) => {
                        self.update(true, |j| j.verdict = Some(v));
                        Err(Error::unsupported(
                            Why::ReadBackMismatch,
                            "the player does not read back this build",
                        ))
                    }
                    Err(e) => Err(e),
                },
                Stage::Dexopt => {
                    self.dexopt();
                    Ok(())
                }
                other => Err(Error::Failed {
                    tool: "install",
                    detail: format!("{other:?} is not a fastboot stage"),
                }),
            };
            match result {
                Ok(()) => {}
                Err(Error::Cancelled) => return End::Cancelled,
                Err(e) => return End::Failed(e),
            }
        }
        verdict.map_or(End::Unverified, |v| End::Done(Some(v)))
    }

    fn finish(&mut self, end: End) {
        finish(self.app, &mut self.last_emit, end);
    }

    /// The files about to be sent are the ones the build wrote.
    fn check_files(&mut self) -> Result<(), Error> {
        let set = self.set.clone();
        let dir = self.dir.clone();
        let checked = set.check_files(&dir, &mut |done, total| {
            let p = done as f64 / total.max(1) as f64;
            self.update(false, |j| j.progress = Some(p.min(1.0)));
        })?;
        for (name, sha) in checked {
            self.log(&format!("{name}: sha256 {}", &sha[..16]));
        }
        Ok(())
    }

    fn see(&mut self) -> Seen {
        let seen = look(&self.adb, &self.fb, &self.serial);
        self.update(false, |j| j.seen = Some(seen));
        seen
    }

    fn reboot_bootloader(&mut self) -> Result<(), Error> {
        self.log("adb reboot bootloader");
        let out = uwm_core::process::run(
            "adb",
            std::process::Command::new(self.adb.path()).args(["-s", &self.serial, "reboot", "bootloader"]),
            Duration::from_secs(30),
        )?;
        if !out.success() {
            return Err(Error::Failed {
                tool: "adb",
                detail: out.stderr.trim().to_owned(),
            });
        }
        self.wait_for(Duration::from_secs(120), true, |s| s == Seen::Fastboot)
            .map(|_| ())
            .map_err(|_| Error::unsupported(Why::NotInFastboot, "the player did not come up in fastboot"))
    }

    fn unlock(&mut self) -> Result<(), Error> {
        let fb = self.fb.clone();
        let serial = self.serial.clone();
        // About 2.5 times the erase.
        fb.unlock(
            &serial,
            Duration::from_secs(25 * 60),
            Duration::from_secs(5),
            &mut |l| self.log(l),
        )
    }

    fn flash(&mut self, partition: &str, file: &str, flags: &[String]) -> Result<(), Error> {
        let path = self.dir.join(file);
        let path_text = path.to_string_lossy().into_owned();
        let mut args: Vec<&str> = flags.iter().map(String::as_str).collect();
        args.extend(["flash", partition, &path_text]);
        self.log(&format!("fastboot {}", args.join(" ")));
        let never = AtomicBool::new(false);
        let mut lines = Vec::new();
        // Each piece is sent, then written; the bar moves on both.
        let (mut chunk, mut chunks) = (1u32, 1u32);
        let ran = {
            let fb = self.fb.clone();
            let serial = self.serial.clone();
            fb.run(
                &serial,
                &args,
                Duration::from_secs(30 * 60),
                &never,
                &mut |text, partial| {
                    let said = fastboot::reports(text);
                    let okay = said.contains(&Report::Okay);
                    if said.iter().any(|r| matches!(r, Report::Sending { .. })) {
                        self.update(false, |j| j.touched = true);
                    }
                    let mut at = None;
                    for r in &said {
                        match r {
                            Report::Sending {
                                chunk: c, chunks: n, ..
                            } => {
                                (chunk, chunks) = (*c, (*n).max(1));
                                at = Some((chunk as f64 - if okay { 0.25 } else { 1.0 }) / chunks as f64);
                            }
                            Report::Writing { .. } if okay => at = Some(chunk as f64 / chunks as f64),
                            _ => {}
                        }
                    }
                    if !partial {
                        lines.push(text.to_owned());
                    }
                    if let Some(p) = at {
                        self.update(false, |j| j.progress = Some(p.clamp(0.0, 1.0)));
                    }
                },
            )
        };
        for l in lines {
            self.log(&l);
        }
        ran?;
        self.update(true, |j| j.progress = Some(1.0));
        Ok(())
    }

    fn fastboot(&mut self, args: &[&str], secs: u64) -> Result<(), Error> {
        self.fastboot_said(args, secs).map(drop)
    }

    /// Runs fastboot and hands back the lines it finished, once the job log has them.
    fn fastboot_said(&mut self, args: &[&str], secs: u64) -> Result<Vec<String>, Error> {
        self.log(&format!("fastboot {}", args.join(" ")));
        let never = AtomicBool::new(false);
        let mut lines = Vec::new();
        let fb = self.fb.clone();
        let ran = fb.run(
            &self.serial,
            args,
            Duration::from_secs(secs),
            &never,
            &mut |text, partial| {
                if !partial {
                    lines.push(text.to_owned());
                }
            },
        );
        for l in &lines {
            self.log(l);
        }
        ran.map(|()| lines)
    }

    /// `fastboot -w` exits 0 having wiped nothing when the bootloader does not name userdata's
    /// filesystem: platform-tools 37 only prints "wipe task partition not found: userdata".
    fn wipe(&mut self) -> Result<(), Error> {
        let said = self.fastboot_said(&["-w"], 25 * 60)?;
        if wiped_userdata(&said) {
            return Ok(());
        }
        let about: Vec<&str> = said
            .iter()
            .map(|l| l.trim())
            .filter(|l| l.contains("userdata"))
            .collect();
        Err(Error::unsupported(Why::NotWiped, "fastboot -w left userdata as it was").with_detail(about.join("\n")))
    }

    /// Polls until `done` holds for what the player shows, or the time is up.
    fn wait_for(&mut self, limit: Duration, cancellable: bool, done: impl Fn(Seen) -> bool) -> Result<Seen, Error> {
        let until = Instant::now() + limit;
        loop {
            let seen = self.see();
            if done(seen) {
                self.update(true, |j| j.seen = Some(seen));
                return Ok(seen);
            }
            if cancellable && self.cancelled() {
                return Err(Error::Cancelled);
            }
            if Instant::now() >= until {
                return Err(Error::Timeout {
                    tool: "player",
                    secs: limit.as_secs(),
                });
            }
            thread::sleep(POLL);
        }
    }

    /// The player left the bootloader and came back on USB as anything else.
    fn wait_boot(&mut self) -> Result<(), Error> {
        // The first boot after an erase sets the system up; later ones take about 80 s.
        let limit = Duration::from_secs(15 * 60);
        let back = self
            .wait_for(Duration::from_secs(60), true, |s| s != Seen::Fastboot)
            .and_then(|_| self.wait_for(limit, true, |s| s != Seen::Gone && s != Seen::Fastboot));
        match back {
            Ok(_) => Ok(()),
            Err(Error::Timeout { .. }) => Err(Error::unsupported(
                Why::NoReturn,
                "the player did not come back on USB after the reboot",
            )),
            Err(e) => Err(e),
        }
    }

    /// True once adb answers and Android finished booting; false when the owner chose to
    /// skip the read-back.
    fn wait_adb(&mut self) -> Result<bool, Error> {
        let mut asked = false;
        loop {
            if self.svc().skip.load(Ordering::SeqCst) {
                return Ok(false);
            }
            if self.cancelled() {
                return Err(Error::Cancelled);
            }
            let seen = self.see();
            if seen == Seen::Adb && verify::booted(&self.adb, &self.serial) {
                self.update(true, |j| j.waiting = false);
                return Ok(true);
            }
            if !asked && seen != Seen::Adb {
                asked = true;
                self.update(true, |j| j.waiting = true);
            }
            thread::sleep(POLL);
        }
    }

    fn verify(&mut self) -> Result<Verdict, Error> {
        self.log("read-back: slot, kernel, files");
        let v = verify::after_boot(&self.adb, &self.serial, &self.set)?;
        self.log(&format!(
            "slot {} {}, kernel {}, files {}/{}",
            v.slot.as_deref().unwrap_or("?"),
            if v.slot_ok { "ok" } else { "wrong" },
            if v.kernel_ok { "ok" } else { "wrong" },
            v.files.iter().filter(|f| f.ok).count(),
            v.files.len()
        ));
        Ok(v)
    }

    /// Compiling is Android's own job, which it does at the next idle charge anyway, so a
    /// failure here leaves the checked install as it is and only goes to the log.
    fn dexopt(&mut self) {
        self.log("cmd package bg-dexopt-job");
        let said = self
            .adb
            .shell(&self.serial, "cmd package bg-dexopt-job", Duration::from_secs(10 * 60));
        match said {
            Ok(out) => {
                for l in out.lines() {
                    self.log(l);
                }
            }
            Err(e) => self.log(&format!("bg-dexopt-job: {e}")),
        }
    }
}

/// Changes the job with `f` and tells the page, at most every 200 ms unless `force`.
pub(crate) fn update(app: &AppHandle, last_emit: &mut Instant, force: bool, f: impl FnOnce(&mut JobView)) {
    let svc = app.state::<InstallService>();
    let mut guard = svc.job.lock().unwrap();
    let Some(job) = guard.as_mut() else { return };
    f(job);
    if force || last_emit.elapsed() >= Duration::from_millis(200) {
        *last_emit = Instant::now();
        let _ = app.emit("install", &*job);
    }
}

pub(crate) fn log(app: &AppHandle, last_emit: &mut Instant, line: &str) {
    let line = line.trim_end().to_owned();
    if line.is_empty() {
        return;
    }
    tracing::info!("install: {line}");
    update(app, last_emit, false, |j| {
        if j.log.len() == LOG_LINES {
            j.log.pop_front();
        }
        j.log.push_back(line);
    });
}

pub(crate) fn finish(app: &AppHandle, last_emit: &mut Instant, end: End) {
    let (state, error, verdict) = match end {
        End::Done(v) => (JobState::Done, None, v),
        End::Unverified => (JobState::Unverified, None, None),
        End::Cancelled => (JobState::Cancelled, None, None),
        End::Failed(e) => (JobState::Failed, Some(err(e)), None),
    };
    tracing::info!("install: ended {state:?}");
    update(app, last_emit, true, |j| {
        j.state = state;
        j.error = error;
        if verdict.is_some() {
            j.verdict = verdict;
        }
        j.waiting = false;
    });
    app.state::<DeviceService>().wake();
}

pub(crate) fn cancelled(app: &AppHandle) -> bool {
    app.state::<InstallService>().cancel.load(Ordering::SeqCst)
}

fn look(adb: &Adb, fb: &Fastboot, serial: &str) -> Seen {
    if fb.devices().is_ok_and(|d| d.iter().any(|s| s == serial)) {
        return Seen::Fastboot;
    }
    if let Ok(list) = adb.devices()
        && let Some(d) = list.iter().find(|d| d.serial == serial)
    {
        return match d.state {
            AdbState::Device => Seen::Adb,
            AdbState::Unauthorized => Seen::Unauthorized,
            _ => Seen::Usb,
        };
    }
    if usb::list().is_ok_and(|l| l.iter().any(|d| d.serial.as_deref() == Some(serial))) {
        return Seen::Usb;
    }
    Seen::Gone
}

/// Userdata was erased and a new filesystem written, as `fastboot -w` does when it works.
fn wiped_userdata(said: &[String]) -> bool {
    let okay = |step: &str| {
        said.iter()
            .any(|l| l.trim_start().starts_with(&format!("{step} 'userdata'")) && l.contains("OKAY"))
    };
    okay("Erasing") && okay("Writing")
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_in_a_mode_not_for_it_is_refused_in_its_own_words() {
        let kit: InstallSet =
            serde_json::from_str(include_str!("../../../crates/uwm-core/tests/fixtures/install.json")).unwrap();
        let stock: InstallSet =
            serde_json::from_str(include_str!("../../../crates/uwm-core/tests/fixtures/restore.json")).unwrap();
        assert!(
            fits(&kit, Mode::Clean).is_ok() && fits(&kit, Mode::Update).is_ok() && fits(&stock, Mode::Restore).is_ok()
        );
        for (set, mode) in [(&kit, Mode::Restore), (&stock, Mode::Clean), (&stock, Mode::Update)] {
            assert_eq!(fits(set, mode).unwrap_err().say, "mode-not-for-set");
        }
    }

    #[test]
    fn a_wipe_that_skipped_userdata_is_not_a_wipe() {
        let said = |text: &str| text.lines().map(str::to_owned).collect::<Vec<_>>();
        let wiped = said(
            "Erasing 'userdata'                                 OKAY [568.012s]\n\
             mke2fs 1.47.2 (1-Jan-2025)\n\
             Sending 'userdata' (96 KB)                         OKAY [  0.012s]\n\
             Writing 'userdata'                                 OKAY [  0.204s]\n\
             wipe task partition not found: cache\n\
             Finished. Total time: 568.420s",
        );
        assert!(wiped_userdata(&wiped));
        let skipped = said(
            "wipe task partition not found: userdata\n\
             wipe task partition not found: cache\n\
             wipe task partition not found: metadata\n\
             Finished. Total time: 0.000s",
        );
        assert!(!wiped_userdata(&skipped));
        let unformatted = said(
            "Erasing 'userdata'                                 OKAY [568.012s]\n\
             Erase successful, but not automatically formatting.",
        );
        assert!(!wiped_userdata(&unformatted));
    }
}
