//! The fastboot client binary, run only inside a flow the owner started. fastboot reports on
//! stderr: a step ("Sending sparse 'system_b' 2/8 (393212 KB)"), then on the same line its
//! result ("OKAY [ 10.112s]" or "FAILED (remote: '...')").

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use crate::process::{self, Chunk};
use crate::{Error, Result, Why};

#[derive(Debug, Clone)]
pub struct Fastboot {
    bin: PathBuf,
}

impl Fastboot {
    /// The configured binary, else the one beside adb (platform-tools ship both), else `PATH`.
    pub fn locate(configured: Option<&Path>, adb: Option<&Path>) -> Result<Self> {
        if let Some(p) = configured {
            return if p.is_file() {
                Ok(Fastboot { bin: p.to_path_buf() })
            } else {
                Err(Error::ToolMissing { tool: "fastboot" })
            };
        }
        let beside = adb
            .and_then(Path::parent)
            .map(|d| d.join(if cfg!(windows) { "fastboot.exe" } else { "fastboot" }))
            .filter(|p| p.is_file());
        let bin = match beside {
            Some(p) => p,
            None => which::which("fastboot").map_err(|_| Error::ToolMissing { tool: "fastboot" })?,
        };
        Ok(Fastboot { bin })
    }

    pub fn path(&self) -> &Path {
        &self.bin
    }

    /// Whether `-w` can make the userdata image on this computer. fastboot makes it with the
    /// mke2fs beside its own binary, links followed (Debian's `/usr/bin/fastboot` is a link
    /// into `/usr/lib/android-sdk/platform-tools`), `mke2fs.android` in Fedora's package;
    /// e2fsprogs' mke2fs has no Android sparse output. This makes a small image in `scratch`
    /// with the arguments fastboot passes and looks for the sparse header.
    pub fn formats(&self, scratch: &Path) -> bool {
        let Some(mke2fs) = formatter(&self.bin) else {
            return false;
        };
        if std::fs::create_dir_all(scratch).is_err() {
            return false;
        }
        let image = scratch.join(format!("formats-{}.img", std::process::id()));
        let mut cmd = Command::new(&mke2fs);
        cmd.env("MKE2FS_CONFIG", mke2fs.with_file_name("mke2fs.conf"))
            .args(["-t", "ext4", "-b", "4096", "-E", "android_sparse", "-O", "uninit_bg"])
            .arg(&image)
            .arg("1024");
        let made = process::run("mke2fs", &mut cmd, Duration::from_secs(20)).is_ok_and(|o| o.success());
        let sparse = made && std::fs::read(&image).is_ok_and(|b| b.starts_with(&SPARSE_MAGIC));
        let _ = std::fs::remove_file(&image);
        sparse
    }

    /// Serials of the devices in fastboot mode.
    pub fn devices(&self) -> Result<Vec<String>> {
        let out = process::run(
            "fastboot",
            Command::new(&self.bin).arg("devices"),
            Duration::from_secs(10),
        )?;
        Ok(out
            .stdout
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .filter(|s| !s.is_empty() && *s != "???????????")
            .map(str::to_owned)
            .collect())
    }

    /// One bootloader variable, or None when the bootloader does not have it.
    pub fn getvar(&self, serial: &str, var: &str) -> Result<Option<String>> {
        let out = process::run(
            "fastboot",
            Command::new(&self.bin).args(["-s", serial, "getvar", var]),
            Duration::from_secs(15),
        )?;
        Ok(parse_getvar(&out.stderr, var))
    }

    /// Runs one fastboot command to its end, handing over each piece of its report as it
    /// comes. A command fastboot reports as failed is an error carrying its last words.
    pub fn run(
        &self,
        serial: &str,
        args: &[&str],
        timeout: Duration,
        cancel: &AtomicBool,
        on_report: &mut dyn FnMut(&str, bool),
    ) -> Result<()> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(["-s", serial]).args(args);
        let mut last = Vec::<String>::new();
        let code = process::stream("fastboot", &mut cmd, timeout, cancel, &mut |c: Chunk| {
            on_report(c.text, c.partial);
            if !c.partial && !c.text.trim().is_empty() {
                last.push(c.text.trim().to_owned());
                if last.len() > 6 {
                    last.remove(0);
                }
            }
        })?;
        let failed = last
            .iter()
            .any(|l| l.contains("FAILED") || l.starts_with("fastboot: error"));
        if code == Some(0) && !failed {
            return Ok(());
        }
        Err(Error::Failed {
            tool: "fastboot",
            detail: last.join("\n"),
        })
    }
}

impl Fastboot {
    /// `fastboot flashing unlock`, then the switch read back; `budget` covers both. The
    /// bootloader answers nothing while it erases userdata (about 9.5 minutes on the ZX507,
    /// walkman-zx507's devlog of 2026-10-14), so the command may time out, or lose the player
    /// on USB, with the unlock still under way: only `getvar unlocked` decides. A bootloader
    /// that answered the command, OKAY or a refusal, has nothing left to erase, so its first
    /// reading is final.
    pub fn unlock(&self, serial: &str, budget: Duration, poll: Duration, log: &mut dyn FnMut(&str)) -> Result<()> {
        let never = AtomicBool::new(false);
        let until = Instant::now() + budget;
        log("fastboot flashing unlock");
        let ran = self.run(serial, &["flashing", "unlock"], budget, &never, &mut |_, _| {});
        let answered = match &ran {
            Ok(()) => true,
            Err(e) => {
                log(&format!("flashing unlock: {e}"));
                matches!(e, Error::Failed { detail, .. } if detail.contains("FAILED (remote"))
            }
        };
        loop {
            if let Ok(Some(v)) = self.getvar(serial, "unlocked") {
                log(&format!("unlocked: {v}"));
                if v == "yes" {
                    return Ok(());
                }
                if answered {
                    break;
                }
            }
            if Instant::now() + poll >= until {
                break;
            }
            thread::sleep(poll);
        }
        Err(Error::unsupported(
            Why::StillLocked,
            "the bootloader still reads locked",
        ))
    }
}

/// The first four bytes of an Android sparse image, 0xed26ff3a little-endian.
const SPARSE_MAGIC: [u8; 4] = [0x3a, 0xff, 0x26, 0xed];

/// The ext4 formatter the fastboot at `bin` runs.
fn formatter(bin: &Path) -> Option<PathBuf> {
    let bin = std::fs::canonicalize(bin).unwrap_or_else(|_| bin.to_path_buf());
    let dir = bin.parent()?;
    ["mke2fs.android", "mke2fs"]
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
}

pub fn parse_getvar(text: &str, var: &str) -> Option<String> {
    let prefix = format!("{var}:");
    text.lines()
        .map(|l| l.trim())
        .find_map(|l| l.strip_prefix("(bootloader)").unwrap_or(l).trim().strip_prefix(&prefix))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// One thing fastboot said about the write in progress.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    /// A piece of an image goes over USB: `chunk` of `chunks` for a sparse image.
    Sending {
        partition: String,
        chunk: u32,
        chunks: u32,
    },
    Writing {
        partition: String,
    },
    /// The step on this line is done.
    Okay,
    Failed(String),
}

/// What a line (or the start of one) of fastboot's report says, in order.
pub fn reports(line: &str) -> Vec<Report> {
    let mut out = Vec::new();
    let quoted = |s: &str| s.split('\'').nth(1).map(str::to_owned);
    let text = line.trim_start();
    if let Some(rest) = text.strip_prefix("Sending sparse ") {
        if let Some(partition) = quoted(rest) {
            let fraction = rest.split('\'').nth(2).and_then(|t| t.split_whitespace().next());
            let (chunk, chunks) = fraction
                .and_then(|f| f.split_once('/'))
                .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
                .unwrap_or((1, 1));
            out.push(Report::Sending {
                partition,
                chunk,
                chunks,
            });
        }
    } else if let Some(rest) = text.strip_prefix("Sending ") {
        if let Some(partition) = quoted(rest) {
            out.push(Report::Sending {
                partition,
                chunk: 1,
                chunks: 1,
            });
        }
    } else if let Some(rest) = text.strip_prefix("Writing ")
        && let Some(partition) = quoted(rest)
    {
        out.push(Report::Writing { partition });
    }
    if text.contains("OKAY [") {
        out.push(Report::Okay);
    }
    if let Some(at) = text.find("FAILED") {
        out.push(Report::Failed(text[at..].trim().to_owned()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fastboot that plays a bootloader: `mode` in its folder says how it takes the unlock,
    /// `calls` gets each command line.
    #[cfg(unix)]
    fn fake(name: &str, mode: &str) -> (Fastboot, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("uwm-unlock-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mode"), mode).unwrap();
        std::fs::write(dir.join("unlocked"), "no").unwrap();
        let script = r#"#!/bin/bash
d=$(dirname "$0")
echo "$*" >> "$d/calls"
mode=$(cat "$d/mode")
now=$(date +%s%N)
erase() { echo $((now + $1 * 1000000)) > "$d/until"; }
case "$3 $4" in
"flashing unlock")
	case $mode in
	answers)
		echo yes > "$d/unlocked"
		echo "OKAY [  0.120s]" >&2
		echo "Finished. Total time: 0.120s" >&2 ;;
	declined)
		echo "OKAY [  0.120s]" >&2 ;;
	refuses)
		echo "FAILED (remote: 'Flashing Unlock is not allowed')" >&2
		echo "fastboot: error: Command failed" >&2
		exit 1 ;;
	silent)
		erase 300
		exec sleep 30 ;;
	drops)
		erase 600
		echo "FAILED (Status read failed (No such device))" >&2
		echo "fastboot: error: Command failed" >&2
		exit 1 ;;
	gone)
		exec sleep 30 ;;
	esac ;;
"getvar unlocked")
	[ "$mode" = gone ] && exit 1
	if [ -f "$d/until" ]; then
		[ "$now" -lt "$(cat "$d/until")" ] && exit 1
		echo yes > "$d/unlocked"
	fi
	echo "unlocked: $(cat "$d/unlocked")" >&2
	echo "Finished. Total time: 0.001s" >&2 ;;
esac
"#;
        let bin = dir.join("fastboot");
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        (Fastboot { bin }, dir)
    }

    #[cfg(unix)]
    fn unlock_with(name: &str, mode: &str, budget_ms: u64) -> (Result<()>, Vec<String>, Duration, Vec<String>) {
        let (fb, dir) = fake(name, mode);
        let mut said = Vec::new();
        let start = Instant::now();
        let got = fb.unlock(
            "001045c6",
            Duration::from_millis(budget_ms),
            Duration::from_millis(50),
            &mut |l| said.push(l.to_owned()),
        );
        let took = start.elapsed();
        let calls = std::fs::read_to_string(dir.join("calls")).unwrap_or_default();
        std::fs::remove_dir_all(&dir).unwrap();
        (got, said, took, calls.lines().map(str::to_owned).collect())
    }

    #[cfg(unix)]
    #[test]
    fn a_bootloader_that_answers_is_read_back_once() {
        let (got, said, _, calls) = unlock_with("answers", "answers", 5000);
        assert!(got.is_ok(), "{got:?}");
        assert_eq!(calls, ["-s 001045c6 flashing unlock", "-s 001045c6 getvar unlocked"]);
        assert_eq!(said, ["fastboot flashing unlock", "unlocked: yes"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_silent_erase_outlasting_the_command_is_read_back_after_it() {
        let (got, _, _, calls) = unlock_with("silent", "silent", 1200);
        assert!(got.is_ok(), "{got:?}");
        assert_eq!(calls.first().map(String::as_str), Some("-s 001045c6 flashing unlock"));
    }

    #[cfg(unix)]
    #[test]
    fn a_player_lost_on_usb_mid_erase_is_waited_for() {
        let (got, said, took, calls) = unlock_with("drops", "drops", 5000);
        assert!(got.is_ok(), "{got:?}");
        assert!(
            took >= Duration::from_millis(600),
            "read back before the erase ended: {took:?}"
        );
        assert!(calls.len() > 2, "the switch is polled while the bootloader is silent");
        assert_eq!(said.last().map(String::as_str), Some("unlocked: yes"));
    }

    #[cfg(unix)]
    #[test]
    fn a_refusal_ends_the_stage_without_waiting() {
        let (got, said, took, calls) = unlock_with("refuses", "refuses", 20_000);
        assert!(
            matches!(
                got,
                Err(Error::Unsupported {
                    why: Why::StillLocked,
                    ..
                })
            ),
            "{got:?}"
        );
        assert!(took < Duration::from_secs(5), "{took:?}");
        assert_eq!(calls.len(), 2);
        assert!(said[1].contains("Flashing Unlock is not allowed"), "{said:?}");
        assert_eq!(said.last().map(String::as_str), Some("unlocked: no"));
    }

    #[cfg(unix)]
    #[test]
    fn an_unlock_the_player_declined_fails() {
        let (got, _, took, calls) = unlock_with("declined", "declined", 20_000);
        assert!(matches!(
            got,
            Err(Error::Unsupported {
                why: Why::StillLocked,
                ..
            })
        ));
        assert!(took < Duration::from_secs(5), "{took:?}");
        assert_eq!(calls.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_player_that_never_answers_fails_when_the_budget_runs_out() {
        let (got, _, took, _) = unlock_with("gone", "gone", 800);
        assert!(matches!(
            got,
            Err(Error::Unsupported {
                why: Why::StillLocked,
                ..
            })
        ));
        assert!(
            took >= Duration::from_millis(800) && took < Duration::from_secs(5),
            "{took:?}"
        );
    }

    #[test]
    fn a_fastboot_without_a_formatter_beside_it_cannot_format() {
        let dir = std::env::temp_dir().join(format!("uwm-fastboot-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fb = Fastboot {
            bin: dir.join("fastboot"),
        };
        assert!(!fb.formats(&dir));
        std::fs::write(dir.join("mke2fs"), b"").unwrap();
        assert_eq!(formatter(&fb.bin), Some(dir.join("mke2fs")));
        std::fs::write(dir.join("mke2fs.android"), b"").unwrap();
        assert_eq!(
            formatter(&fb.bin),
            Some(dir.join("mke2fs.android")),
            "Fedora's name first"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_fastboot_is_judged_where_the_link_leads() {
        let dir = std::env::temp_dir().join(format!("uwm-fastboot-link-test-{}", std::process::id()));
        let real = dir.join("platform-tools");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("fastboot"), b"").unwrap();
        std::fs::write(dir.join("mke2fs"), b"").unwrap();
        std::os::unix::fs::symlink(real.join("fastboot"), dir.join("fastboot")).unwrap();
        assert_eq!(
            formatter(&dir.join("fastboot")),
            None,
            "the mke2fs beside the link is not the one it runs"
        );
        std::fs::write(real.join("mke2fs"), b"").unwrap();
        assert_eq!(
            formatter(&dir.join("fastboot")).and_then(|p| p.canonicalize().ok()),
            real.join("mke2fs").canonicalize().ok()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_variables_from_stderr() {
        let text = "unlocked: yes\nFinished. Total time: 0.001s\n";
        assert_eq!(parse_getvar(text, "unlocked").as_deref(), Some("yes"));
        assert_eq!(parse_getvar("current-slot: b\n", "current-slot").as_deref(), Some("b"));
        assert_eq!(
            parse_getvar("(bootloader) unlocked: yes\nOKAY [  0.001s]\n", "unlocked").as_deref(),
            Some("yes")
        );
        assert_eq!(
            parse_getvar("getvar:nothing FAILED (remote: 'unknown')\n", "unlocked"),
            None
        );
    }

    #[test]
    fn follows_a_sparse_write() {
        assert_eq!(
            reports("Sending sparse 'system_b' 2/8 (393212 KB)"),
            [Report::Sending {
                partition: "system_b".into(),
                chunk: 2,
                chunks: 8
            }]
        );
        assert_eq!(
            reports("Sending sparse 'system_b' 2/8 (393212 KB)          OKAY [ 10.112s]"),
            [
                Report::Sending {
                    partition: "system_b".into(),
                    chunk: 2,
                    chunks: 8
                },
                Report::Okay
            ]
        );
        assert_eq!(
            reports("Writing 'vbmeta_b'                                 OKAY [  0.004s]"),
            [
                Report::Writing {
                    partition: "vbmeta_b".into()
                },
                Report::Okay
            ]
        );
        assert_eq!(
            reports("Sending 'boot_b' (49152 KB)"),
            [Report::Sending {
                partition: "boot_b".into(),
                chunk: 1,
                chunks: 1
            }]
        );
        assert_eq!(
            reports("Writing 'system_b'                                 FAILED (remote: 'Partition not found')"),
            [
                Report::Writing {
                    partition: "system_b".into()
                },
                Report::Failed("FAILED (remote: 'Partition not found')".into())
            ]
        );
        assert!(reports("Finished. Total time: 75.123s").is_empty());
    }
}
