//! Firmware Studio builds: the kit's repository and the player's UPG key, the host check, and
//! one build at a time of a kit's entry point, with its progress as events.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use uwm_core::studio::build::{self as kit_build, Event, Options, Record, Request, UsedFile, Variant};
use uwm_core::studio::doctor::{self, Check};
use uwm_core::studio::kit as kit_repo;
use uwm_core::studio::{Kit, catalog};
use uwm_core::{Error, ErrorReport, Why};

use crate::devices::DeviceService;
use crate::studio::{StudioService, StudioView, needs_key};

/// The player keeps the key its firmware files are encrypted with here; stock and the GSI
/// builds alike let the shell user read it.
const NVP: &str = "/vendor/usr/data/icx_nvp.cfg";
const LINES: usize = 14;
const LINE_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildProgress {
    pub id: Option<String>,
    pub kit: String,
    pub options: Options,
    /// Seconds since the epoch.
    pub started: i64,
    /// `fetch` while the kit's published archive downloads or `copy` while its files are taken
    /// from the owner's checkout, then `run`.
    pub phase: &'static str,
    /// Bytes of the kit's archive downloaded so far, and in all.
    pub fetched: u64,
    pub fetch_total: u64,
    pub index: usize,
    pub total: usize,
    pub step: Option<String>,
    /// When the current step started, ms since the epoch.
    pub step_started: i64,
    /// The latest lines of the log.
    pub lines: VecDeque<String>,
}

fn err(e: Error) -> ErrorReport {
    ErrorReport::from(&e)
}

fn kit_of(id: &str) -> Result<&'static Kit, ErrorReport> {
    catalog::kit(id)
        .filter(|k| k.source.is_some())
        .ok_or_else(|| ErrorReport::refused("no-steps", Why::KitHasNoSteps, format!("{id} has no build steps")))
}

/// Asks for the folder of the kit's repository; false when the picker was closed.
#[tauri::command]
pub async fn studio_pick_repo(kit: String, app: AppHandle) -> Result<StudioView, ErrorReport> {
    kit_of(&kit)?;
    let picker = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || picker.dialog().file().blocking_pick_folder())
        .await
        .map_err(ErrorReport::internal)?;
    let svc = app.state::<StudioService>();
    if let Some(dir) = picked.and_then(|p| p.into_path().ok()) {
        svc.repos.lock().unwrap().insert(kit, dir);
        svc.save_repos().map_err(err)?;
    }
    Ok(svc.view())
}

/// Reads the UPG key from the connected player into the Studio's keys. Only the key's own
/// line is kept; the file it comes from holds the unit's other settings too.
#[tauri::command(async)]
pub fn studio_read_key(device: String, app: AppHandle) -> Result<StudioView, ErrorReport> {
    let devices = app.state::<DeviceService>();
    let (adb, target, snap) = devices.fresh_snapshot(&device, Duration::from_secs(60)).map_err(err)?;
    let profile = snap
        .identity
        .profile_id
        .clone()
        .ok_or_else(|| ErrorReport::refused("unknown-player", Why::UnknownPlayer, "the player has no profile"))?;
    let text = adb
        .shell(&target, &format!("cat {NVP}"), Duration::from_secs(15))
        .map_err(err)?;
    let line = upg_key_line(&text).ok_or_else(|| {
        ErrorReport::refused(
            "not-upg-key",
            Why::NotUpgKey,
            format!("{NVP} holds no 48-character _KAS_ field"),
        )
    })?;
    let svc = app.state::<StudioService>();
    let file = svc.key_file(&profile);
    write_private(&file, format!("{line}\n").as_bytes()).map_err(err)?;
    tracing::info!("studio: UPG key for {profile} read from the player");
    Ok(svc.view())
}

/// `_KAS_="<48 characters>"`, the field decrypt-upg.sh reads.
fn upg_key_line(cfg: &str) -> Option<String> {
    cfg.lines().find_map(|l| {
        let rest = l.trim().strip_prefix("_KAS_=")?.trim();
        let q = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let inner = rest[1..].split(q).next()?;
        (inner.len() == 48 && inner.is_ascii() && !inner.contains(char::is_whitespace))
            .then(|| format!("_KAS_=\"{inner}\""))
    })
}

fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<(), Error> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    opts.open(&tmp)?.write_all(bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostView {
    checks: Vec<Check>,
    ok: bool,
}

/// What the kit needs from this computer, looked up now.
#[tauri::command(async)]
pub fn studio_host(kit: String, app: AppHandle) -> Result<HostView, ErrorReport> {
    let k = kit_of(&kit)?;
    let svc = app.state::<StudioService>();
    let Some(needs) = &k.host else {
        return Ok(HostView {
            checks: Vec::new(),
            ok: true,
        });
    };
    let (mut checks, _) = doctor::host(needs, &svc.dir, dirs::home_dir().as_deref());
    checks.push(adb_key_check());
    let ok = checks.iter().all(|c| c.ok || c.kind == "adbKey");
    Ok(HostView { checks, ok })
}

/// This computer's adb key, which only a debug build takes.
fn adb_key_check() -> Check {
    let key = adb_public_key();
    Check {
        kind: "adbKey",
        name: "adbkey.pub".into(),
        ok: key.as_ref().is_some_and(|k| k.is_file()),
        found: key.filter(|k| k.is_file()).map(|k| k.to_string_lossy().into_owned()),
    }
}

/// Where adb keeps the key it signs in with: `ANDROID_USER_HOME`, else `~/.android`.
fn adb_public_key() -> Option<PathBuf> {
    std::env::var_os("ANDROID_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".android")))
        .map(|d| d.join("adbkey.pub"))
}

#[tauri::command(async)]
pub fn studio_build(kit: String, mut options: Options, app: AppHandle) -> Result<(), ErrorReport> {
    let k = kit_of(&kit)?;
    let svc = app.state::<StudioService>();
    let source = k.source.as_ref().expect("kit_of keeps kits with steps");
    let stock = options.variant == Variant::Stock;
    if stock || !source.variants {
        options.root = false;
    }
    if !source.variants && options.variant == Variant::Debug {
        return Err(ErrorReport::refused(
            "no-steps",
            Why::KitHasNoSteps,
            format!("{kit} makes release builds only"),
        ));
    }
    let (_, steps) = source.steps(options.variant).ok_or_else(|| {
        ErrorReport::refused("no-steps", Why::KitHasNoSteps, format!("{kit} cannot go back to stock"))
    })?;
    let taken = source.inputs(options.variant);

    let view = svc.vault.lock().unwrap().kits().into_iter().find(|v| v.id == kit);
    let view = view
        .filter(|v| if stock { v.restore == Some(true) } else { v.ready })
        .ok_or_else(|| ErrorReport::refused("inputs", Why::KitInputsMissing, format!("{kit} is missing files")))?;
    let repo = match svc.repo_state(&kit) {
        Some(kit_repo::RepoState::Ready { path }) => Some(PathBuf::from(path)),
        Some(kit_repo::RepoState::Published { .. }) => None,
        _ => {
            return Err(ErrorReport::refused(
                "repo",
                Why::KitRepoNotReady,
                format!("no checkout of {} with {}", source.repository, source.commit),
            ));
        }
    };
    // Going back downloads nothing, so nothing of the licence's applies to it.
    if !stock && !svc.licence_agreed(&kit) {
        return Err(ErrorReport::refused(
            "licence",
            Why::LicenceNotAgreed,
            format!("{kit}: the licence of its downloads has not been agreed to"),
        ));
    }
    let key = svc.key_file(&k.profile);
    if needs_key(&kit) && !key.is_file() {
        return Err(ErrorReport::refused(
            "key",
            Why::NoUpgKey,
            "the player's UPG key has not been read",
        ));
    }
    let needs = k.host.as_ref();
    let (found, missing) = match needs {
        Some(n) => {
            let (checks, found) = doctor::host(n, &svc.dir, dirs::home_dir().as_deref());
            let missing: Vec<String> = checks.iter().filter(|c| !c.ok).map(|c| c.name.clone()).collect();
            (found, missing)
        }
        None => (Default::default(), Vec::new()),
    };
    if !missing.is_empty() {
        let mut e = ErrorReport::refused("host", Why::HostNotReady, format!("missing: {}", missing.join(", ")));
        e.detail = Some(missing.join(", "));
        return Err(e);
    }
    let adb_key = match options.variant {
        Variant::Debug => Some(adb_public_key().filter(|k| k.is_file()).ok_or_else(|| {
            ErrorReport::refused("adb-key", Why::NoAdbKey, "a debug build needs this computer's adb key")
        })?),
        Variant::Release | Variant::Stock => None,
    };

    let entries = svc.vault.lock().unwrap().view();
    let file_of = |input: &str| {
        let id = view.inputs.iter().find(|i| i.id == input)?.filled_by.clone()?;
        entries.iter().find(|e| e.entry.id == id).map(|e| e.entry.clone())
    };
    let stock_entry = file_of("stock");
    let mut args = Vec::new();
    for a in steps {
        let path = match a.as_str() {
            "key" => key.clone(),
            "stock" => {
                let tag = stock_entry
                    .as_ref()
                    .map_or("stock".into(), |e| e.file.sha256[..16].to_owned());
                svc.dir.join("stock").join(tag)
            }
            other => {
                let input = other.strip_prefix("input:").unwrap_or(other);
                file_of(input)
                    .ok_or_else(|| {
                        ErrorReport::refused("inputs", Why::KitInputsMissing, format!("{kit}: no file for {input}"))
                    })?
                    .file
                    .path
            }
        };
        args.push(path);
    }
    let inputs: Vec<UsedFile> = view
        .inputs
        .iter()
        .filter(|i| taken.contains(&i.id.as_str()))
        .filter_map(|i| {
            let e = file_of(&i.id)?;
            Some(UsedFile {
                input: i.id.clone(),
                path: e.file.path,
                sha256: e.file.sha256,
            })
        })
        .collect();

    {
        let mut building = svc.building.lock().unwrap();
        if building.is_some() {
            return Err(ErrorReport::refused("busy", Why::StudioBusy, "a build is running"));
        }
        *building = Some(BuildProgress {
            id: None,
            kit: kit.clone(),
            options,
            started: now_secs(),
            phase: if repo.is_some() { "copy" } else { "fetch" },
            fetched: 0,
            fetch_total: 0,
            index: 0,
            total: 0,
            step: None,
            step_started: now_ms(),
            lines: VecDeque::new(),
        });
    }
    svc.build_cancel.store(false, Ordering::SeqCst);
    let _ = app.emit("studio-build", svc.building.lock().unwrap().clone());

    let worker = app.clone();
    let spawned = thread::Builder::new().name("studio-build".into()).spawn(move || {
        let svc = worker.state::<StudioService>();
        let result = (|| -> Result<Record, Error> {
            let kits = svc.dir.join("kits");
            let tools = svc.dir.join("tools").join(&k.id);
            let mut fetch_emit = Instant::now() - LINE_EVERY;
            let kit_dir = match &repo {
                Some(repo) => kit_repo::copy(repo, &k.id, source, &kits, &tools)?,
                None => kit_repo::download(&k.id, source, &kits, &tools, &svc.build_cancel, &mut |done, total| {
                    let mut guard = svc.building.lock().unwrap();
                    let Some(p) = guard.as_mut() else { return };
                    p.fetched = done;
                    p.fetch_total = total;
                    if fetch_emit.elapsed() >= LINE_EVERY || done == total {
                        fetch_emit = Instant::now();
                        let _ = worker.emit("studio-build", p.clone());
                    }
                })?,
            };
            let mut last_emit = Instant::now() - LINE_EVERY;
            let builds = svc.builds_dir();
            kit_build::run(
                Request {
                    kit: k,
                    kit_dir: &kit_dir,
                    args,
                    inputs,
                    builds_dir: &builds,
                    options,
                    ndk: found.ndk.as_deref(),
                    sdk: found.sdk.as_deref(),
                    adb_key: adb_key.as_deref(),
                    licence_env: k.licence.as_ref().filter(|_| !stock).map(|l| l.env.as_str()),
                    cache: Some(&tools),
                },
                &svc.build_cancel,
                &mut |e| {
                    let mut guard = svc.building.lock().unwrap();
                    let Some(p) = guard.as_mut() else { return };
                    let emit = match e {
                        Event::Started(r) => {
                            p.id = Some(r.id.clone());
                            p.phase = "run";
                            true
                        }
                        Event::Step { index, total, name } => {
                            p.index = index;
                            p.total = total;
                            p.step = Some(name.to_owned());
                            p.step_started = now_ms();
                            true
                        }
                        Event::Line(l) => {
                            if p.lines.len() == LINES {
                                p.lines.pop_front();
                            }
                            p.lines.push_back(l.to_owned());
                            last_emit.elapsed() >= LINE_EVERY
                        }
                    };
                    if emit {
                        last_emit = Instant::now();
                        let _ = worker.emit("studio-build", p.clone());
                    }
                },
            )
        })();
        *svc.building.lock().unwrap() = None;
        match result {
            Ok(record) => {
                tracing::info!("studio: build {} ended {:?}", record.id, record.outcome);
                let _ = worker.emit(
                    "studio-build-done",
                    BuildDone {
                        record: Some(record),
                        error: None,
                    },
                );
            }
            Err(e) => {
                tracing::warn!("studio: build did not run: {e}");
                let _ = worker.emit(
                    "studio-build-done",
                    BuildDone {
                        record: None,
                        error: Some(err(e)),
                    },
                );
            }
        }
    });
    if let Err(e) = spawned {
        *svc.building.lock().unwrap() = None;
        return Err(ErrorReport::internal(e));
    }
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildDone {
    record: Option<Record>,
    /// The build could not start: the kit's files could not be copied, say.
    error: Option<ErrorReport>,
}

#[tauri::command]
pub fn studio_build_cancel(app: AppHandle) {
    app.state::<StudioService>().build_cancel.store(true, Ordering::SeqCst);
}

/// Shows a build's folder in the file manager.
#[tauri::command(async)]
pub fn studio_open_build(id: String, app: AppHandle) -> Result<(), ErrorReport> {
    let svc = app.state::<StudioService>();
    let record = kit_build::find(&svc.builds_dir(), &id)
        .ok_or_else(|| ErrorReport::refused("no-build", Why::BuildNotFound, format!("no build {id}")))?;
    crate::studio::open_in_browser(&record.dir.to_string_lossy()).map_err(err)
}

/// Deletes a build's folder, images and log with it.
#[tauri::command(async)]
pub fn studio_forget_build(id: String, app: AppHandle) -> Result<StudioView, ErrorReport> {
    let svc = app.state::<StudioService>();
    if svc.building.lock().unwrap().as_ref().and_then(|b| b.id.as_deref()) == Some(id.as_str()) {
        return Err(ErrorReport::refused("busy", Why::StudioBusy, "that build is running"));
    }
    let record = kit_build::find(&svc.builds_dir(), &id)
        .ok_or_else(|| ErrorReport::refused("no-build", Why::BuildNotFound, format!("no build {id}")))?;
    std::fs::remove_dir_all(&record.dir).map_err(|e| err(e.into()))?;
    Ok(svc.view())
}

fn now_secs() -> i64 {
    now_ms() / 1000
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_key_line() {
        let key = "A".repeat(48);
        let cfg = format!("#@ Key\n_KAS_=\"{key}\"\n_OTHER_=\"x\"\n");
        assert_eq!(upg_key_line(&cfg), Some(format!("_KAS_=\"{key}\"")));
        assert_eq!(upg_key_line("_KAS_=\"short\"\n"), None);
        assert_eq!(
            upg_key_line(&format!("_KAS_='{key}'")),
            Some(format!("_KAS_=\"{key}\""))
        );
        assert_eq!(upg_key_line("nothing"), None);
    }
}
