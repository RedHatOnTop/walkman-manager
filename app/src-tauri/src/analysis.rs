//! SensMe analysis made on this computer with Music Center for PC's own engine, for
//! the tracks of the library or of a player that have none.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use uwm_core::adb::Adb;
use uwm_core::library::analyse::{self, AnalyseItem, Engine, EngineInfo};
use uwm_core::library::pull::free_bytes;
use uwm_core::library::retag::{self, OnPlayer};
use uwm_core::library::transfer::{Progress, Report};
use uwm_core::library::{Codec, Track, hex_id};
use uwm_core::probe::Platform;
use uwm_core::{Error, ErrorReport, Why};

use crate::devices::DeviceService;
use crate::library::{LibraryService, start_scan, write_atomic};
use crate::sensme::Written;

const SNAPSHOT_AGE: Duration = Duration::from_secs(20);

#[derive(Default, Serialize, Deserialize)]
struct Settings {
    /// The engine the user chose, when it is not where Music Center installs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dll: Option<PathBuf>,
}

struct Stored {
    device_id: Option<String>,
    items: Vec<AnalyseItem>,
    /// Size and time of each item's track when the plan was made.
    stamps: Vec<(u64, i64)>,
}

pub struct AnalysisService {
    settings_file: PathBuf,
    settings: Mutex<Settings>,
    /// The helper and Wine's prefix.
    work: PathBuf,
    /// Copies of player files while they are analysed.
    scratch: PathBuf,
    backups: PathBuf,
    plans: Mutex<HashMap<String, Stored>>,
}

impl AnalysisService {
    pub fn new(data_dir: &Path, cache_dir: &Path) -> Self {
        let settings_file = data_dir.join("analysis.json");
        let settings = std::fs::read(&settings_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        AnalysisService {
            settings_file,
            settings: Mutex::new(settings),
            work: cache_dir.join("analysis"),
            scratch: cache_dir.join("analysis-work"),
            backups: data_dir.join("backups"),
            plans: Mutex::new(HashMap::new()),
        }
    }

    /// The chosen engine while it is still there, else the installed one.
    fn chosen(&self) -> Option<PathBuf> {
        self.settings
            .lock()
            .unwrap()
            .dll
            .clone()
            .filter(|p| analyse::is_engine(p))
    }

    fn engine(&self) -> Result<Engine, Error> {
        Engine::locate(self.chosen().as_deref(), &self.work)
    }

    fn view(&self, found: Result<Engine, Error>) -> EngineView {
        let need_engine = found.is_err() && self.chosen().is_none() && analyse::find_dll(&self.work).is_none();
        match found {
            Ok(e) => EngineView {
                engine: Some(e.info()),
                problem: None,
                need_engine,
            },
            Err(e) => EngineView {
                engine: None,
                problem: Some(ErrorReport::from(&e)),
                need_engine,
            },
        }
    }
}

/// The engine the analysis would run with, or why there is none.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineView {
    engine: Option<EngineInfo>,
    problem: Option<ErrorReport>,
    /// The engine itself is what is missing, so choosing its file helps.
    need_engine: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatCount {
    codec: Codec,
    tracks: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisPlan {
    id: String,
    device_id: Option<String>,
    /// Tracks without analysis, in a format it can be written into.
    tracks: usize,
    bytes: u64,
    duration_ms: u64,
    /// Tracks without analysis in other formats, which are left as they are.
    others: Vec<FormatCount>,
    engine: EngineInfo,
    /// Player only: where the originals are kept, and the room there.
    backup_root: Option<String>,
    free_bytes: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent<'a> {
    plan_id: &'a str,
    progress: &'a Progress,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoneEvent<'a> {
    plan_id: &'a str,
    device_id: Option<&'a str>,
    report: &'a Report,
    backup: Option<String>,
}

/// Off the main thread: finding the engine reads it and puts the helper in place.
#[tauri::command(async)]
pub fn analysis_engine(app: AppHandle) -> EngineView {
    let svc = app.state::<AnalysisService>();
    svc.view(svc.engine())
}

/// Asks for Music Center's `AVLib/MMLib11.dll` and keeps the choice. None when the
/// picker was closed.
#[tauri::command]
pub async fn analysis_pick_engine(app: AppHandle) -> Result<Option<EngineView>, ErrorReport> {
    let picker = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        picker
            .dialog()
            .file()
            .add_filter("MMLib11.dll", &["dll", "DLL"])
            .blocking_pick_file()
    })
    .await
    .map_err(ErrorReport::internal)?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || {
        let svc = app.state::<AnalysisService>();
        if !analyse::is_engine(&path) {
            return Err(ErrorReport::from(
                &Error::unsupported(
                    Why::NotAnalysisEngine,
                    format!(
                        "{} is not Music Center's analysis engine (AVLib/MMLib11.dll)",
                        path.display()
                    ),
                )
                .with_detail(path.display().to_string()),
            ));
        }
        {
            let mut settings = svc.settings.lock().unwrap();
            settings.dll = Some(path);
            if let Some(dir) = svc.settings_file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_vec_pretty(&*settings) {
                write_atomic(&svc.settings_file, &json);
            }
        }
        Ok::<_, ErrorReport>(Some(svc.view(svc.engine())))
    })
    .await
    .map_err(ErrorReport::internal)?
}

fn tracks_of(library: &LibraryService, device: Option<&str>) -> Result<Vec<Track>, Error> {
    match device {
        Some(id) => library
            .players
            .lock()
            .unwrap()
            .get(id)
            .map(|i| i.tracks.clone())
            .ok_or_else(|| Error::unsupported(Why::MusicNotRead, format!("the music on {id} has not been read yet"))),
        None => Ok(library.index.lock().unwrap().tracks.clone()),
    }
}

/// What analysing the library's or a player's unanalysed tracks would do.
#[tauri::command(async)]
pub fn analysis_plan(device: Option<String>, app: AppHandle) -> Result<AnalysisPlan, ErrorReport> {
    let svc = app.state::<AnalysisService>();
    let library = app.state::<LibraryService>();
    let plan = (|| -> Result<AnalysisPlan, Error> {
        let engine = svc.engine()?;
        let mut tracks = tracks_of(&library, device.as_deref())?;
        if let Some(id) = &device {
            let unread = crate::sensme::merge_cached(&library, id, &mut tracks);
            if unread > 0 {
                return Err(Error::unsupported(
                    Why::UnreadAnalysis,
                    format!(
                        "{unread} files on the player have not been read for the analysis they already hold; read them first"
                    ),
                )
                .with_var("n", unread));
            }
        }
        let missing: Vec<&Track> = tracks.iter().filter(|t| t.sensme.is_none()).collect();
        let (todo, rest): (Vec<&Track>, Vec<&Track>) = missing.into_iter().partition(|t| analyse::writable(&t.path));
        let mut others: BTreeMap<Codec, usize> = BTreeMap::new();
        for t in &rest {
            *others.entry(t.format.codec).or_default() += 1;
        }
        let bytes = todo.iter().map(|t| t.size).sum();
        let backup_root = device.as_ref().map(|_| svc.backups.to_string_lossy().into_owned());
        let free = device.as_ref().and_then(|_| {
            let mut at = svc.backups.as_path();
            while !at.exists() {
                at = at.parent()?;
            }
            free_bytes(at)
        });
        let id = hex_id(format!("analysis\0{}\0{}", device.as_deref().unwrap_or(""), todo.len()).as_bytes());
        let plan = AnalysisPlan {
            id: id.clone(),
            device_id: device.clone(),
            tracks: todo.len(),
            bytes,
            duration_ms: todo.iter().map(|t| t.duration_ms).sum(),
            others: others
                .into_iter()
                .map(|(codec, tracks)| FormatCount { codec, tracks })
                .collect(),
            engine: engine.info(),
            backup_root,
            free_bytes: free,
        };
        svc.plans.lock().unwrap().insert(
            id,
            Stored {
                device_id: device.clone(),
                items: todo
                    .iter()
                    .map(|t| AnalyseItem {
                        track_id: t.id.clone(),
                        path: t.path.clone(),
                        rel: t.rel.clone(),
                        size: t.size,
                    })
                    .collect(),
                stamps: todo.iter().map(|t| (t.size, t.modified)).collect(),
            },
        );
        Ok(plan)
    })();
    plan.map_err(|e| ErrorReport::from(&e))
}

/// Off the main thread: a player is probed first.
#[tauri::command(async)]
pub fn analysis_start(plan_id: String, app: AppHandle) -> Result<(), ErrorReport> {
    let svc = app.state::<AnalysisService>();
    let library = app.state::<LibraryService>();
    let stored =
        svc.plans.lock().unwrap().remove(&plan_id).ok_or_else(|| {
            ErrorReport::refused("no-such-plan", Why::PlanExpired, "the plan has expired; look again")
        })?;
    if stored.items.is_empty() {
        return Err(ErrorReport::refused(
            "nothing",
            Why::AllAnalysed,
            "every track already has its analysis",
        ));
    }
    let engine = svc.engine().map_err(|e| ErrorReport::from(&e))?;
    let player = match &stored.device_id {
        Some(device_id) => {
            let devices = app.state::<DeviceService>();
            let (_, target, snap) = devices
                .fresh_snapshot(device_id, SNAPSHOT_AGE)
                .map_err(|e| ErrorReport::from(&e))?;
            let name = snap.identity.model.clone().unwrap_or_else(|| device_id.clone());
            let adb = Adb::locate(None).map_err(|e| ErrorReport::from(&e))?;
            Some((
                adb,
                target,
                snap.system.platform == Platform::Android,
                retag::backup_dir(&svc.backups, &name),
            ))
        }
        None => None,
    };
    if library.syncing.swap(true, Ordering::SeqCst) {
        return Err(ErrorReport::refused("busy", Why::Busy, "another transfer is running"));
    }
    library.cancel_sync.store(false, Ordering::SeqCst);
    let scratch = svc.scratch.clone();
    let app = app.clone();
    thread::Builder::new()
        .name("analysis".into())
        .spawn(move || {
            let library = app.state::<LibraryService>();
            let mut emit = |p: &Progress| {
                let _ = app.emit(
                    "analysis-progress",
                    ProgressEvent {
                        plan_id: &plan_id,
                        progress: p,
                    },
                );
            };
            let on_player = player.as_ref().map(|(adb, serial, android, backup)| OnPlayer {
                adb,
                serial,
                android: *android,
                work: &scratch,
                backup,
            });
            let (report, found) = analyse::run(&engine, &stored.items, on_player, &library.cancel_sync, &mut emit);
            let device_id = stored.device_id.as_deref();
            tracing::info!(
                "analysis {plan_id} on {}: {} written, {} failed, cancelled {}",
                device_id.unwrap_or("this computer"),
                report.copied,
                report.failures.len(),
                report.cancelled
            );
            match device_id {
                Some(id) => {
                    let written: Vec<Written> = stored
                        .items
                        .iter()
                        .zip(&stored.stamps)
                        .zip(found)
                        .filter_map(|((item, (size, modified)), s)| {
                            s.map(|sensme| Written {
                                path: item.path.clone(),
                                size: *size,
                                modified: *modified,
                                sensme,
                            })
                        })
                        .collect();
                    crate::sensme::remember(&library, id, written);
                }
                None if report.copied > 0 => start_scan(&app),
                None => {}
            }
            library.syncing.store(false, Ordering::SeqCst);
            let kept = player
                .as_ref()
                .map(|p| &p.3)
                .filter(|b| b.is_dir())
                .map(|b| b.to_string_lossy().into_owned());
            let _ = app.emit(
                "analysis-done",
                DoneEvent {
                    plan_id: &plan_id,
                    device_id,
                    report: &report,
                    backup: kept,
                },
            );
        })
        .map_err(ErrorReport::internal)?;
    Ok(())
}
