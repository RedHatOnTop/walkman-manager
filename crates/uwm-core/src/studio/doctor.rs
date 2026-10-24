//! What a kit's steps need from the owner's computer, looked up before a build starts, so a
//! missing program is named up front instead of failing twenty minutes in.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Serialize;

use super::catalog::HostNeeds;
use crate::process;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    /// `tool`, `java`, `ndk`, `sdk`, `space` or `adbKey`.
    pub kind: &'static str,
    /// The program, or what the kind looks for.
    pub name: String,
    pub ok: bool,
    /// Where it was found or what it reported: a path, a version, free bytes.
    pub found: Option<String>,
}

/// Where the Android NDK and SDK were found, for the kit's environment.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub ndk: Option<PathBuf>,
    pub sdk: Option<PathBuf>,
}

pub fn host(needs: &HostNeeds, work: &Path, home: Option<&Path>) -> (Vec<Check>, Found) {
    let mut checks = Vec::new();
    let mut found = Found::default();
    for tool in &needs.tools {
        let at = which::which(tool).ok();
        checks.push(Check {
            kind: "tool",
            name: tool.clone(),
            ok: at.is_some(),
            found: at.map(|p| p.to_string_lossy().into_owned()),
        });
    }
    for module in &needs.python {
        let ok = process::run(
            "python3",
            Command::new("python3").args(["-I", "-c", &format!("import {module}")]),
            Duration::from_secs(20),
        )
        .is_ok_and(|out| out.success());
        checks.push(Check {
            kind: "python",
            name: module.clone(),
            ok,
            found: None,
        });
    }
    if let Some(want) = needs.java {
        let release = java_release();
        checks.push(Check {
            kind: "java",
            name: want.to_string(),
            ok: release.is_some_and(|r| r >= want),
            found: release.map(|r| r.to_string()),
        });
    }
    if let Some(name) = &needs.ndk {
        found.ndk = ndk_dirs(home)
            .into_iter()
            .map(|d| if d.ends_with(name) { d } else { d.join(name) })
            .find(|d| d.join("toolchains/llvm/prebuilt/linux-x86_64/bin").is_dir());
        checks.push(Check {
            kind: "ndk",
            name: name.clone(),
            ok: found.ndk.is_some(),
            found: found.ndk.as_ref().map(|p| p.to_string_lossy().into_owned()),
        });
    }
    if let (Some(bt), Some(platform)) = (&needs.build_tools, &needs.platform) {
        found.sdk = sdk_dirs(home).into_iter().find(|d| {
            d.join("build-tools").join(bt).join("d8").exists()
                && d.join("platforms").join(platform).join("android.jar").exists()
        });
        checks.push(Check {
            kind: "sdk",
            name: format!("build-tools {bt}, {platform}"),
            ok: found.sdk.is_some(),
            found: found.sdk.as_ref().map(|p| p.to_string_lossy().into_owned()),
        });
    }
    let free = crate::library::pull::free_bytes(work);
    checks.push(Check {
        kind: "space",
        name: needs.free_gb.to_string(),
        ok: free.is_some_and(|b| b >= needs.free_gb * 1_000_000_000),
        found: free.map(|b| b.to_string()),
    });
    (checks, found)
}

/// The Java feature release `java -version` reports ("17.0.12", or "1.8.0_402" for 8).
fn java_release() -> Option<u32> {
    let out = process::run("java", Command::new("java").arg("-version"), Duration::from_secs(10)).ok()?;
    parse_java_release(&out.stderr)
}

fn parse_java_release(text: &str) -> Option<u32> {
    let quoted = text.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        return parts.next()?.parse().ok();
    }
    Some(first)
}

/// `NDK`, then the places Android Studio and the kit's own scripts put it.
fn ndk_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["NDK", "ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    if let Some(h) = home {
        dirs.push(h.join(".cache/android-ndk"));
        dirs.push(h.join("Android/Sdk/ndk"));
        dirs.push(h.join("Android/sdk/ndk"));
    }
    dirs
}

fn sdk_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["ANDROID_SDK", "ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    if let Some(h) = home {
        dirs.push(h.join("Android/sdk"));
        dirs.push(h.join("Android/Sdk"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_java_releases() {
        assert_eq!(
            parse_java_release("openjdk version \"21.0.9\" 2025-10-21\nOpenJDK Runtime Environment"),
            Some(21)
        );
        assert_eq!(parse_java_release("java version \"1.8.0_402\""), Some(8));
        assert_eq!(parse_java_release("openjdk version \"17-ea\""), Some(17));
        assert_eq!(parse_java_release("no java here"), None);
    }
}
