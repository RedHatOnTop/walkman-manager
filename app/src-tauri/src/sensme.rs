//! SensMe channels: the firmware table the user brings in once, and the analysis read
//! off a player's files.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use uwm_core::library::sensme::{self, PlayerFile, SensMe, Table};
use uwm_core::library::{Codec, Track, hex_id};
use uwm_core::probe::Platform;
use uwm_core::{Error, ErrorReport, Why};

use crate::devices::DeviceService;
use crate::library::{LibraryService, write_atomic};

const SNAPSHOT_AGE: Duration = Duration::from_secs(20);

#[derive(Serialize, Deserialize)]
struct Cached {
    size: u64,
    modified: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sensme: Option<SensMe>,
}

/// By path on the player.
type Cache = HashMap<String, Cached>;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadProgress<'a> {
    device_id: &'a str,
    done: usize,
    total: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadDone<'a> {
    device_id: &'a str,
    read: usize,
    analysed: usize,
    cancelled: bool,
    error: Option<ErrorReport>,
}

fn cache_file(svc: &LibraryService, device_id: &str) -> PathBuf {
    svc.sensme_cache.join(format!("{}.json", hex_id(device_id.as_bytes())))
}

fn load_cache(svc: &LibraryService, device_id: &str) -> Cache {
    std::fs::read(cache_file(svc, device_id))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// The containers Walkman firmware reads the analysis from.
fn may_hold(t: &Track) -> bool {
    matches!(t.format.codec, Codec::Mp3 | Codec::Flac | Codec::Aac | Codec::Alac)
}

fn known(cache: &Cache, t: &Track) -> Option<Option<SensMe>> {
    cache
        .get(&t.path)
        .filter(|c| c.size == t.size && c.modified == t.modified)
        .map(|c| c.sensme.clone())
}

/// Puts what was read off a player before back on its tracks; returns how many files
/// that may hold analysis have not been read yet.
pub(crate) fn merge_cached(svc: &LibraryService, device_id: &str, tracks: &mut [Track]) -> usize {
    let cache = load_cache(svc, device_id);
    let mut unread = 0;
    for t in tracks.iter_mut().filter(|t| may_hold(t)) {
        match known(&cache, t) {
            Some(s) => t.sensme = s,
            None => unread += 1,
        }
    }
    unread
}

/// Analysis just written into a file of a player, with the file's size and time as its
/// track had them.
pub(crate) struct Written {
    pub path: String,
    pub size: u64,
    pub modified: i64,
    pub sensme: SensMe,
}

/// Keeps analysis written into a player's files, so its library shows it without the
/// files being read again; once the player has indexed the new files, they are.
pub(crate) fn remember(svc: &LibraryService, device_id: &str, written: Vec<Written>) {
    let mut cache = load_cache(svc, device_id);
    for w in written {
        cache.insert(
            w.path,
            Cached {
                size: w.size,
                modified: w.modified,
                sensme: Some(w.sensme),
            },
        );
    }
    save_cache(svc, device_id, &cache);
}

fn save_cache(svc: &LibraryService, device_id: &str, cache: &Cache) {
    let file = cache_file(svc, device_id);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec(cache) {
        write_atomic(&file, &json);
    }
}

/// Asks for the firmware's `libMediaStoreService.so` (or a saved table) and keeps a
/// copy of the table in the app's data. False when the picker was closed.
#[tauri::command]
pub async fn sensme_import(app: AppHandle) -> Result<bool, ErrorReport> {
    let picker = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || picker.dialog().file().blocking_pick_file())
        .await
        .map_err(ErrorReport::internal)?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(false);
    };
    let svc = app.state::<LibraryService>();
    let table = Table::load(&path).map_err(|e| ErrorReport::from(&e))?;
    let saved = svc
        .sensme_file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(&svc.sensme_file, table.to_json()));
    saved.map_err(|e| ErrorReport::from(&Error::Io(e)))?;
    tracing::info!("SensMe table taken from {}", path.display());
    *svc.sensme.write().unwrap() = Some(Arc::new(table));
    {
        let mut index = svc.index.lock().unwrap();
        svc.classify(&mut index.tracks);
        svc.save_index(&index);
    }
    for index in svc.players.lock().unwrap().values_mut() {
        svc.classify(&mut index.tracks);
    }
    let _ = app.emit("library", ());
    Ok(true)
}

/// Reads the analysis of a player's files that were not read before. Off the main
/// thread: the player is probed first.
#[tauri::command(async)]
pub fn sensme_read(id: String, app: AppHandle) -> Result<(), ErrorReport> {
    let library = app.state::<LibraryService>();
    let devices = app.state::<DeviceService>();
    let (adb, target, snap) = devices
        .fresh_snapshot(&id, SNAPSHOT_AGE)
        .map_err(|e| ErrorReport::from(&e))?;
    let todo: Vec<(String, u64, i64)> = {
        let players = library.players.lock().unwrap();
        let index = players.get(&id).ok_or_else(|| {
            ErrorReport::refused(
                "unsupported",
                Why::MusicNotRead,
                format!("the music on {id} has not been read yet"),
            )
        })?;
        let cache = load_cache(&library, &id);
        index
            .tracks
            .iter()
            .filter(|t| may_hold(t) && known(&cache, t).is_none())
            .map(|t| (t.path.clone(), t.size, t.modified))
            .collect()
    };
    if library.syncing.swap(true, Ordering::SeqCst) {
        return Err(ErrorReport::refused("busy", Why::Busy, "another transfer is running"));
    }
    library.cancel_sync.store(false, Ordering::SeqCst);
    let walkman_os = snap.system.platform == Platform::WalkmanLinux;
    let app = app.clone();
    thread::Builder::new()
        .name("sensme-read".into())
        .spawn(move || {
            let library = app.state::<LibraryService>();
            let files: Vec<PlayerFile> = todo
                .iter()
                .map(|(path, size, _)| PlayerFile { path, size: *size })
                .collect();
            let total = files.len();
            let result = sensme::read_player(&adb, &target, &files, walkman_os, &library.cancel_sync, &mut |done| {
                let _ = app.emit(
                    "sensme-progress",
                    ReadProgress {
                        device_id: &id,
                        done,
                        total,
                    },
                );
            });
            let (read, analysed, error) = match result {
                Ok(found) => {
                    let mut cache = load_cache(&library, &id);
                    for ((path, size, modified), s) in todo.iter().zip(&found) {
                        cache.insert(
                            path.clone(),
                            Cached {
                                size: *size,
                                modified: *modified,
                                sensme: s.clone(),
                            },
                        );
                    }
                    save_cache(&library, &id, &cache);
                    (found.len(), found.iter().flatten().count(), None)
                }
                Err(e) => (0, 0, Some(ErrorReport::from(&e))),
            };
            let cancelled = library.cancel_sync.load(Ordering::SeqCst);
            tracing::info!("SensMe on {id}: {read} of {total} files read, {analysed} analysed, cancelled {cancelled}");
            library.syncing.store(false, Ordering::SeqCst);
            let _ = app.emit(
                "sensme-done",
                ReadDone {
                    device_id: &id,
                    read,
                    analysed,
                    cancelled,
                    error,
                },
            );
        })
        .map_err(ErrorReport::internal)?;
    Ok(())
}
