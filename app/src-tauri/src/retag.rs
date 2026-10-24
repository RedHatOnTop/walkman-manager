use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use uwm_core::adb::Adb;
use uwm_core::library::lookup::{self, Candidate, Client, Field, Proposal, Query, Release, SearchResult};
use uwm_core::library::retag::{self, Place, RetagItem, RetagJob};
use uwm_core::library::tags::{self, Cover};
use uwm_core::library::transfer::{Progress, Report};
use uwm_core::library::{Track, hex_id};
use uwm_core::probe::Platform;
use uwm_core::{Error, ErrorReport, Why};

use crate::devices::DeviceService;
use crate::library::{LibraryService, start_scan};

const SNAPSHOT_AGE: Duration = Duration::from_secs(20);

struct Stored {
    proposal: Proposal,
    release: Release,
    device_id: Option<String>,
    tracks: Vec<Track>,
    cover: Option<Cover>,
}

pub struct LookupService {
    client: Client,
    candidates: Mutex<HashMap<String, Candidate>>,
    releases: Mutex<HashMap<String, Release>>,
    proposals: Mutex<HashMap<String, Stored>>,
    backups: PathBuf,
    work: PathBuf,
}

impl LookupService {
    pub fn new(data_dir: &std::path::Path, cache_dir: &std::path::Path) -> Self {
        LookupService {
            client: Client::new(),
            candidates: Mutex::new(HashMap::new()),
            releases: Mutex::new(HashMap::new()),
            proposals: Mutex::new(HashMap::new()),
            backups: data_dir.join("backups"),
            work: cache_dir.join("retag"),
        }
    }

    fn candidate(&self, id: &str) -> Result<Candidate, Error> {
        self.candidates
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| Error::unsupported(Why::ResultExpired, "that result has expired; search again"))
    }

    fn release(&self, id: &str) -> Result<Release, Error> {
        if let Some(r) = self.releases.lock().unwrap().get(id) {
            return Ok(r.clone());
        }
        let r = self.client.release(&self.candidate(id)?)?;
        self.releases.lock().unwrap().insert(id.to_owned(), r.clone());
        Ok(r)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverView {
    art: String,
    width: u32,
    height: u32,
    bytes: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalView {
    id: String,
    #[serde(flatten)]
    proposal: Proposal,
    cover: Option<CoverView>,
    device_id: Option<String>,
    /// The album's folder, where the files are written.
    folder: String,
    /// Player only: where the originals are kept before anything is written.
    backup_root: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RetagProgressEvent<'a> {
    id: &'a str,
    progress: &'a Progress,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RetagDoneEvent<'a> {
    id: &'a str,
    device_id: Option<&'a str>,
    report: &'a Report,
    backup: Option<String>,
}

#[tauri::command]
pub async fn lookup_search(query: Query, app: AppHandle) -> Result<SearchResult, ErrorReport> {
    tauri::async_runtime::spawn_blocking(move || {
        let svc = app.state::<LookupService>();
        let found = svc.client.search(&query)?;
        let mut known = svc.candidates.lock().unwrap();
        for c in &found.candidates {
            known.insert(c.id.clone(), c.clone());
        }
        Ok::<_, Error>(found)
    })
    .await
    .map_err(ErrorReport::internal)?
    .map_err(|e| ErrorReport::from(&e))
}

/// A search result's cover as an art key, fetched once.
#[tauri::command]
pub async fn lookup_cover(id: String, app: AppHandle) -> Result<Option<String>, ErrorReport> {
    tauri::async_runtime::spawn_blocking(move || {
        let svc = app.state::<LookupService>();
        let library = app.state::<LibraryService>();
        let key = hex_id(format!("lookup-thumb\0{id}").as_bytes());
        if library.art.path(&key).is_some() {
            return Ok(Some(key));
        }
        let Some(url) = svc.candidate(&id)?.thumb_url else {
            return Ok(None);
        };
        match svc.client.image(&url) {
            Ok(bytes) => Ok(library.art.store_as(&key, &bytes)),
            Err(Error::Network(m)) if m.contains("404") => Ok(None),
            Err(e) => Err(e),
        }
    })
    .await
    .map_err(ErrorReport::internal)?
    .map_err(|e: Error| ErrorReport::from(&e))
}

#[tauri::command]
pub async fn lookup_propose(
    candidate: String,
    album: String,
    device: Option<String>,
    app: AppHandle,
) -> Result<ProposalView, ErrorReport> {
    tauri::async_runtime::spawn_blocking(move || -> Result<ProposalView, Error> {
        let svc = app.state::<LookupService>();
        let library = app.state::<LibraryService>();
        let index = match &device {
            Some(id) => library.players.lock().unwrap().get(id).cloned().ok_or_else(|| {
                Error::unsupported(Why::MusicNotRead, format!("the music on {id} has not been read yet"))
            })?,
            None => library.index.lock().unwrap().clone(),
        };
        let found = index
            .albums
            .iter()
            .find(|a| a.id == album)
            .ok_or_else(|| Error::unsupported(Why::AlbumGone, "that album is no longer in the library"))?;
        let by_id = index.tracks_by_id();
        let tracks: Vec<Track> = found
            .tracks
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).map(|t| (*t).clone()))
            .collect();
        let release = svc.release(&candidate)?;
        let refs: Vec<&Track> = tracks.iter().collect();
        let proposal = lookup::propose(&refs, &release, |t| tags::writable(std::path::Path::new(&t.path)));

        let mut cover = None;
        let mut view = None;
        if let Some(url) = &release.candidate.cover_url {
            match svc.client.image(url) {
                Ok(bytes) => {
                    if let Some((c, width, height)) = lookup::cover_from(bytes) {
                        let key = hex_id(format!("lookup-cover\0{candidate}").as_bytes());
                        if let Some(art) = library.art.store_as(&key, &c.data) {
                            view = Some(CoverView {
                                art,
                                width,
                                height,
                                bytes: c.data.len(),
                            });
                            cover = Some(c);
                        }
                    }
                }
                Err(e) => tracing::info!("no cover for {candidate}: {e}"),
            }
        }

        let id = hex_id(format!("retag\0{candidate}\0{album}").as_bytes());
        let backup_root = device.as_ref().map(|_| svc.backups.to_string_lossy().into_owned());
        let result = ProposalView {
            id: id.clone(),
            proposal: proposal.clone(),
            cover: view,
            device_id: device.clone(),
            folder: found.folder.clone(),
            backup_root,
        };
        svc.proposals.lock().unwrap().insert(
            id,
            Stored {
                proposal,
                release,
                device_id: device,
                tracks,
                cover,
            },
        );
        Ok(result)
    })
    .await
    .map_err(ErrorReport::internal)?
    .map_err(|e| ErrorReport::from(&e))
}

/// Off the main thread: a player is probed first, which takes a moment.
#[tauri::command(async)]
pub fn retag_start(
    id: String,
    album_fields: Vec<Field>,
    tracks: Vec<String>,
    cover: bool,
    app: AppHandle,
) -> Result<(), ErrorReport> {
    let svc = app.state::<LookupService>();
    let library = app.state::<LibraryService>();
    let stored = svc.proposals.lock().unwrap().remove(&id).ok_or_else(|| {
        ErrorReport::refused(
            "no-such-plan",
            Why::ProposalExpired,
            "the proposal has expired; look the album up again",
        )
    })?;
    let cover_data = stored.cover.clone().filter(|_| cover);
    let items: Vec<RetagItem> = stored
        .proposal
        .tracks
        .iter()
        .filter(|t| t.writable)
        .filter_map(|t| {
            let file = stored.tracks.iter().find(|f| f.id == t.track_id)?;
            let edit = lookup::edit_for(
                &stored.proposal,
                &stored.release,
                t,
                &album_fields,
                tracks.contains(&t.track_id),
            );
            let wanted = edit != Default::default() || t.repairs || cover_data.is_some();
            wanted.then(|| RetagItem {
                track_id: t.track_id.clone(),
                path: file.path.clone(),
                rel: file.rel.clone(),
                size: file.size,
                edit,
                cover: cover_data.is_some(),
            })
        })
        .collect();
    if items.is_empty() {
        return Err(ErrorReport::refused(
            "nothing",
            Why::NothingChosen,
            "nothing is chosen to write",
        ));
    }

    let (place, backup) = match &stored.device_id {
        Some(device_id) => {
            let devices = app.state::<DeviceService>();
            let (_, target, snap) = devices
                .fresh_snapshot(device_id, SNAPSHOT_AGE)
                .map_err(|e| ErrorReport::from(&e))?;
            let name = snap.identity.model.clone().unwrap_or_else(|| device_id.clone());
            (
                Place::Player {
                    device_id: device_id.clone(),
                    adb_target: target,
                    android: snap.system.platform == Platform::Android,
                },
                Some(retag::backup_dir(&svc.backups, &name)),
            )
        }
        None => (Place::Computer, None),
    };
    let adb = match place {
        Place::Player { .. } => Some(Adb::locate(None).map_err(|e| ErrorReport::from(&e))?),
        Place::Computer => None,
    };
    if library.syncing.swap(true, Ordering::SeqCst) {
        return Err(ErrorReport::refused("busy", Why::Busy, "another transfer is running"));
    }
    library.cancel_sync.store(false, Ordering::SeqCst);
    let job = RetagJob {
        id,
        place,
        items,
        cover: cover_data,
    };
    let work = svc.work.clone();
    let app = app.clone();
    thread::Builder::new()
        .name("retag".into())
        .spawn(move || {
            let library = app.state::<LibraryService>();
            let mut emit = |p: &Progress| {
                let _ = app.emit(
                    "retag-progress",
                    RetagProgressEvent {
                        id: &job.id,
                        progress: p,
                    },
                );
            };
            let report = match (&adb, &backup) {
                (Some(adb), Some(backup)) => {
                    retag::run_player(adb, &job, &work, backup, &library.cancel_sync, &mut emit)
                }
                _ => retag::run_computer(&job, &library.cancel_sync, &mut emit),
            };
            let device_id = match &job.place {
                Place::Player { device_id, .. } => Some(device_id.as_str()),
                Place::Computer => None,
            };
            tracing::info!(
                "retag {} on {}: {} written, {} failed, cancelled {}",
                job.id,
                device_id.unwrap_or("this computer"),
                report.copied,
                report.failures.len(),
                report.cancelled
            );
            library.syncing.store(false, Ordering::SeqCst);
            if device_id.is_none() && report.copied > 0 {
                start_scan(&app);
            }
            let kept = backup.filter(|b| b.is_dir()).map(|b| b.to_string_lossy().into_owned());
            let _ = app.emit(
                "retag-done",
                RetagDoneEvent {
                    id: &job.id,
                    device_id,
                    report: &report,
                    backup: kept,
                },
            );
        })
        .map_err(ErrorReport::internal)?;
    Ok(())
}

#[tauri::command]
pub fn retag_forget(id: String, svc: State<'_, LookupService>) {
    svc.proposals.lock().unwrap().remove(&id);
}
