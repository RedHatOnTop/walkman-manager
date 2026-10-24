use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use uwm_core::adb::Adb;
use uwm_core::library::analyse::Engine;
use uwm_core::library::art::ArtCache;
use uwm_core::library::plan::{self, Target};
use uwm_core::library::sensme::{self, PlayerFile, Table};
use uwm_core::library::{Index, player, remote, scan, tags};
use uwm_core::probe::{self, Platform, Snapshot, VolumeKind};
use uwm_core::profile;
use uwm_core::registry::Registry;
use uwm_core::studio::{Vault, identify};

const USAGE: &str = "usage:
  uwm devices                      list connected players as JSON
  uwm snapshot <id>                read one player's state as JSON
  uwm parse-probe <file>           parse a saved probe transcript as JSON
  uwm scan <dir>...                index music folders as JSON
  uwm music <id>                   list the music on each of a player's volumes
  uwm plan <id> <internal|card> <dir>...
                                   dry run: what sending those folders would do
  uwm sensme [--table <file>] <file>...
                                   SensMe analysis of music files or raw SMFMF blocks;
                                   with the firmware's table, also their channels
  uwm player-sensme <id> [--table <file>] [<path prefix>]
                                   the same for the music on an Android player
  uwm sensme-copy <from> <to>      put the analysis in one file (or a raw SMFMF block)
                                   into an MP3 or FLAC, replacing any it had
  uwm sensme-analyse [--engine <MMLib11.dll>] [--table <file>] [--write] <file>...
                                   analyse music with Music Center for PC's own engine;
                                   with --write, put the analysis into each MP3 or FLAC
  uwm tag <file> <field>=<value>...
                                   write tags (title, artist, album, albumArtist, genre,
                                   year, track, trackTotal, disc, discTotal)
  uwm studio identify <file>...    what firmware or ROM files are, by structure and hash
  uwm studio add <file>...         the same, kept in the Studio's vault
  uwm studio kits                  each build kit's inputs and the vault files that fill them";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["devices"] => devices(),
        ["snapshot", id] => snapshot(id),
        ["parse-probe", file] => parse_probe(file),
        ["scan", dirs @ ..] if !dirs.is_empty() => scan_json(dirs),
        ["music", id] => music(id),
        ["plan", id, volume, dirs @ ..] if !dirs.is_empty() => plan_dirs(id, volume, dirs),
        ["sensme", "--table", table, files @ ..] if !files.is_empty() => sensme_files(Some(table), files),
        ["sensme", files @ ..] if !files.is_empty() => sensme_files(None, files),
        ["player-sensme", id, "--table", table, prefix @ ..] if prefix.len() <= 1 => {
            player_sensme(id, Some(table), prefix.first().copied())
        }
        ["player-sensme", id, prefix @ ..] if prefix.len() <= 1 => player_sensme(id, None, prefix.first().copied()),
        ["sensme-copy", from, to] => sensme_copy(from, to),
        ["sensme-analyse", rest @ ..] => sensme_analyse(rest),
        ["tag", file, fields @ ..] if !fields.is_empty() => tag(file, fields),
        ["studio", "identify", files @ ..] if !files.is_empty() => studio_identify(files, false),
        ["studio", "add", files @ ..] if !files.is_empty() => studio_identify(files, true),
        ["studio", "kits"] => studio_kits(),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("uwm: {e}");
            ExitCode::FAILURE
        }
    }
}

fn registry() -> Registry {
    Registry::new(Adb::locate(None).ok(), Vec::new())
}

fn devices() -> Res<String> {
    Ok(serde_json::to_string_pretty(&registry().scan())?)
}

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn read(id: &str) -> Res<(Adb, String, Snapshot)> {
    let mut reg = registry();
    let scan = reg.scan();
    let device = scan
        .devices
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| uwm_core::Error::NoSuchDevice(id.to_owned()))?;
    let (adb, target) = reg.probe_target(device)?;
    let snap = probe::run(&adb, &target)?;
    Ok((adb, target, snap))
}

fn snapshot(id: &str) -> Res<String> {
    Ok(serde_json::to_string_pretty(&read(id)?.2)?)
}

fn scan_dirs(dirs: &[&str]) -> Res<Index> {
    let roots: Vec<PathBuf> = dirs.iter().map(PathBuf::from).collect();
    let art = ArtCache::new(std::env::temp_dir().join("uwm-cli-art"));
    let cancel = AtomicBool::new(false);
    Ok(scan::scan(&roots, None, &art, &cancel, &|p| {
        eprint!("\r{} of {}", p.done, p.found);
    }))
}

fn scan_json(dirs: &[&str]) -> Res<String> {
    let index = scan_dirs(dirs)?;
    eprintln!();
    Ok(serde_json::to_string_pretty(&index)?)
}

fn table(path: Option<&str>) -> Res<Option<Table>> {
    Ok(path.map(|p| Table::load(Path::new(p))).transpose()?)
}

/// The SMFMF block in a music file, or the file itself when it is a raw block.
fn smfmf_of(path: &Path) -> Option<Vec<u8>> {
    sensme::extract(path).or_else(|| {
        std::fs::read(path)
            .ok()
            .filter(|d| matches!(d.get(4..8), Some(b"STAE" | b"SBZT")))
    })
}

fn sensme_files(table_path: Option<&str>, files: &[&str]) -> Res<String> {
    let table = table(table_path)?;
    let out: Vec<_> = files
        .iter()
        .map(|f| {
            let mut found = smfmf_of(Path::new(f)).and_then(|b| sensme::decode(&b));
            sensme::classify(found.iter_mut(), table.as_ref());
            serde_json::json!({ "file": f, "sensme": found })
        })
        .collect();
    Ok(serde_json::to_string_pretty(&out)?)
}

fn sensme_copy(from: &str, to: &str) -> Res<String> {
    let blob = smfmf_of(Path::new(from)).ok_or_else(|| format!("{from} holds no SensMe analysis"))?;
    tags::write_smfmf(Path::new(to), &blob)?;
    let found = sensme::read(Path::new(to));
    Ok(serde_json::to_string_pretty(
        &serde_json::json!({ "file": to, "sensme": found }),
    )?)
}

fn sensme_analyse(args: &[&str]) -> Res<String> {
    let (mut engine, mut table_path, mut write, mut files) = (None, None, false, Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--engine" => engine = Some(Path::new(*it.next().ok_or("--engine needs a path")?)),
            "--table" => table_path = Some(*it.next().ok_or("--table needs a path")?),
            "--write" => write = true,
            f => files.push(f),
        }
    }
    if files.is_empty() {
        return Err(USAGE.into());
    }
    let table = table(table_path)?;
    let work = dirs::data_dir()
        .ok_or("no data folder for the analysis helper")?
        .join("dev.personnya.walkman-manager")
        .join("analysis");
    let engine = Engine::locate(engine, &work)?;
    eprintln!("engine {}", serde_json::to_string(&engine.info())?);
    let mut session = engine.start()?;
    let cancel = AtomicBool::new(false);
    let mut out = Vec::new();
    for (n, f) in files.iter().enumerate() {
        eprint!("\r{} of {}", n + 1, files.len());
        let started = std::time::Instant::now();
        let result = session.analyse(Path::new(f), &cancel).and_then(|blob| {
            if write {
                tags::write_smfmf(Path::new(f), &blob)?;
            }
            Ok(blob)
        });
        let secs = started.elapsed().as_secs_f64();
        out.push(match result {
            Ok(blob) => {
                let mut found = sensme::decode(&blob);
                sensme::classify(found.iter_mut(), table.as_ref());
                serde_json::json!({ "file": f, "sensme": found, "written": write, "secs": secs })
            }
            Err(e) => {
                if !session.alive() {
                    session = engine.start()?;
                }
                serde_json::json!({ "file": f, "error": e.to_string(), "secs": secs })
            }
        });
    }
    eprintln!();
    Ok(serde_json::to_string_pretty(&out)?)
}

fn vault() -> Vault {
    let data = dirs::data_dir().unwrap_or_else(std::env::temp_dir);
    Vault::open(
        data.join("dev.personnya.walkman-manager")
            .join("studio")
            .join("vault.json"),
    )
}

fn studio_kits() -> Res<String> {
    Ok(serde_json::to_string_pretty(&vault().kits())?)
}

fn studio_identify(files: &[&str], keep: bool) -> Res<String> {
    let mut vault = keep.then(vault);
    let cancel = AtomicBool::new(false);
    let mut out = Vec::new();
    for f in files {
        let found = identify::identify(Path::new(f), &cancel, &mut |done, total| {
            eprint!("\r{f}: {} of {} MB", done >> 20, total >> 20);
        })?;
        eprintln!();
        match vault.as_mut() {
            Some(v) => out.push(serde_json::to_value(v.add(found)?)?),
            None => out.push(serde_json::to_value(found)?),
        }
    }
    Ok(serde_json::to_string_pretty(&out)?)
}

fn tag(file: &str, fields: &[&str]) -> Res<String> {
    let mut edit = serde_json::Map::new();
    for f in fields {
        let (key, value) = f
            .split_once('=')
            .ok_or_else(|| format!("{f}: expected <field>=<value>"))?;
        let value = match value.parse::<u32>() {
            Ok(n) if matches!(key, "year" | "track" | "trackTotal" | "disc" | "discTotal") => n.into(),
            _ => value.into(),
        };
        edit.insert(key.to_owned(), value);
    }
    let edit: tags::TagEdit = serde_json::from_value(edit.into())?;
    tags::write(Path::new(file), &edit, None)?;
    Ok(serde_json::to_string_pretty(
        &serde_json::json!({ "file": file, "written": edit }),
    )?)
}

fn player_sensme(id: &str, table_path: Option<&str>, prefix: Option<&str>) -> Res<String> {
    let table = table(table_path)?;
    let (adb, target, snap) = read(id)?;
    if snap.system.platform != Platform::Android {
        return Err("player-sensme reads Android players".into());
    }
    let library = player::read_media_store(&adb, &target)?;
    let tracks: Vec<_> = library
        .index
        .tracks
        .iter()
        .filter(|t| prefix.is_none_or(|p| t.path.starts_with(p)))
        .collect();
    let files: Vec<PlayerFile> = tracks
        .iter()
        .map(|t| PlayerFile {
            path: &t.path,
            size: t.size,
        })
        .collect();
    let cancel = AtomicBool::new(false);
    let started = std::time::Instant::now();
    let mut found = sensme::read_player(&adb, &target, &files, false, &cancel, &mut |n| {
        eprint!("\r{n} of {}", files.len());
    })?;
    eprintln!(" in {:.1} s", started.elapsed().as_secs_f64());
    sensme::classify(found.iter_mut().flatten(), table.as_ref());
    let out: Vec<_> = tracks
        .iter()
        .zip(found)
        .filter_map(|(t, s)| s.map(|s| serde_json::json!({ "file": t.path, "sensme": s })))
        .collect();
    Ok(serde_json::to_string_pretty(&out)?)
}

fn music(id: &str) -> Res<String> {
    let (adb, target, snap) = read(id)?;
    let mut out = Vec::new();
    for v in &snap.volumes {
        let root = remote::music_root(snap.system.platform, v);
        out.push(remote::list(&adb, &target, &root)?);
    }
    Ok(serde_json::to_string_pretty(&out)?)
}

fn plan_dirs(id: &str, volume: &str, dirs: &[&str]) -> Res<String> {
    let kind = match volume {
        "internal" => VolumeKind::Internal,
        "card" => VolumeKind::Card,
        other => return Err(format!("unknown volume {other}; use internal or card").into()),
    };
    let index = scan_dirs(dirs)?;
    eprintln!();
    let (adb, target, snap) = read(id)?;
    let profile = snap
        .identity
        .profile_id
        .as_deref()
        .and_then(profile::get)
        .ok_or("not a known player")?;
    let vol = snap.volumes.iter().find(|v| v.kind == kind).ok_or("no such volume")?;
    let root = remote::music_root(snap.system.platform, vol);
    let listing = remote::list(&adb, &target, &root)?;
    let ids: Vec<String> = index.tracks.iter().map(|t| t.id.clone()).collect();
    let plan = plan::build(
        &index,
        &ids,
        &listing,
        &profile.playback,
        Target {
            device_id: id.to_owned(),
            adb_target: target,
            platform: snap.system.platform,
            volume: kind,
            root,
            free_bytes: vol.free_bytes,
        },
    );
    Ok(serde_json::to_string_pretty(&plan)?)
}

fn parse_probe(file: &str) -> Res<String> {
    let text = std::fs::read_to_string(file)?;
    Ok(serde_json::to_string_pretty(&probe::parse(&text))?)
}
