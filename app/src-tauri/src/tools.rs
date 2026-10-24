//! Google's platform-tools on this computer: downloaded once the owner agrees to the Android
//! SDK licence, then the fastboot every install runs, and the adb the app uses where the
//! computer has none.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use uwm_core::adb::Adb;
use uwm_core::flash::{Fastboot, Flasher};
use uwm_core::platform_tools::{self as pt, LICENCE, LICENCE_FROM, REVISION};
use uwm_core::studio::licence::{self, LicenceText};
use uwm_core::{Error, ErrorReport, Why};

use crate::devices::DeviceService;
use crate::studio::PROGRESS_EVERY;

pub struct ToolsService {
    /// The app's data folder; the tools unpack under it.
    data: PathBuf,
    /// Where licence agreements are kept, with the Studio's.
    agreements: PathBuf,
    /// Where the formatter check writes its small image.
    scratch: PathBuf,
    /// The licence text as last shown to the owner.
    shown: Mutex<Option<LicenceText>>,
    fetching: Mutex<Option<Fetching>>,
    cancel: AtomicBool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fetching {
    done: u64,
    total: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsView {
    /// The revision unpacked here, if any.
    installed: Option<String>,
    /// The revision the app downloads, and its archive's size.
    revision: &'static str,
    bytes: u64,
    agreed: bool,
    fetching: Option<Fetching>,
    adb: Option<String>,
    fastboot: Option<String>,
    flasher: Flasher,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolsDone {
    view: ToolsView,
    error: Option<ErrorReport>,
    cancelled: bool,
}

impl ToolsService {
    pub fn new(data_dir: &Path, cache_dir: &Path) -> Self {
        Adb::fall_back_to(pt::adb(data_dir));
        ToolsService {
            data: data_dir.to_path_buf(),
            agreements: data_dir.join("studio"),
            scratch: cache_dir.to_path_buf(),
            shown: Mutex::new(None),
            fetching: Mutex::new(None),
            cancel: AtomicBool::new(false),
        }
    }

    /// The downloaded fastboot, else the one beside adb, else the one on `PATH`.
    pub fn fastboot(&self) -> Result<Fastboot, Error> {
        let adb = Adb::locate(None).ok();
        Fastboot::locate(pt::fastboot(&self.data).as_deref(), adb.as_ref().map(|a| a.path()))
    }

    pub fn flasher(&self) -> Flasher {
        self.judge(self.fastboot().ok().as_ref())
    }

    fn judge(&self, fastboot: Option<&Fastboot>) -> Flasher {
        match fastboot {
            None => Flasher::Missing,
            Some(fb) if fb.formats(&self.scratch) => Flasher::Formats,
            Some(_) => Flasher::Writes,
        }
    }

    fn agreed(&self) -> bool {
        licence::agreed(&self.agreements).contains_key(LICENCE)
    }

    fn view(&self) -> ToolsView {
        let fastboot = self.fastboot().ok();
        ToolsView {
            installed: pt::installed(&self.data),
            revision: REVISION,
            bytes: pt::BYTES,
            agreed: self.agreed(),
            fetching: *self.fetching.lock().unwrap(),
            adb: Adb::locate(None).ok().map(|a| a.path().display().to_string()),
            flasher: self.judge(fastboot.as_ref()),
            fastboot: fastboot.map(|f| f.path().display().to_string()),
        }
    }
}

#[tauri::command(async)]
pub fn tools_view(app: AppHandle) -> ToolsView {
    app.state::<ToolsService>().view()
}

/// The licence the platform-tools come under, from Google's SDK repository.
#[tauri::command(async)]
pub fn tools_licence(app: AppHandle) -> Result<LicenceText, ErrorReport> {
    let text = licence::read(LICENCE, LICENCE_FROM).map_err(|e| ErrorReport::from(&e))?;
    *app.state::<ToolsService>().shown.lock().unwrap() = Some(text.clone());
    Ok(text)
}

/// Keeps the owner's agreement to the text [`tools_licence`] showed, named by its sha256.
#[tauri::command(async)]
pub fn tools_agree(sha256: String, app: AppHandle) -> Result<ToolsView, ErrorReport> {
    let svc = app.state::<ToolsService>();
    let shown = svc.shown.lock().unwrap().clone();
    let text = shown.filter(|t| t.sha256 == sha256).ok_or_else(|| {
        ErrorReport::refused(
            "licence",
            Why::LicenceChanged,
            format!("{sha256} is not the text last shown for {LICENCE}"),
        )
    })?;
    licence::agree(&svc.agreements, &text).map_err(|e| ErrorReport::from(&e))?;
    Ok(svc.view())
}

/// Downloads the platform-tools; `tools-progress` events carry the bytes, `tools-done` the end.
#[tauri::command(async)]
pub fn tools_fetch(app: AppHandle) -> Result<(), ErrorReport> {
    let svc = app.state::<ToolsService>();
    if !svc.agreed() {
        return Err(ErrorReport::refused(
            "licence",
            Why::LicenceNotAgreed,
            format!("{LICENCE} is not agreed to"),
        ));
    }
    {
        let mut fetching = svc.fetching.lock().unwrap();
        if fetching.is_some() {
            return Err(ErrorReport::refused(
                "busy",
                Why::ToolsBusy,
                "the platform-tools are downloading",
            ));
        }
        *fetching = Some(Fetching {
            done: 0,
            total: pt::BYTES,
        });
    }
    svc.cancel.store(false, Ordering::SeqCst);
    let worker = app.clone();
    thread::Builder::new()
        .name("tools".into())
        .spawn(move || {
            let svc = worker.state::<ToolsService>();
            let mut last = Instant::now();
            let fetched = pt::fetch(&svc.data, &svc.cancel, &mut |done, total| {
                let now = Fetching { done, total };
                *svc.fetching.lock().unwrap() = Some(now);
                if done < total && last.elapsed() < PROGRESS_EVERY {
                    return;
                }
                last = Instant::now();
                let _ = worker.emit("tools-progress", now);
            });
            *svc.fetching.lock().unwrap() = None;
            if fetched.is_ok() {
                Adb::fall_back_to(pt::adb(&svc.data));
                worker.state::<DeviceService>().refresh_adb();
            }
            let (error, cancelled) = match fetched {
                Ok(()) => (None, false),
                Err(Error::Cancelled) => (None, true),
                Err(e) => (Some(ErrorReport::from(&e)), false),
            };
            let view = svc.view();
            let _ = worker.emit("tools-done", ToolsDone { view, error, cancelled });
        })
        .map(drop)
        .map_err(|e| {
            *app.state::<ToolsService>().fetching.lock().unwrap() = None;
            ErrorReport::internal(e)
        })
}

#[tauri::command]
pub fn tools_cancel(app: AppHandle) {
    app.state::<ToolsService>().cancel.store(true, Ordering::SeqCst);
}
