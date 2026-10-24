//! The read-back after the installed system boots: the slot it booted from, the kernel's banner
//! and the hashes of files the kit names, all readable as the shell user.

use std::time::Duration;

use serde::Serialize;

use super::set::InstallSet;
use crate::Result;
use crate::adb::Adb;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileVerdict {
    pub path: String,
    pub ok: bool,
    /// The hash the player computed, or None when it could not read the file.
    pub got: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub slot: Option<String>,
    pub slot_ok: bool,
    pub kernel_ok: bool,
    /// `/proc/version` as the player has it.
    pub kernel: Option<String>,
    pub files: Vec<FileVerdict>,
    /// Paths that must be gone, and whether they are.
    pub absent: Vec<(String, bool)>,
    pub ok: bool,
}

/// Whether Android finished booting.
pub fn booted(adb: &Adb, serial: &str) -> bool {
    adb.shell(serial, "getprop sys.boot_completed", Duration::from_secs(10))
        .is_ok_and(|s| s.trim() == "1")
}

pub fn after_boot(adb: &Adb, serial: &str, set: &InstallSet) -> Result<Verdict> {
    let out = adb.shell(serial, &script(set), Duration::from_secs(60))?;
    Ok(judge(set, &out))
}

fn script(set: &InstallSet) -> String {
    let mut s = String::from("echo \"@@slot $(getprop ro.boot.slot_suffix)\"; echo \"@@kernel $(cat /proc/version)\"");
    for c in &set.after.check {
        s.push_str(&format!("; echo \"@@sum $(sha256sum {} 2>/dev/null)\"", quote(&c.path)));
    }
    for p in &set.after.absent {
        let q = quote(p);
        s.push_str(&format!(
            "; if [ -e {q} ]; then echo \"@@present {p}\"; else echo \"@@absent {p}\"; fi"
        ));
    }
    s
}

fn quote(p: &str) -> String {
    format!("'{}'", p.replace('\'', "'\\''"))
}

fn judge(set: &InstallSet, out: &str) -> Verdict {
    let mut slot = None;
    let mut kernel = None;
    let mut sums = std::collections::HashMap::new();
    let mut present = std::collections::HashSet::new();
    for line in out.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(v) = line.strip_prefix("@@slot ") {
            slot = Some(v.trim().trim_start_matches('_').to_owned()).filter(|s| !s.is_empty());
        } else if let Some(v) = line.strip_prefix("@@kernel ") {
            kernel = Some(v.trim().to_owned());
        } else if let Some(v) = line.strip_prefix("@@sum ") {
            let mut parts = v.split_whitespace();
            if let (Some(hash), Some(path)) = (parts.next(), parts.next()) {
                sums.insert(path.to_owned(), hash.to_owned());
            }
        } else if let Some(p) = line.strip_prefix("@@present ") {
            present.insert(p.to_owned());
        }
    }
    let files: Vec<FileVerdict> = set
        .after
        .check
        .iter()
        .map(|c| {
            let got = sums.get(&c.path).cloned();
            FileVerdict {
                path: c.path.clone(),
                ok: got.as_deref() == Some(c.sha256.as_str()),
                got,
            }
        })
        .collect();
    let absent: Vec<(String, bool)> = set
        .after
        .absent
        .iter()
        .map(|p| (p.clone(), !present.contains(p)))
        .collect();
    let slot_ok = slot.as_deref() == Some(set.slot.as_str());
    let kernel_ok = kernel.as_deref() == Some(set.after.kernel.as_str());
    let ok = slot_ok && kernel_ok && files.iter().all(|f| f.ok) && absent.iter().all(|(_, gone)| *gone);
    Verdict {
        slot,
        slot_ok,
        kernel_ok,
        kernel,
        files,
        absent,
        ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> InstallSet {
        serde_json::from_str(include_str!("../../tests/fixtures/install.json")).unwrap()
    }

    fn report(set: &InstallSet, slot: &str, su: bool) -> String {
        let mut out = format!("@@slot _{slot}\r\n@@kernel {}\r\n", set.after.kernel);
        for c in &set.after.check {
            out.push_str(&format!("@@sum {}  {}\r\n", c.sha256, c.path));
        }
        out.push_str(if su {
            "@@present /system/xbin/su\n"
        } else {
            "@@absent /system/xbin/su\n"
        });
        out
    }

    #[test]
    fn the_installed_build_reads_back_whole() {
        let s = set();
        let v = judge(&s, &report(&s, "b", false));
        assert!(v.ok, "{v:?}");
        assert_eq!(v.slot.as_deref(), Some("b"));
    }

    #[test]
    fn a_fallback_to_stock_or_a_stray_su_fails() {
        let s = set();
        let v = judge(&s, &report(&s, "a", false));
        assert!(!v.ok && !v.slot_ok);
        assert!(!judge(&s, &report(&s, "b", true)).ok);
        let mut bad = report(&s, "b", false);
        bad = bad.replacen(&s.after.check[1].sha256, &"0".repeat(64), 1);
        let v = judge(&s, &bad);
        assert!(!v.ok);
        assert_eq!(v.files.iter().filter(|f| !f.ok).count(), 1);
    }

    #[test]
    fn the_script_quotes_paths() {
        let s = set();
        let sh = script(&s);
        assert!(sh.contains("sha256sum '/system/framework/services.jar'"));
        assert!(sh.contains("if [ -e '/system/xbin/su' ]"));
    }
}
