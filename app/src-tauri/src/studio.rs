//! Firmware Studio: the owner's firmware and ROM files, looked at and kept in the vault,
//! and the build kits they fill.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use uwm_core::studio::build::{self as kit_build, Record};
use uwm_core::studio::kit::{self as kit_repo, RepoState};
use uwm_core::studio::licence::{self, LicenceText};
use uwm_core::studio::{EntryView, KitLicence, KitView, KnownFile, Lang, Vault, catalog, fetch, identify};
use uwm_core::{Error, ErrorReport, Why};

use crate::build::BuildProgress;

pub(crate) const PROGRESS_EVERY: Duration = Duration::from_millis(120);

pub struct StudioService {
    pub(crate) vault: Mutex<Vault>,
    /// The Studio's own folder under the app's data.
    pub(crate) dir: PathBuf,
    /// Where files fetched for the owner are saved.
    downloads: PathBuf,
    busy: AtomicBool,
    cancel: AtomicBool,
    /// The folder the owner named for each kit's repository.
    pub(crate) repos: Mutex<BTreeMap<String, PathBuf>>,
    pub(crate) building: Mutex<Option<BuildProgress>>,
    pub(crate) build_cancel: AtomicBool,
    /// Each licence's text as last shown to the owner, by its id.
    licences: Mutex<BTreeMap<String, LicenceText>>,
}

impl StudioService {
    pub fn new(data_dir: &Path) -> Self {
        let dir = data_dir.join("studio");
        let repos = std::fs::read(dir.join("kit-repos.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        kit_build::settle_interrupted(&dir.join("builds"));
        StudioService {
            vault: Mutex::new(Vault::open(dir.join("vault.json"))),
            downloads: dir.join("files"),
            busy: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            repos: Mutex::new(repos),
            building: Mutex::new(None),
            build_cancel: AtomicBool::new(false),
            licences: Mutex::new(BTreeMap::new()),
            dir,
        }
    }

    pub(crate) fn builds_dir(&self) -> PathBuf {
        self.dir.join("builds")
    }

    /// The UPG key read from the owner's player, one per player family.
    pub(crate) fn key_file(&self, profile: &str) -> PathBuf {
        self.dir.join("keys").join(format!("{profile}.cfg"))
    }

    pub(crate) fn repo_state(&self, kit: &str) -> Option<RepoState> {
        let source = catalog::kit(kit)?.source.as_ref()?;
        if let Some(archive) = &source.archive {
            return Some(RepoState::Published {
                have: kit_repo::have(&self.dir.join("kits"), kit, source),
                bytes: archive.bytes,
            });
        }
        let repos = self.repos.lock().unwrap();
        Some(kit_repo::check(repos.get(kit).map(PathBuf::as_path), source))
    }

    pub(crate) fn save_repos(&self) -> Result<(), Error> {
        let repos = self.repos.lock().unwrap();
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join("kit-repos.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&*repos).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, self.dir.join("kit-repos.json"))?;
        Ok(())
    }

    /// The owner agreed to the licence the kit's downloads come under, or it has none.
    pub(crate) fn licence_agreed(&self, kit: &str) -> bool {
        catalog::kit(kit)
            .and_then(|k| k.licence.as_ref())
            .is_none_or(|l| licence::agreed(&self.dir).contains_key(&l.id))
    }

    pub(crate) fn view(&self) -> StudioView {
        let (kits, files) = {
            let vault = self.vault.lock().unwrap();
            (vault.kits(), vault.view())
        };
        let repos = kits
            .iter()
            .filter_map(|k| Some((k.id.clone(), self.repo_state(&k.id)?)))
            .collect();
        let keys = kits
            .iter()
            .filter(|k| needs_key(&k.id))
            .map(|k| (k.profile.clone(), self.key_file(&k.profile).is_file()))
            .collect();
        let licences = kits
            .iter()
            .filter(|k| catalog::kit(&k.id).is_some_and(|c| c.licence.is_some()))
            .map(|k| (k.id.clone(), self.licence_agreed(&k.id)))
            .collect();
        StudioView {
            kits,
            files,
            catalog: catalog::files(),
            downloads: self.downloads.to_string_lossy().into_owned(),
            repos,
            keys,
            licences,
            builds: kit_build::list(&self.builds_dir()),
            building: self.building.lock().unwrap().clone(),
        }
    }
}

/// The kit's entry point takes the player's UPG key.
pub(crate) fn needs_key(kit: &str) -> bool {
    catalog::kit(kit)
        .and_then(|k| k.source.as_ref())
        .is_some_and(|s| s.args.iter().any(|a| a == "key"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioView {
    kits: Vec<KitView>,
    files: Vec<EntryView>,
    /// Every file the Studio knows, for naming vault files no kit takes.
    catalog: &'static [KnownFile],
    /// The folder fetched files go to.
    downloads: String,
    /// Each buildable kit's repository on this computer.
    repos: BTreeMap<String, RepoState>,
    /// Whether the UPG key of each player family that needs one has been read.
    keys: BTreeMap<String, bool>,
    /// Whether the owner agreed to the licence of each kit that has one.
    licences: BTreeMap<String, bool>,
    /// Every build, newest first.
    builds: Vec<Record>,
    /// The build running now.
    building: Option<BuildProgress>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioProgress {
    /// The path of the file being looked at, or the id of the known file being fetched.
    name: String,
    fetching: bool,
    done: u64,
    total: u64,
    index: usize,
    count: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Failure {
    name: String,
    /// The known file a failed download was for.
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    error: ErrorReport,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioDone {
    /// Vault entries added or looked at again.
    added: Vec<String>,
    failed: Vec<Failure>,
    cancelled: bool,
}

enum Job {
    Look(Vec<PathBuf>),
    Fetch(&'static KnownFile),
}

#[tauri::command(async)]
pub fn studio_view(app: AppHandle) -> StudioView {
    app.state::<StudioService>().view()
}

/// Looks at files dropped on the window or chosen elsewhere, and keeps them.
#[tauri::command(async)]
pub fn studio_add(paths: Vec<String>, app: AppHandle) -> Result<(), ErrorReport> {
    start(app, Job::Look(paths.into_iter().map(PathBuf::from).collect()))
}

/// False when the picker was closed.
#[tauri::command]
pub async fn studio_pick(app: AppHandle) -> Result<bool, ErrorReport> {
    let picker = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || picker.dialog().file().blocking_pick_files())
        .await
        .map_err(ErrorReport::internal)?;
    let paths: Vec<PathBuf> = picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .collect();
    if paths.is_empty() {
        return Ok(false);
    }
    start(app, Job::Look(paths))?;
    Ok(true)
}

/// Downloads a known file from its publisher and keeps it.
#[tauri::command(async)]
pub fn studio_fetch(file: String, app: AppHandle) -> Result<(), ErrorReport> {
    let known = catalog::file(&file)
        .ok_or_else(|| ErrorReport::refused("unknown-file", Why::UnknownFile, format!("no known file {file}")))?;
    start(app, Job::Fetch(known))
}

/// Forgets a file; the file itself stays where it is.
#[tauri::command(async)]
pub fn studio_remove(id: String, app: AppHandle) -> Result<StudioView, ErrorReport> {
    let svc = app.state::<StudioService>();
    svc.vault
        .lock()
        .unwrap()
        .remove(&id)
        .map_err(|e| ErrorReport::from(&e))?;
    Ok(svc.view())
}

#[tauri::command]
pub fn studio_cancel(app: AppHandle) {
    app.state::<StudioService>().cancel.store(true, Ordering::SeqCst);
}

/// The licence text the kit's downloads come under, from its publisher.
#[tauri::command(async)]
pub fn studio_licence(kit: String, app: AppHandle) -> Result<LicenceText, ErrorReport> {
    let l = kit_licence(&kit)?;
    let text = licence::read(&l.id, &l.from).map_err(|e| ErrorReport::from(&e))?;
    let svc = app.state::<StudioService>();
    svc.licences.lock().unwrap().insert(l.id.clone(), text.clone());
    Ok(text)
}

/// Keeps the owner's agreement to the text [`studio_licence`] showed, named by its sha256.
#[tauri::command(async)]
pub fn studio_agree(kit: String, sha256: String, app: AppHandle) -> Result<StudioView, ErrorReport> {
    let l = kit_licence(&kit)?;
    let svc = app.state::<StudioService>();
    let shown = svc.licences.lock().unwrap().get(&l.id).cloned();
    let text = shown.filter(|t| t.sha256 == sha256).ok_or_else(|| {
        ErrorReport::refused(
            "licence",
            Why::LicenceChanged,
            format!("{} is not the text last shown for {}", sha256, l.id),
        )
    })?;
    licence::agree(&svc.dir, &text).map_err(|e| ErrorReport::from(&e))?;
    Ok(svc.view())
}

fn kit_licence(kit: &str) -> Result<&'static KitLicence, ErrorReport> {
    catalog::kit(kit)
        .and_then(|k| k.licence.as_ref())
        .ok_or_else(|| ErrorReport::refused("licence", Why::LicenceNotFound, format!("{kit} has no licence")))
}

/// Opens the guide's section on where to get a known file, in the browser.
#[tauri::command(async)]
pub fn studio_open_guide(file: String, lang: Lang) -> Result<(), ErrorReport> {
    let link = catalog::file(&file)
        .and_then(|k| k.guide_url(lang))
        .ok_or_else(|| ErrorReport::refused("no-link", Why::NoGuide, format!("{file} has no guide section")))?;
    open_in_browser(&link).map_err(|e| ErrorReport::from(&e))
}

/// Opens the guide's section on making or getting a kit input's file.
#[tauri::command(async)]
pub fn studio_open_input_guide(kit: String, input: String, lang: Lang) -> Result<(), ErrorReport> {
    let link = catalog::kit(&kit)
        .and_then(|k| k.inputs.iter().find(|i| i.id == input))
        .and_then(|i| i.guide_url(lang))
        .ok_or_else(|| ErrorReport::refused("no-link", Why::NoGuide, format!("{kit}/{input} has no guide section")))?;
    open_in_browser(&link).map_err(|e| ErrorReport::from(&e))
}

pub(crate) fn open_in_browser(url: &str) -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    let (tool, args) = ("xdg-open", vec![url]);
    #[cfg(target_os = "macos")]
    let (tool, args) = ("open", vec![url]);
    #[cfg(target_os = "windows")]
    let (tool, args) = ("explorer", vec![url]);
    std::process::Command::new(tool)
        .args(args)
        .spawn()
        .map(drop)
        .map_err(|source| Error::Spawn { tool, source })
}

fn start(app: AppHandle, job: Job) -> Result<(), ErrorReport> {
    let svc = app.state::<StudioService>();
    if svc.busy.swap(true, Ordering::SeqCst) {
        return Err(ErrorReport::refused(
            "busy",
            Why::StudioBusy,
            "the Studio is still looking at other files",
        ));
    }
    svc.cancel.store(false, Ordering::SeqCst);
    let worker = app.clone();
    thread::Builder::new()
        .name("studio".into())
        .spawn(move || {
            let done = run(&worker, job);
            worker.state::<StudioService>().busy.store(false, Ordering::SeqCst);
            let _ = worker.emit("studio-done", done);
        })
        .map(drop)
        .map_err(|e| {
            app.state::<StudioService>().busy.store(false, Ordering::SeqCst);
            ErrorReport::internal(e)
        })
}

fn run(app: &AppHandle, job: Job) -> StudioDone {
    let svc = app.state::<StudioService>();
    let mut done = StudioDone {
        added: Vec::new(),
        failed: Vec::new(),
        cancelled: false,
    };
    let (paths, fetched_from) = match job {
        Job::Look(paths) => (paths, None),
        Job::Fetch(known) => {
            let mut emit = throttled(app, known.id.clone(), true, 0, 1);
            match fetch::fetch(known, &svc.downloads, &svc.cancel, &mut emit) {
                Ok(path) => (vec![path], Some(known)),
                Err(e) => {
                    done.cancelled = matches!(e, Error::Cancelled);
                    if !done.cancelled {
                        done.failed.push(Failure {
                            name: known.id.clone(),
                            file: Some(known.id.clone()),
                            error: ErrorReport::from(&e),
                        });
                    }
                    return done;
                }
            }
        }
    };
    let count = paths.len();
    for (index, path) in paths.into_iter().enumerate() {
        let name = path.to_string_lossy().into_owned();
        let mut emit = throttled(app, name.clone(), false, index, count);
        let looked = identify::identify(&path, &svc.cancel, &mut emit).and_then(|found| {
            if let Some(known) = fetched_from
                && found.known.as_deref() != Some(known.id.as_str())
            {
                return Err(Error::unsupported(
                    Why::NotPinnedFile,
                    format!(
                        "the download is not {}: its content differs from the pinned one",
                        known.id
                    ),
                ));
            }
            svc.vault.lock().unwrap().add(found)
        });
        match looked {
            Ok(entry) => {
                done.added.push(entry.id);
                let _ = app.emit("studio-changed", ());
            }
            Err(Error::Cancelled) => {
                done.cancelled = true;
                break;
            }
            Err(e) => done.failed.push(Failure {
                name,
                file: None,
                error: ErrorReport::from(&e),
            }),
        }
    }
    tracing::info!(
        "studio: {} kept, {} failed, cancelled {}",
        done.added.len(),
        done.failed.len(),
        done.cancelled
    );
    done
}

fn throttled(app: &AppHandle, name: String, fetching: bool, index: usize, count: usize) -> impl FnMut(u64, u64) {
    let app = app.clone();
    let mut last = Instant::now() - PROGRESS_EVERY;
    move |done, total| {
        if done < total && last.elapsed() < PROGRESS_EVERY {
            return;
        }
        last = Instant::now();
        let _ = app.emit(
            "studio-progress",
            StudioProgress {
                name: name.clone(),
                fetching,
                done,
                total,
                index,
                count,
            },
        );
    }
}
