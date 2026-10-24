use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use uwm_core::adb::Adb;
use uwm_core::library::art::ArtCache;
use uwm_core::library::plan::{self, Plan, Target};
use uwm_core::library::player;
use uwm_core::library::pull::{self, PullPlan};
use uwm_core::library::remote::{self, RemoteFile};
use uwm_core::library::scan::{self, ScanProgress};
use uwm_core::library::sensme::{self, Table};
use uwm_core::library::transfer::{self, Progress, Report};
use uwm_core::library::{INDEX_VERSION, Index, Track};
use uwm_core::probe::{Platform, VolumeKind};
use uwm_core::{Error, ErrorReport, Why, profile};

use crate::devices::DeviceService;

/// Readings older than this are taken again before planning against a volume.
const SNAPSHOT_AGE: Duration = Duration::from_secs(20);
/// Covers read off players at once; each is one adb process.
const ART_FETCHES: usize = 4;

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    roots: Vec<PathBuf>,
}

pub struct LibraryService {
    settings_file: PathBuf,
    index_file: PathBuf,
    pub art: Arc<ArtCache>,
    roots: Mutex<Vec<PathBuf>>,
    pub(crate) index: Arc<Mutex<Index>>,
    scanning: Arc<AtomicBool>,
    cancel_scan: Arc<AtomicBool>,
    plans: Mutex<HashMap<String, Plan>>,
    /// One transfer at a time in either direction, so two never share the cable.
    pub(crate) syncing: Arc<AtomicBool>,
    pub(crate) cancel_sync: Arc<AtomicBool>,
    /// The last library read off each player, by device id.
    pub(crate) players: Mutex<HashMap<String, Index>>,
    /// Where each player cover comes from: art key to adb target and content uri.
    remote_art: Mutex<HashMap<String, (String, String)>>,
    /// Covers a player answered without an image for, so they are not asked again.
    missing_art: Mutex<HashSet<String>>,
    art_slots: (Mutex<usize>, Condvar),
    pulls: Mutex<HashMap<String, PullPlan>>,
    /// The firmware's SensMe table, copied out of the user's firmware once.
    pub(crate) sensme_file: PathBuf,
    pub(crate) sensme: RwLock<Option<Arc<Table>>>,
    /// What was read off each player's files, so a file is read once.
    pub(crate) sensme_cache: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryState {
    roots: Vec<String>,
    scanning: bool,
    index: Index,
    /// Whether SensMe channels can be worked out.
    sensme_table: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanEvent {
    running: bool,
    progress: ScanProgress,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncProgressEvent<'a> {
    plan_id: &'a str,
    progress: &'a Progress,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncDoneEvent<'a> {
    plan_id: &'a str,
    device_id: &'a str,
    report: &'a Report,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PullDoneEvent<'a> {
    plan_id: &'a str,
    device_id: &'a str,
    dest_root: &'a str,
    report: &'a Report,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerLibrary {
    device_id: String,
    /// `media-store` when the player's own music database was read, `folders` when
    /// albums come from the folder layout alone.
    source: &'static str,
    index: Index,
    /// Files that may hold SensMe data and have not been read for it yet.
    sensme_unread: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeMusic {
    kind: VolumeKind,
    root: String,
    exists: bool,
    free_bytes: u64,
    total_bytes: u64,
    files: Vec<RemoteFile>,
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes)
        .and_then(|_| std::fs::rename(&tmp, path))
        .is_err()
    {
        tracing::warn!("could not save {}", path.display());
    }
}

impl LibraryService {
    pub fn new(data_dir: &Path, cache_dir: &Path) -> Self {
        let settings_file = data_dir.join("library.json");
        let index_file = cache_dir.join("library-index.json");
        let settings: Settings = match std::fs::read(&settings_file) {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_default(),
            Err(_) => Settings {
                roots: dirs::audio_dir().filter(|d| d.is_dir()).into_iter().collect(),
            },
        };
        let index: Index = std::fs::read(&index_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .filter(|i: &Index| i.version == INDEX_VERSION)
            .unwrap_or_default();
        let sensme_file = data_dir.join("sensme-table.json");
        let table = Table::load(&sensme_file).ok().map(Arc::new);
        LibraryService {
            settings_file,
            index_file,
            art: Arc::new(ArtCache::new(cache_dir.join("art"))),
            roots: Mutex::new(settings.roots),
            index: Arc::new(Mutex::new(index)),
            scanning: Arc::new(AtomicBool::new(false)),
            cancel_scan: Arc::new(AtomicBool::new(false)),
            plans: Mutex::new(HashMap::new()),
            syncing: Arc::new(AtomicBool::new(false)),
            cancel_sync: Arc::new(AtomicBool::new(false)),
            players: Mutex::new(HashMap::new()),
            remote_art: Mutex::new(HashMap::new()),
            missing_art: Mutex::new(HashSet::new()),
            art_slots: (Mutex::new(0), Condvar::new()),
            pulls: Mutex::new(HashMap::new()),
            sensme_file,
            sensme: RwLock::new(table),
            sensme_cache: cache_dir.join("sensme"),
        }
    }

    /// Works out the SensMe channels of analysed tracks with the current table.
    pub(crate) fn classify(&self, tracks: &mut [Track]) {
        let table = self.sensme.read().unwrap().clone();
        sensme::classify(tracks.iter_mut().filter_map(|t| t.sensme.as_mut()), table.as_deref());
    }

    pub(crate) fn save_index(&self, index: &Index) {
        if let Some(dir) = self.index_file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec(index) {
            write_atomic(&self.index_file, &json);
        }
    }

    /// A cached cover, reading it off its player first when it came from one.
    /// Blocks while the player answers.
    pub fn art_bytes(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(path) = self.art.path(key) {
            return std::fs::read(path).ok();
        }
        let (target, uri) = self.remote_art.lock().unwrap().get(key).cloned()?;
        if self.missing_art.lock().unwrap().contains(key) {
            return None;
        }
        let (count, freed) = &self.art_slots;
        let mut busy = freed.wait_while(count.lock().unwrap(), |n| *n >= ART_FETCHES).unwrap();
        *busy += 1;
        drop(busy);
        let fetched = match self.art.path(key) {
            Some(_) => None,
            None => Some(Adb::locate(None).and_then(|adb| player::read_art(&adb, &target, &uri))),
        };
        *count.lock().unwrap() -= 1;
        freed.notify_one();
        match fetched {
            Some(Ok(bytes)) => {
                if self.art.store_as(key, &bytes).is_none() {
                    self.missing_art.lock().unwrap().insert(key.to_owned());
                }
            }
            Some(Err(e)) => {
                tracing::debug!("no cover at {uri}: {e}");
                self.missing_art.lock().unwrap().insert(key.to_owned());
            }
            None => {}
        }
        std::fs::read(self.art.path(key)?).ok()
    }

    /// Where music taken off a player goes: the first library folder, else the
    /// system's music folder.
    fn pull_root(&self) -> Option<PathBuf> {
        self.roots
            .lock()
            .unwrap()
            .first()
            .cloned()
            .or_else(dirs::audio_dir)
            .or_else(|| dirs::home_dir().map(|h| h.join("Music")))
    }

    fn state(&self) -> LibraryState {
        LibraryState {
            roots: self
                .roots
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.to_string_lossy().into_owned())
                .collect(),
            scanning: self.scanning.load(Ordering::SeqCst),
            index: self.index.lock().unwrap().clone(),
            sensme_table: self.sensme.read().unwrap().is_some(),
        }
    }

    fn save_settings(&self) {
        let settings = Settings {
            roots: self.roots.lock().unwrap().clone(),
        };
        if let Some(dir) = self.settings_file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&settings) {
            write_atomic(&self.settings_file, &json);
        }
    }
}

/// Starts a scan of the current roots unless one is running.
pub fn start_scan(app: &AppHandle) {
    let svc = app.state::<LibraryService>();
    if svc.scanning.swap(true, Ordering::SeqCst) {
        return;
    }
    svc.cancel_scan.store(false, Ordering::SeqCst);
    let roots = svc.roots.lock().unwrap().clone();
    let previous = svc.index.lock().unwrap().clone();
    let app = app.clone();
    thread::Builder::new()
        .name("library-scan".into())
        .spawn(move || {
            let svc = app.state::<LibraryService>();
            let emit = |p: &ScanProgress| {
                let _ = app.emit(
                    "library-scan",
                    ScanEvent {
                        running: true,
                        progress: p.clone(),
                    },
                );
            };
            let mut index = scan::scan(&roots, Some(&previous), &svc.art, &svc.cancel_scan, &emit);
            svc.classify(&mut index.tracks);
            let cancelled = svc.cancel_scan.load(Ordering::SeqCst);
            let progress = ScanProgress {
                found: index.tracks.len() + index.skipped.len(),
                done: index.tracks.len() + index.skipped.len(),
                reused: 0,
            };
            if !cancelled {
                svc.save_index(&index);
                *svc.index.lock().unwrap() = index;
            }
            svc.scanning.store(false, Ordering::SeqCst);
            let _ = app.emit(
                "library-scan",
                ScanEvent {
                    running: false,
                    progress,
                },
            );
            let _ = app.emit("library", ());
        })
        .expect("spawn library-scan");
}

#[tauri::command]
pub fn library_state(svc: State<'_, LibraryService>) -> LibraryState {
    svc.state()
}

#[tauri::command]
pub async fn library_add_root(app: AppHandle) -> Result<LibraryState, ErrorReport> {
    let picker = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || picker.dialog().file().blocking_pick_folder())
        .await
        .map_err(ErrorReport::internal)?;
    let svc = app.state::<LibraryService>();
    if let Some(path) = picked.and_then(|p| p.into_path().ok()) {
        let mut roots = svc.roots.lock().unwrap();
        let covered = roots.iter().any(|r| path.starts_with(r));
        if !covered {
            roots.retain(|r| !r.starts_with(&path));
            roots.push(path);
            drop(roots);
            svc.save_settings();
            start_scan(&app);
        }
    }
    Ok(svc.state())
}

#[tauri::command]
pub fn library_remove_root(path: String, app: AppHandle) -> LibraryState {
    let svc = app.state::<LibraryService>();
    svc.roots.lock().unwrap().retain(|r| r != Path::new(&path));
    svc.save_settings();
    start_scan(&app);
    svc.state()
}

#[tauri::command]
pub fn library_rescan(app: AppHandle) {
    start_scan(&app);
}

#[tauri::command]
pub fn library_cancel_scan(svc: State<'_, LibraryService>) {
    svc.cancel_scan.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub async fn device_music(id: String, app: AppHandle) -> Result<Vec<VolumeMusic>, ErrorReport> {
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<VolumeMusic>, Error> {
        let devices = app.state::<DeviceService>();
        let (adb, target, snap) = devices.fresh_snapshot(&id, SNAPSHOT_AGE)?;
        snap.volumes
            .iter()
            .map(|v| {
                let root = remote::music_root(snap.system.platform, v);
                let listing = remote::list(&adb, &target, &root)?;
                Ok(VolumeMusic {
                    kind: v.kind,
                    root,
                    exists: listing.exists,
                    free_bytes: v.free_bytes,
                    total_bytes: v.total_bytes,
                    files: listing.files,
                })
            })
            .collect()
    })
    .await
    .map_err(ErrorReport::internal)?
    .map_err(|e| ErrorReport::from(&e))
}

#[tauri::command]
pub async fn sync_plan(
    id: String,
    volume: VolumeKind,
    tracks: Vec<String>,
    app: AppHandle,
) -> Result<Plan, ErrorReport> {
    let plan = tauri::async_runtime::spawn_blocking(move || -> Result<Plan, Error> {
        let devices = app.state::<DeviceService>();
        let library = app.state::<LibraryService>();
        let (adb, target, snap) = devices.fresh_snapshot(&id, SNAPSHOT_AGE)?;
        let profile = snap
            .identity
            .profile_id
            .as_deref()
            .and_then(profile::get)
            .ok_or_else(|| {
                Error::unsupported(Why::UnknownPlayer, format!("{id} is not a player this version knows"))
            })?;
        let vol = snap.volumes.iter().find(|v| v.kind == volume).ok_or_else(|| {
            Error::unsupported(Why::VolumeNotMounted, format!("{id} has no {volume:?} volume mounted"))
        })?;
        let root = remote::music_root(snap.system.platform, vol);
        let listing = remote::list(&adb, &target, &root)?;
        let index = library.index.lock().unwrap().clone();
        let plan = plan::build(
            &index,
            &tracks,
            &listing,
            &profile.playback,
            Target {
                device_id: id,
                adb_target: target,
                platform: snap.system.platform,
                volume,
                root,
                free_bytes: vol.free_bytes,
            },
        );
        library.plans.lock().unwrap().insert(plan.id.clone(), plan.clone());
        Ok(plan)
    })
    .await
    .map_err(ErrorReport::internal)?
    .map_err(|e| ErrorReport::from(&e))?;
    Ok(plan)
}

#[tauri::command]
pub fn sync_start(plan_id: String, app: AppHandle) -> Result<(), ErrorReport> {
    let svc = app.state::<LibraryService>();
    let plan = svc.plans.lock().unwrap().get(&plan_id).cloned().ok_or_else(|| {
        ErrorReport::refused(
            "no-such-plan",
            Why::PlanExpired,
            "the plan has expired; review it again",
        )
    })?;
    if !plan.fits {
        return Err(ErrorReport::refused(
            "no-space",
            Why::NoRoomPlayer,
            "the selection does not fit on this volume",
        ));
    }
    let adb = uwm_core::adb::Adb::locate(None).map_err(|e| ErrorReport::from(&e))?;
    if svc.syncing.swap(true, Ordering::SeqCst) {
        return Err(ErrorReport::refused("busy", Why::Busy, "another transfer is running"));
    }
    svc.cancel_sync.store(false, Ordering::SeqCst);
    let app = app.clone();
    thread::Builder::new()
        .name("music-sync".into())
        .spawn(move || {
            let svc = app.state::<LibraryService>();
            let report = transfer::run(&adb, &plan, &svc.cancel_sync, &mut |p: &Progress| {
                let _ = app.emit(
                    "sync-progress",
                    SyncProgressEvent {
                        plan_id: &plan.id,
                        progress: p,
                    },
                );
            });
            tracing::info!(
                "sync to {} {:?}: {} copied, {} verified, {} failed, cancelled {}",
                plan.target.device_id,
                plan.target.volume,
                report.copied,
                report.verified,
                report.failures.len(),
                report.cancelled
            );
            svc.plans.lock().unwrap().remove(&plan.id);
            app.state::<DeviceService>().forget_snapshot(&plan.target.device_id);
            svc.syncing.store(false, Ordering::SeqCst);
            let _ = app.emit(
                "sync-done",
                SyncDoneEvent {
                    plan_id: &plan.id,
                    device_id: &plan.target.device_id,
                    report: &report,
                },
            );
        })
        .map_err(ErrorReport::internal)?;
    Ok(())
}

#[tauri::command]
pub fn sync_cancel(svc: State<'_, LibraryService>) {
    svc.cancel_sync.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub async fn player_library(id: String, app: AppHandle) -> Result<PlayerLibrary, ErrorReport> {
    tauri::async_runtime::spawn_blocking(move || -> Result<PlayerLibrary, Error> {
        let devices = app.state::<DeviceService>();
        let library = app.state::<LibraryService>();
        let (adb, target, snap) = devices.fresh_snapshot(&id, SNAPSHOT_AGE)?;
        let (source, mut read) = match snap.system.platform {
            Platform::Android => ("media-store", player::read_media_store(&adb, &target)?),
            Platform::WalkmanLinux => {
                let listings = snap
                    .volumes
                    .iter()
                    .map(|v| remote::list(&adb, &target, &remote::music_root(snap.system.platform, v)))
                    .collect::<Result<Vec<_>, Error>>()?;
                ("folders", player::from_listings(&target, &listings))
            }
        };
        {
            let mut art = library.remote_art.lock().unwrap();
            for a in read.art {
                art.insert(a.key, (target.clone(), a.uri));
            }
        }
        let sensme_unread = crate::sensme::merge_cached(&library, &id, &mut read.index.tracks);
        library.classify(&mut read.index.tracks);
        library.players.lock().unwrap().insert(id.clone(), read.index.clone());
        Ok(PlayerLibrary {
            device_id: id,
            source,
            index: read.index,
            sensme_unread,
        })
    })
    .await
    .map_err(ErrorReport::internal)?
    .map_err(|e| ErrorReport::from(&e))
}

#[tauri::command]
pub async fn pull_plan(id: String, tracks: Vec<String>, app: AppHandle) -> Result<PullPlan, ErrorReport> {
    tauri::async_runtime::spawn_blocking(move || -> Result<PullPlan, Error> {
        let devices = app.state::<DeviceService>();
        let library = app.state::<LibraryService>();
        let index =
            library.players.lock().unwrap().get(&id).cloned().ok_or_else(|| {
                Error::unsupported(Why::MusicNotRead, format!("the music on {id} has not been read yet"))
            })?;
        let (_, target) = devices.target(&id)?;
        let dest = library
            .pull_root()
            .ok_or_else(|| Error::unsupported(Why::NoMusicFolder, "this computer has no music folder"))?;
        let plan = pull::build(&index, &tracks, &dest, &id, &target);
        library.pulls.lock().unwrap().insert(plan.id.clone(), plan.clone());
        Ok(plan)
    })
    .await
    .map_err(ErrorReport::internal)?
    .map_err(|e| ErrorReport::from(&e))
}

#[tauri::command]
pub fn pull_start(plan_id: String, app: AppHandle) -> Result<(), ErrorReport> {
    let svc = app.state::<LibraryService>();
    let plan = svc.pulls.lock().unwrap().get(&plan_id).cloned().ok_or_else(|| {
        ErrorReport::refused(
            "no-such-plan",
            Why::PlanExpired,
            "the plan has expired; review it again",
        )
    })?;
    if !plan.fits {
        return Err(ErrorReport::refused(
            "no-space",
            Why::NoRoomComputer,
            "the selection does not fit on this computer",
        ));
    }
    let adb = Adb::locate(None).map_err(|e| ErrorReport::from(&e))?;
    if svc.syncing.swap(true, Ordering::SeqCst) {
        return Err(ErrorReport::refused("busy", Why::Busy, "another transfer is running"));
    }
    svc.cancel_sync.store(false, Ordering::SeqCst);
    let app = app.clone();
    thread::Builder::new()
        .name("music-pull".into())
        .spawn(move || {
            let svc = app.state::<LibraryService>();
            let report = pull::run(&adb, &plan, &svc.cancel_sync, &mut |p: &Progress| {
                let _ = app.emit(
                    "pull-progress",
                    SyncProgressEvent {
                        plan_id: &plan.id,
                        progress: p,
                    },
                );
            });
            tracing::info!(
                "pull from {} into {}: {} copied, {} failed, cancelled {}",
                plan.device_id,
                plan.dest_root,
                report.copied,
                report.failures.len(),
                report.cancelled
            );
            svc.pulls.lock().unwrap().remove(&plan.id);
            svc.syncing.store(false, Ordering::SeqCst);
            if report.copied > 0 {
                let dest = PathBuf::from(&plan.dest_root);
                let mut roots = svc.roots.lock().unwrap();
                if !roots.iter().any(|r| dest.starts_with(r)) {
                    roots.retain(|r| !r.starts_with(&dest));
                    roots.push(dest);
                    drop(roots);
                    svc.save_settings();
                } else {
                    drop(roots);
                }
                start_scan(&app);
            }
            let _ = app.emit(
                "pull-done",
                PullDoneEvent {
                    plan_id: &plan.id,
                    device_id: &plan.device_id,
                    dest_root: &plan.dest_root,
                    report: &report,
                },
            );
        })
        .map_err(ErrorReport::internal)?;
    Ok(())
}
