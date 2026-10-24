//! One run of a kit's entry point: a folder of its own under the Studio's builds, the full
//! log, and a record of what went in, which options, how each step went and what came out.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::catalog::{Kit, SetKind};
use crate::flash::{InstallSet, StorageSet};
use crate::process::{self, Chunk};
use crate::{Error, Result, Why};

/// No build of ours takes this long; one that does is stuck.
const LIMIT: Duration = Duration::from_secs(2 * 3600);
/// Lines kept for the record when a build fails.
const TAIL: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Variant {
    /// adb asks per computer, USB is MTP, root only when asked for.
    Release,
    /// Root shell over USB during boot and adb open to this computer: for the bench.
    Debug,
    /// Sony's firmware as the owner's UPG has it, from the kit's restore entry: going back.
    Stock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub variant: Variant,
    /// Keep TrebleDroid's `su` in a release build.
    pub root: bool,
}

/// A vault file a build took, by the kit input it filled.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsedFile {
    pub input: String,
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Outcome {
    Running,
    Done,
    Failed {
        /// The step that was running, as the kit names it.
        step: Option<String>,
        /// The last lines of the log.
        tail: Vec<String>,
        code: Option<i32>,
    },
    Cancelled,
    /// The app ended while it ran.
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepTime {
    pub name: String,
    pub secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub kit: String,
    pub commit: String,
    /// Seconds since the epoch.
    pub started: i64,
    pub ended: Option<i64>,
    pub options: Options,
    pub inputs: Vec<UsedFile>,
    pub outcome: Outcome,
    pub total: usize,
    pub steps: Vec<StepTime>,
    /// The install set's description, once the kit wrote it.
    pub set: Option<InstallSet>,
    /// A storage kit's set instead (`SetKind::Storage`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<StorageSet>,
    /// The folder the build's files are in.
    pub dir: PathBuf,
}

impl Record {
    /// Where the install set's files are.
    pub fn set_dir(&self) -> PathBuf {
        self.dir.join("set")
    }
}

pub enum Event<'a> {
    Started(&'a Record),
    Step { index: usize, total: usize, name: &'a str },
    Line(&'a str),
}

pub struct Request<'a> {
    pub kit: &'a Kit,
    /// The copy of the kit's repository at its commit.
    pub kit_dir: &'a Path,
    /// The entry point's arguments as the kit's `args` names them; the folder for the install
    /// set comes after them.
    pub args: Vec<PathBuf>,
    pub inputs: Vec<UsedFile>,
    pub builds_dir: &'a Path,
    pub options: Options,
    pub ndk: Option<&'a Path>,
    pub sdk: Option<&'a Path>,
    /// This computer's adb public key, which a debug build lets in without asking.
    pub adb_key: Option<&'a Path>,
    /// The variable that tells the kit the owner agreed to its licence.
    pub licence_env: Option<&'a str>,
    /// Where the kit keeps its pinned downloads between builds, for a kit that does not take
    /// them through the `tools` link.
    pub cache: Option<&'a Path>,
}

/// Runs the build to its end and returns its record, which is also in `<dir>/build.json`.
pub fn run(req: Request, cancel: &AtomicBool, on_event: &mut dyn FnMut(Event)) -> Result<Record> {
    let (source, entry) = req
        .kit
        .source
        .as_ref()
        .and_then(|s| Some((s, s.steps(req.options.variant)?.0)))
        .ok_or_else(|| Error::unsupported(Why::KitHasNoSteps, format!("{} has no build steps", req.kit.id)))?;
    if req.options.variant == Variant::Debug && req.adb_key.is_none_or(|k| !k.is_file()) {
        return Err(Error::unsupported(
            Why::NoAdbKey,
            "a debug build needs this computer's adb public key",
        ));
    }
    let started = now();
    let id = build_id(started, req.builds_dir);
    let dir = req.builds_dir.join(&id);
    std::fs::create_dir_all(&dir)?;
    let mut record = Record {
        id,
        kit: req.kit.id.clone(),
        commit: source.commit.clone(),
        started,
        ended: None,
        options: req.options,
        inputs: req.inputs.clone(),
        outcome: Outcome::Running,
        total: 0,
        steps: Vec::new(),
        set: None,
        storage: None,
        dir: dir.clone(),
    };
    save(&record)?;
    on_event(Event::Started(&record));

    let mut log = File::create(dir.join("build.log"))?;
    let mut cmd = Command::new("bash");
    cmd.arg(req.kit_dir.join(entry))
        .args(&req.args)
        .arg(record.set_dir())
        .current_dir(req.kit_dir)
        .env("LC_ALL", "C.UTF-8")
        .env_remove("RELEASE")
        .env_remove("KEEP_SU");
    if req.options.variant == Variant::Release {
        cmd.env("RELEASE", "1");
        if req.options.root {
            cmd.env("KEEP_SU", "1");
        }
    }
    if let Some(k) = req.adb_key {
        cmd.env("ADB_KEY", k);
    }
    if let Some(n) = req.ndk {
        cmd.env("NDK", n);
    }
    if let Some(s) = req.sdk {
        cmd.env("ANDROID_SDK", s);
    }
    if let Some(name) = req.licence_env {
        cmd.env(name, "accepted");
    }
    if let Some(c) = req.cache {
        cmd.env("KIT_CACHE", c);
    }

    let mut tail: VecDeque<String> = VecDeque::with_capacity(TAIL);
    let mut current: Option<(String, Instant)> = None;
    let ran = process::stream("bash", &mut cmd, LIMIT, cancel, &mut |c: Chunk| {
        if c.partial {
            return;
        }
        let _ = writeln!(log, "{}", c.text);
        if tail.len() == TAIL {
            tail.pop_front();
        }
        tail.push_back(c.text.to_owned());
        match parse_line(c.text) {
            Some(Progress::Total(n)) => record.total = n,
            Some(Progress::Step(index, name)) => {
                if let Some((done, at)) = current.take() {
                    record.steps.push(StepTime {
                        name: done,
                        secs: at.elapsed().as_secs_f64(),
                    });
                }
                current = Some((name.to_owned(), Instant::now()));
                on_event(Event::Step {
                    index,
                    total: record.total,
                    name,
                });
            }
            None => on_event(Event::Line(c.text)),
        }
    });
    let _ = log.flush();
    let running = current.map(|(name, at)| (name, at.elapsed().as_secs_f64()));
    record.ended = Some(now());
    record.outcome = match ran {
        Ok(Some(0)) if req.kit.set == SetKind::Storage => match read_storage(&record.set_dir()) {
            Ok(set) => {
                if let Some((name, secs)) = running {
                    record.steps.push(StepTime { name, secs });
                }
                record.storage = Some(set);
                Outcome::Done
            }
            Err(e) => Outcome::Failed {
                step: None,
                tail: vec![e.to_string()],
                code: Some(0),
            },
        },
        Ok(Some(0)) => match read_set(&record.set_dir()) {
            Ok(set) if set.variant != req.options.variant => Outcome::Failed {
                step: None,
                tail: vec![format!(
                    "install.json: a {:?} set from a {:?} build",
                    set.variant, req.options.variant
                )],
                code: Some(0),
            },
            Ok(set) => {
                if let Some((name, secs)) = running {
                    record.steps.push(StepTime { name, secs });
                }
                record.set = Some(set);
                Outcome::Done
            }
            Err(e) => Outcome::Failed {
                step: None,
                tail: vec![e.to_string()],
                code: Some(0),
            },
        },
        Ok(code) => Outcome::Failed {
            step: running.map(|(n, _)| n),
            tail: tail.into_iter().collect(),
            code,
        },
        Err(Error::Cancelled) => Outcome::Cancelled,
        Err(e) => Outcome::Failed {
            step: running.map(|(n, _)| n),
            tail: tail.into_iter().chain([e.to_string()]).collect(),
            code: None,
        },
    };
    if !matches!(record.outcome, Outcome::Done) {
        // A half-made set is several gigabytes and installs nothing.
        let _ = std::fs::remove_dir_all(record.set_dir());
    }
    save(&record)?;
    Ok(record)
}

/// Every build under `builds_dir`, newest first. A build the app was killed during reads as
/// interrupted.
pub fn list(builds_dir: &Path) -> Vec<Record> {
    let mut out: Vec<Record> = std::fs::read_dir(builds_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let bytes = std::fs::read(e.path().join("build.json")).ok()?;
            serde_json::from_slice::<Record>(&bytes).ok()
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.started));
    out
}

/// Marks builds that were running when the app last ended.
pub fn settle_interrupted(builds_dir: &Path) {
    for mut r in list(builds_dir) {
        if matches!(r.outcome, Outcome::Running) {
            r.outcome = Outcome::Interrupted;
            let _ = std::fs::remove_dir_all(r.set_dir());
            let _ = save(&r);
        }
    }
}

pub fn find(builds_dir: &Path, id: &str) -> Option<Record> {
    if id.is_empty() || id.contains(['/', '\\', '.']) {
        return None;
    }
    let bytes = std::fs::read(builds_dir.join(id).join("build.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn read_set(dir: &Path) -> Result<InstallSet> {
    let bytes = std::fs::read(dir.join("install.json"))?;
    let set: InstallSet = serde_json::from_slice(&bytes)
        .map_err(|e| Error::unsupported(Why::BadInstallSet, format!("install.json: {e}")).with_detail(e.to_string()))?;
    set.validate()?;
    Ok(set)
}

fn read_storage(dir: &Path) -> Result<StorageSet> {
    let bytes = std::fs::read(dir.join("install.json"))?;
    let set: StorageSet = serde_json::from_slice(&bytes)
        .map_err(|e| Error::unsupported(Why::BadInstallSet, format!("install.json: {e}")).with_detail(e.to_string()))?;
    set.validate()?;
    Ok(set)
}

fn save(r: &Record) -> Result<()> {
    let file = r.dir.join("build.json");
    let tmp = r.dir.join("build.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(r).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, &file)?;
    Ok(())
}

enum Progress<'a> {
    Total(usize),
    Step(usize, &'a str),
}

/// The kit's progress lines: `== total <n>`, then `== <i> <step>` as each step starts.
fn parse_line(line: &str) -> Option<Progress<'_>> {
    let rest = line.strip_prefix("== ")?;
    if let Some(n) = rest.strip_prefix("total ") {
        return n.trim().parse().ok().map(Progress::Total);
    }
    let (index, name) = rest.split_once(' ')?;
    let index = index.parse().ok()?;
    let name = name.trim();
    (!name.is_empty() && !name.contains(' ')).then_some(Progress::Step(index, name))
}

/// `2026-10-20-193012`, local time, with `-2` and on when two start in the same second.
fn build_id(at: i64, builds_dir: &Path) -> String {
    let base = crate::library::retag::local_stamp(at).replace(' ', "-");
    let mut id = base.clone();
    let mut n = 2;
    while builds_dir.join(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_kit_progress_lines() {
        assert!(matches!(parse_line("== total 26"), Some(Progress::Total(26))));
        assert!(matches!(
            parse_line("== 4 prepare-gsi"),
            Some(Progress::Step(4, "prepare-gsi"))
        ));
        assert!(parse_line("== 4 two words").is_none());
        assert!(parse_line("==4 x").is_none());
        assert!(parse_line("tools ready").is_none());
    }

    #[test]
    fn runs_an_entry_point_and_keeps_its_record() {
        let root = std::env::temp_dir().join(format!("uwm-build-{}", std::process::id()));
        let kit_dir = root.join("kit");
        std::fs::create_dir_all(kit_dir.join("scripts")).unwrap();
        std::fs::write(
            kit_dir.join("scripts/make.sh"),
            "set -e\nout=\"$2\"\necho '== total 2'\necho '== 1 first'\necho \"release=${RELEASE:-} su=${KEEP_SU:-}\"\n\
             echo '== 2 second'\nmkdir -p \"$out\"\n\
             printf '%s' '{\"kit\":\"k\",\"commit\":null,\"dirty\":false,\"variant\":\"release\",\"root\":true,\"model\":\"NW-ZX500\",\"firmware\":\"4.06.00\",\"series\":\"0000\",\"slot\":\"b\",\"files\":{},\"flash\":[],\"after\":{\"kernel\":\"Linux version x\",\"check\":[],\"absent\":[]}}' > \"$out/install.json\"\n",
        )
        .unwrap();
        let kit: Kit = toml::from_str(
            "id = \"k\"\nprofile = \"nw-zx500\"\n[source]\nrepository = \"r\"\ncommit = \"0000000000000000000000000000000000000000\"\nentry = \"scripts/make.sh\"\nargs = [\"input:a\"]\n[[input]]\nid = \"a\"\naccepts = []\n",
        )
        .unwrap();
        let p = root.join("in");
        std::fs::write(&p, "x").unwrap();
        let builds = root.join("builds");
        let mut steps = Vec::new();
        let mut lines = Vec::new();
        let record = run(
            Request {
                kit: &kit,
                kit_dir: &kit_dir,
                args: vec![p.clone()],
                inputs: Vec::new(),
                builds_dir: &builds,
                options: Options {
                    variant: Variant::Release,
                    root: true,
                },
                ndk: None,
                sdk: None,
                adb_key: None,
                licence_env: None,
                cache: None,
            },
            &AtomicBool::new(false),
            &mut |e| match e {
                Event::Step { index, total, name } => steps.push((index, total, name.to_owned())),
                Event::Line(l) => lines.push(l.to_owned()),
                Event::Started(_) => {}
            },
        )
        .unwrap();
        assert!(matches!(record.outcome, Outcome::Done), "{:?}", record.outcome);
        assert_eq!(steps, [(1, 2, "first".to_owned()), (2, 2, "second".to_owned())]);
        assert_eq!(lines, ["release=1 su=1"]);
        assert_eq!(record.steps.len(), 2);
        assert!(record.set.as_ref().is_some_and(|s| s.root));
        let listed = list(&builds);
        assert_eq!(listed.len(), 1);
        assert_eq!(find(&builds, &record.id).unwrap().id, record.id);
        assert!(find(&builds, "../x").is_none());

        std::fs::write(
            kit_dir.join("scripts/make.sh"),
            "echo '== total 1'\necho '== 1 only'\necho boom >&2\nexit 3\n",
        )
        .unwrap();
        let failed = run(
            Request {
                kit: &kit,
                kit_dir: &kit_dir,
                args: vec![p.clone()],
                inputs: Vec::new(),
                builds_dir: &builds,
                options: Options {
                    variant: Variant::Release,
                    root: false,
                },
                ndk: None,
                sdk: None,
                adb_key: None,
                licence_env: None,
                cache: None,
            },
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        match &failed.outcome {
            Outcome::Failed { step, tail, code } => {
                assert_eq!(step.as_deref(), Some("only"));
                assert_eq!(*code, Some(3));
                assert!(tail.iter().any(|l| l == "boom"), "{tail:?}");
            }
            other => panic!("{other:?}"),
        }
        assert!(!failed.set_dir().exists());
    }

    #[test]
    fn a_stock_set_comes_from_the_restore_entry() {
        let root = std::env::temp_dir().join(format!("uwm-restore-{}", std::process::id()));
        let kit_dir = root.join("kit");
        std::fs::create_dir_all(kit_dir.join("scripts")).unwrap();
        let set = |variant: &str| {
            format!(
                "mkdir -p \"$2\"\nprintf '%s' '{{\"kit\":\"k\",\"commit\":null,\"dirty\":false,\"variant\":\"{variant}\",\"root\":false,\"model\":\"NW-ZX500\",\"firmware\":\"4.06.00\",\"series\":\"0000\",\"slot\":\"b\",\"files\":{{}},\"flash\":[],\"after\":{{\"kernel\":\"Linux version x\",\"check\":[],\"absent\":[]}}}}' > \"$2/install.json\"\n"
            )
        };
        std::fs::write(kit_dir.join("scripts/make.sh"), "exit 9\n").unwrap();
        std::fs::write(
            kit_dir.join("scripts/back.sh"),
            format!("echo \"release=${{RELEASE:-}}\"\n{}", set("stock")),
        )
        .unwrap();
        let toml = |restore: &str| {
            format!(
                "id = \"k\"\nprofile = \"nw-zx500\"\n[source]\nrepository = \"r\"\ncommit = \"0000000000000000000000000000000000000000\"\nentry = \"scripts/make.sh\"\nargs = [\"input:a\", \"input:b\"]\n{restore}[[input]]\nid = \"a\"\naccepts = []\n[[input]]\nid = \"b\"\naccepts = []\n"
            )
        };
        let kit: Kit = toml::from_str(&toml(
            "restore = { entry = \"scripts/back.sh\", args = [\"input:a\"] }\n",
        ))
        .unwrap();
        let src = kit.source.as_ref().unwrap();
        assert_eq!(src.inputs(Variant::Stock), ["a"]);
        assert_eq!(src.inputs(Variant::Release), ["a", "b"]);
        let p = root.join("in");
        std::fs::write(&p, "x").unwrap();
        let builds = root.join("builds");
        let stock = |kit: &Kit| {
            let mut lines = Vec::new();
            let r = run(
                Request {
                    kit,
                    kit_dir: &kit_dir,
                    args: vec![p.clone()],
                    inputs: Vec::new(),
                    builds_dir: &builds,
                    options: Options {
                        variant: Variant::Stock,
                        root: false,
                    },
                    ndk: None,
                    sdk: None,
                    adb_key: None,
                    licence_env: None,
                    cache: None,
                },
                &AtomicBool::new(false),
                &mut |e| {
                    if let Event::Line(l) = e {
                        lines.push(l.to_owned());
                    }
                },
            );
            (r, lines)
        };
        let (record, lines) = stock(&kit);
        let record = record.unwrap();
        assert!(matches!(record.outcome, Outcome::Done), "{:?}", record.outcome);
        assert_eq!(lines, ["release="]);
        assert_eq!(record.set.unwrap().variant, Variant::Stock);

        std::fs::write(kit_dir.join("scripts/back.sh"), set("release")).unwrap();
        let (record, _) = stock(&kit);
        assert!(
            matches!(record.unwrap().outcome, Outcome::Failed { .. }),
            "a release set is not stock"
        );

        let without: Kit = toml::from_str(&toml("")).unwrap();
        assert!(matches!(
            stock(&without).0,
            Err(Error::Unsupported {
                why: Why::KitHasNoSteps,
                ..
            })
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
