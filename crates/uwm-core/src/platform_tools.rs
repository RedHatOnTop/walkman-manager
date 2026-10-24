//! Google's SDK Platform-Tools, which the app downloads for the owner once they agree to the
//! Android SDK licence it comes under; that licence does not let anyone pass it on. Its
//! fastboot formats userdata with the mke2fs beside it, where Debian's and Ubuntu's fastboot
//! hands the job to e2fsprogs' mke2fs, which cannot write the Android sparse image `-w`
//! sends. Its adb stands in on a computer that has none.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::studio::fetch;
use crate::{Error, Result};

pub const REVISION: &str = "37.0.1";
/// The Linux archive, as Google's SDK repository lists it.
pub const URL: &str = "https://dl.google.com/android/repository/platform-tools_r37.0.1-linux.zip";
pub const BYTES: u64 = 9_054_187;
const SHA256: &str = "d230f13842f60f782a8645f9c813f8f845bf36089ea7289f28c48f17979313f1";
/// The licence the repository names for the package, and the manifest that holds its text.
pub const LICENCE: &str = "android-sdk-license";
pub const LICENCE_FROM: &str = "https://dl.google.com/android/repository/repository2-3.xml";

/// Where they are kept: `<data>/platform-tools`, as the archive unpacks.
pub fn dir(data: &Path) -> PathBuf {
    data.join("platform-tools")
}

/// The revision unpacked in `data`, as its `source.properties` says.
pub fn installed(data: &Path) -> Option<String> {
    revision(&dir(data))
}

fn revision(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("source.properties")).ok()?;
    text.lines()
        .find_map(|l| l.trim().strip_prefix("Pkg.Revision="))
        .map(|v| v.trim().to_owned())
}

pub fn fastboot(data: &Path) -> Option<PathBuf> {
    tool(data, "fastboot")
}

pub fn adb(data: &Path) -> Option<PathBuf> {
    tool(data, "adb")
}

fn tool(data: &Path, name: &str) -> Option<PathBuf> {
    installed(data)?;
    Some(dir(data).join(name)).filter(|p| p.is_file())
}

/// Downloads the pinned revision and unpacks it in place of any earlier one.
pub fn fetch(data: &Path, cancel: &AtomicBool, on_progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
    let name = fetch::file_name(URL).expect("the pinned URL ends in a file name");
    let zip = fetch::download(URL, Some(BYTES), Some(SHA256), &data.join(name), cancel, on_progress)?;
    let part = data.join("platform-tools.part");
    if part.exists() {
        std::fs::remove_dir_all(&part)?;
    }
    let unpacked = File::open(&zip).map_err(Error::from).and_then(|f| {
        zip::ZipArchive::new(f)
            .and_then(|mut a| a.extract(&part))
            .map_err(unzip)
    });
    let _ = std::fs::remove_file(&zip);
    unpacked?;
    let inside = part.join("platform-tools");
    match revision(&inside) {
        Some(r) if r == REVISION => {}
        other => {
            return Err(Error::Failed {
                tool: "platform-tools",
                detail: format!("the archive says revision {other:?}, pinned {REVISION}"),
            });
        }
    }
    let dest = dir(data);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::rename(&inside, &dest)?;
    std::fs::remove_dir(&part)?;
    Ok(())
}

fn unzip(e: zip::result::ZipError) -> Error {
    match e {
        zip::result::ZipError::Io(e) => Error::Io(e),
        other => Error::Failed {
            tool: "platform-tools",
            detail: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_revision_the_package_says() {
        let data = std::env::temp_dir().join(format!("uwm-pt-test-{}", std::process::id()));
        std::fs::create_dir_all(dir(&data)).unwrap();
        assert_eq!(installed(&data), None);
        std::fs::write(
            dir(&data).join("source.properties"),
            "Pkg.UserSrc=false\nPkg.Revision=37.0.1\n",
        )
        .unwrap();
        assert_eq!(installed(&data).as_deref(), Some(REVISION));
        assert_eq!(fastboot(&data), None, "no fastboot in it yet");
        std::fs::write(dir(&data).join("fastboot"), b"").unwrap();
        assert_eq!(fastboot(&data), Some(dir(&data).join("fastboot")));
        std::fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn the_pin_names_its_own_file() {
        assert_eq!(fetch::file_name(URL), Some("platform-tools_r37.0.1-linux.zip"));
        assert!(URL.contains(&format!("_r{REVISION}-")));
    }
}
