use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, State};
use uwm_core::adb::Adb;
use uwm_core::probe::{self, Snapshot};
use uwm_core::registry::{KnownDevice, Registry, Scan};
use uwm_core::{Error, ErrorReport, usb};

/// adb has no hotplug signal for authorization changes, so the worker also
/// rescans on this interval.
const POLL: Duration = Duration::from_millis(2500);
/// USB interfaces and adb settle a moment after a connect.
const SETTLE: Duration = Duration::from_millis(400);
const FOLLOW_UP: Duration = Duration::from_millis(1600);

pub struct DeviceService {
    registry: Arc<Mutex<Registry>>,
    latest: Arc<Mutex<Scan>>,
    wake: Mutex<Sender<()>>,
    known_file: PathBuf,
    snapshots: Mutex<HashMap<String, Snapshot>>,
}

impl DeviceService {
    pub fn start(app: AppHandle, data_dir: PathBuf) -> Self {
        let known_file = data_dir.join("known-devices.json");
        let known: Vec<KnownDevice> = std::fs::read(&known_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let registry = Arc::new(Mutex::new(Registry::new(Adb::locate(None).ok(), known)));
        let latest = Arc::new(Mutex::new(Scan::default()));
        let (tx, rx) = mpsc::channel::<()>();

        let hotplug = tx.clone();
        thread::Builder::new()
            .name("usb-hotplug".into())
            .spawn(move || {
                if let Err(e) = usb::watch(|| {
                    let _ = hotplug.send(());
                }) {
                    tracing::warn!("USB hotplug unavailable, polling only: {e}");
                }
            })
            .expect("spawn usb-hotplug");

        let worker_registry = registry.clone();
        let worker_latest = latest.clone();
        let worker_known_file = known_file.clone();
        thread::Builder::new()
            .name("device-scan".into())
            .spawn(move || {
                let mut follow_up = false;
                loop {
                    let wait = if follow_up { FOLLOW_UP } else { POLL };
                    match rx.recv_timeout(wait) {
                        Ok(()) => {
                            thread::sleep(SETTLE);
                            while rx.try_recv().is_ok() {}
                            follow_up = true;
                        }
                        Err(RecvTimeoutError::Timeout) => follow_up = false,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    let previous = worker_latest.lock().unwrap().clone();
                    let scan = scan_and_store(&worker_registry, &worker_latest, &worker_known_file);
                    if scan.devices != previous.devices || scan.issues != previous.issues {
                        let _ = app.emit("devices", &scan);
                    }
                }
            })
            .expect("spawn device-scan");
        let _ = tx.send(());

        DeviceService {
            registry,
            latest,
            wake: Mutex::new(tx),
            known_file,
            snapshots: Mutex::new(HashMap::new()),
        }
    }

    pub fn target(&self, id: &str) -> Result<(Adb, String), Error> {
        let device = self
            .latest
            .lock()
            .unwrap()
            .devices
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or_else(|| Error::NoSuchDevice(id.to_owned()))?;
        self.registry.lock().unwrap().probe_target(&device)
    }

    /// The device's adb target and a snapshot no older than `max_age`. Blocks while it probes.
    pub fn fresh_snapshot(&self, id: &str, max_age: Duration) -> Result<(Adb, String, Snapshot), Error> {
        let (adb, target) = self.target(id)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        if let Some(s) = self.snapshots.lock().unwrap().get(id)
            && now.saturating_sub(s.taken_at_ms) <= max_age.as_millis() as u64
        {
            return Ok((adb, target, s.clone()));
        }
        let snap = probe::run(&adb, &target)?;
        self.snapshots.lock().unwrap().insert(id.to_owned(), snap.clone());
        Ok((adb, target, snap))
    }

    pub fn summary(&self, id: &str) -> Option<uwm_core::registry::DeviceSummary> {
        self.latest.lock().unwrap().devices.iter().find(|d| d.id == id).cloned()
    }

    /// Takes up an adb that appeared after the start, such as the downloaded platform-tools'.
    pub fn refresh_adb(&self) {
        let mut registry = self.registry.lock().unwrap();
        if registry.adb().is_none() {
            registry.set_adb(Adb::locate(None).ok());
        }
        drop(registry);
        self.wake();
    }

    /// Ask the scanner for a fresh look now, after something changed on USB on purpose.
    pub fn wake(&self) {
        let _ = self.wake.lock().unwrap().send(());
    }

    pub fn forget_snapshot(&self, id: &str) {
        self.snapshots.lock().unwrap().remove(id);
    }
}

fn scan_and_store(registry: &Mutex<Registry>, latest: &Mutex<Scan>, known_file: &Path) -> Scan {
    let (scan, known) = {
        let mut reg = registry.lock().unwrap();
        (reg.scan(), reg.known())
    };
    persist_known(known_file, &known);
    *latest.lock().unwrap() = scan.clone();
    scan
}

fn persist_known(path: &Path, known: &[KnownDevice]) {
    let Ok(json) = serde_json::to_vec_pretty(known) else {
        return;
    };
    if std::fs::read(path).ok().as_deref() == Some(json.as_slice()) {
        return;
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, &json)
        .and_then(|_| std::fs::rename(&tmp, path))
        .is_err()
    {
        tracing::warn!("could not save {}", path.display());
    }
}

#[tauri::command]
pub fn scan_devices(service: State<'_, DeviceService>) -> Scan {
    service.latest.lock().unwrap().clone()
}

#[tauri::command]
pub fn known_devices(service: State<'_, DeviceService>) -> Vec<KnownDevice> {
    service.registry.lock().unwrap().known()
}

#[tauri::command]
pub async fn rescan_devices(service: State<'_, DeviceService>) -> Result<Scan, ErrorReport> {
    let registry = service.registry.clone();
    let latest = service.latest.clone();
    let known_file = service.known_file.clone();
    tauri::async_runtime::spawn_blocking(move || scan_and_store(&registry, &latest, &known_file))
        .await
        .map_err(ErrorReport::internal)
}

#[tauri::command]
pub async fn device_snapshot(id: String, service: State<'_, DeviceService>) -> Result<Snapshot, ErrorReport> {
    let device = service
        .latest
        .lock()
        .unwrap()
        .devices
        .iter()
        .find(|d| d.id == id)
        .cloned()
        .ok_or_else(|| ErrorReport::from(&Error::NoSuchDevice(id.clone())))?;
    let (adb, target) = service
        .registry
        .lock()
        .unwrap()
        .probe_target(&device)
        .map_err(|e| ErrorReport::from(&e))?;
    let snap = tauri::async_runtime::spawn_blocking(move || probe::run(&adb, &target).map(|s| (target, s)))
        .await
        .map_err(ErrorReport::internal)?
        .map_err(|e| ErrorReport::from(&e))?;
    let (target, snap) = snap;
    service.snapshots.lock().unwrap().insert(id, snap.clone());
    let learned = service.registry.lock().unwrap().record(&target, &snap);
    if learned {
        let _ = service.wake.lock().unwrap().send(());
    }
    Ok(snap)
}
