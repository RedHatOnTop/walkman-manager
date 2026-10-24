//! A kit's build steps: the owner's checkout of the device repository, and the copy of it at
//! the kit's pinned commit that the Studio runs. The checkout's own working tree is never run,
//! so what a build ran is always the commit the kit names.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::catalog::KitSource;
use super::fetch;
use crate::{Error, Result, Why, process};

/// What the Studio found at the folder the owner named for a kit's repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RepoState {
    /// No folder named yet.
    Unset,
    Ready {
        path: String,
    },
    Missing {
        path: String,
    },
    NotGit {
        path: String,
    },
    /// A git repository that does not hold the pinned commit.
    NoCommit {
        path: String,
    },
    /// The kit comes from its repository's published archive; `have` once it is here.
    Published {
        have: bool,
        bytes: u64,
    },
}

impl RepoState {
    pub fn is_ready(&self) -> bool {
        matches!(self, RepoState::Ready { .. } | RepoState::Published { .. })
    }
}

pub fn check(dir: Option<&Path>, source: &KitSource) -> RepoState {
    let Some(dir) = dir else {
        return RepoState::Unset;
    };
    let path = dir.to_string_lossy().into_owned();
    if !dir.is_dir() {
        return RepoState::Missing { path };
    }
    let git = |args: &[&str]| {
        process::run(
            "git",
            Command::new("git").arg("-C").arg(dir).args(args),
            Duration::from_secs(15),
        )
    };
    match git(&["rev-parse", "--git-dir"]) {
        Ok(out) if out.success() => {}
        _ => return RepoState::NotGit { path },
    }
    match git(&["cat-file", "-e", &format!("{}^{{commit}}", source.commit)]) {
        Ok(out) if out.success() => RepoState::Ready { path },
        _ => RepoState::NoCommit { path },
    }
}

fn short(source: &KitSource) -> &str {
    source.commit.get(..12).unwrap_or(&source.commit)
}

fn dest_of(kits: &Path, kit_id: &str, source: &KitSource) -> PathBuf {
    kits.join(format!("{kit_id}-{}", short(source)))
}

/// An empty `<kit>-<commit>.part` folder to unpack into.
fn fresh_part(kits: &Path, kit_id: &str, source: &KitSource, tools_dir: &Path) -> Result<PathBuf> {
    let part = kits.join(format!("{kit_id}-{}.part", short(source)));
    if part.exists() {
        std::fs::remove_dir_all(&part)?;
    }
    std::fs::create_dir_all(&part)?;
    std::fs::create_dir_all(tools_dir)?;
    Ok(part)
}

/// The kit's published archive is unpacked here already.
pub fn have(kits: &Path, kit_id: &str, source: &KitSource) -> bool {
    stamped(&dest_of(kits, kit_id, source), &source.commit)
}

/// The kit's files at its commit, under `kits`, with `tools` linked to `tools_dir` so the
/// kit's pinned downloads are fetched once for every copy.
pub fn copy(repo: &Path, kit_id: &str, source: &KitSource, kits: &Path, tools_dir: &Path) -> Result<PathBuf> {
    let dest = dest_of(kits, kit_id, source);
    if stamped(&dest, &source.commit) {
        return Ok(dest);
    }
    let part = fresh_part(kits, kit_id, source, tools_dir)?;

    let mut archive = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["archive", "--format=tar", &source.commit])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| Error::Spawn { tool: "git", source })?;
    let tar_in = archive.stdout.take().expect("piped stdout");
    let extracted = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(&part)
        .stdin(tar_in)
        .output()
        .map_err(|source| Error::Spawn { tool: "tar", source })?;
    let archived = archive.wait_with_output()?;
    if !archived.status.success() {
        return Err(Error::Failed {
            tool: "git",
            detail: String::from_utf8_lossy(&archived.stderr).trim().to_owned(),
        });
    }
    if !extracted.status.success() {
        return Err(Error::Failed {
            tool: "tar",
            detail: String::from_utf8_lossy(&extracted.stderr).trim().to_owned(),
        });
    }
    finish(&part, &dest, source, tools_dir, &repo.to_string_lossy())
}

/// The kit's files at its commit from the archive its repository published, checked against
/// the pinned sha256, under `kits` as [`copy`] leaves them.
pub fn download(
    kit_id: &str,
    source: &KitSource,
    kits: &Path,
    tools_dir: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf> {
    let Some(archive) = &source.archive else {
        return Err(Error::unsupported(
            Why::NoDirectDownload,
            format!("{kit_id} has no published archive"),
        ));
    };
    let dest = dest_of(kits, kit_id, source);
    if stamped(&dest, &source.commit) {
        return Ok(dest);
    }
    let tarball = kits.join(format!("{kit_id}-{}.tar.gz", short(source)));
    if sha256_of(&tarball).ok().as_deref() != Some(archive.sha256.as_str()) {
        fetch::download(
            &archive.url,
            Some(archive.bytes),
            Some(&archive.sha256),
            &tarball,
            cancel,
            on_progress,
        )?;
    }
    let part = fresh_part(kits, kit_id, source, tools_dir)?;
    let out = process::run(
        "tar",
        Command::new("tar").arg("-xzf").arg(&tarball).arg("-C").arg(&part),
        Duration::from_secs(120),
    )?;
    if !out.success() {
        return Err(Error::Failed {
            tool: "tar",
            detail: out.stderr.trim().to_owned(),
        });
    }
    let done = finish(&part, &dest, source, tools_dir, &archive.url)?;
    std::fs::remove_file(&tarball)?;
    Ok(done)
}

fn sha256_of(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Checks the stamp, links `tools` and moves the unpacked kit into place. A kit with a `tools`
/// folder of its own finds the downloads folder in `KIT_CACHE` instead.
fn finish(part: &Path, dest: &Path, source: &KitSource, tools_dir: &Path, origin: &str) -> Result<PathBuf> {
    if !stamped(part, &source.commit) {
        return Err(Error::unsupported(
            Why::KitNotStamped,
            format!("{origin}: kit/COMMIT does not name {}", source.commit),
        )
        .with_detail(origin.to_owned()));
    }
    #[cfg(unix)]
    if !part.join("tools").exists() {
        std::os::unix::fs::symlink(tools_dir, part.join("tools"))?;
    }
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::rename(part, dest)?;
    Ok(dest.to_path_buf())
}

/// `git archive` wrote the commit into kit/COMMIT (the repository's `export-subst`).
fn stamped(dir: &Path, commit: &str) -> bool {
    std::fs::read_to_string(dir.join("kit/COMMIT")).is_ok_and(|s| s.trim() == commit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::catalog::KitArchive;

    fn sh(dir: &Path, script: &str) {
        let ok = Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(ok.success(), "{script}");
    }

    #[test]
    fn copies_the_pinned_commit_not_the_working_tree() {
        let root = std::env::temp_dir().join(format!("uwm-kit-{}", std::process::id()));
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join("kit")).unwrap();
        sh(
            &repo,
            "git init -q && git config user.email t@t && git config user.name t \
             && printf '%s\\n' '$Format:%H$' > kit/COMMIT && echo 'kit/COMMIT export-subst' > .gitattributes \
             && echo one > step.sh && git add -A && git commit -q -m one",
        );
        let commit = String::from_utf8(
            Command::new("git")
                .args(["-C"])
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        sh(&repo, "echo two > step.sh");
        let source = KitSource {
            repository: "repo".into(),
            commit: commit.clone(),
            entry: "step.sh".into(),
            args: Vec::new(),
            archive: None,
            restore: None,
            variants: true,
        };
        assert_eq!(
            check(Some(&repo), &source),
            RepoState::Ready {
                path: repo.to_string_lossy().into_owned()
            }
        );
        let wrong = KitSource {
            commit: "0".repeat(40),
            ..source.clone()
        };
        assert!(matches!(check(Some(&repo), &wrong), RepoState::NoCommit { .. }));
        assert!(matches!(check(Some(&root), &source), RepoState::NotGit { .. }));
        assert_eq!(check(None, &source), RepoState::Unset);

        let kits = root.join("kits");
        let copy_dir = copy(&repo, "k", &source, &kits, &root.join("tools")).unwrap();
        assert_eq!(std::fs::read_to_string(copy_dir.join("step.sh")).unwrap(), "one\n");
        assert!(copy_dir.join("tools").exists());
        assert_eq!(copy(&repo, "k", &source, &kits, &root.join("tools")).unwrap(), copy_dir);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unpacks_the_published_archive_it_pins() {
        let root = std::env::temp_dir().join(format!("uwm-kit-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let commit = "c".repeat(40);
        let src = root.join("src");
        std::fs::create_dir_all(src.join("kit")).unwrap();
        std::fs::write(src.join("kit/COMMIT"), format!("{commit}\n")).unwrap();
        std::fs::write(src.join("step.sh"), "one\n").unwrap();
        let kits = root.join("kits");
        std::fs::create_dir_all(&kits).unwrap();
        let tarball = kits.join("k-cccccccccccc.tar.gz");
        sh(&root, &format!("tar -czf {} -C src .", tarball.display()));
        let source = KitSource {
            repository: "repo".into(),
            commit,
            entry: "step.sh".into(),
            args: Vec::new(),
            archive: Some(KitArchive {
                // Never fetched: the tarball already there has the pinned sha256.
                url: "http://127.0.0.1:9/k.tar.gz".into(),
                bytes: std::fs::metadata(&tarball).unwrap().len(),
                sha256: sha256_of(&tarball).unwrap(),
            }),
            restore: None,
            variants: true,
        };
        assert!(!have(&kits, "k", &source));
        let cancel = AtomicBool::new(false);
        let dir = download("k", &source, &kits, &root.join("tools"), &cancel, &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("step.sh")).unwrap(), "one\n");
        assert!(dir.join("tools").exists());
        assert!(!tarball.exists());
        assert!(have(&kits, "k", &source));
        assert_eq!(
            download("k", &source, &kits, &root.join("tools"), &cancel, &mut |_, _| {}).unwrap(),
            dir
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
