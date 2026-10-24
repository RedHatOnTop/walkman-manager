use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::process;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdbState {
    Device,
    Unauthorized,
    Offline,
    Recovery,
    Sideload,
    Bootloader,
    NoPermissions,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbDevice {
    pub serial: String,
    pub state: AdbState,
    /// USB port in the same "bus-port" form as `usb::UsbDevice::port`.
    pub usb_port: Option<String>,
    pub model: Option<String>,
}

impl AdbDevice {
    pub fn is_network(&self) -> bool {
        self.usb_port.is_none() && self.serial.contains(':')
    }
}

#[derive(Debug, Clone)]
pub struct Adb {
    bin: PathBuf,
}

/// The adb [`Adb::locate`] takes when `PATH` has none.
static FALLBACK: RwLock<Option<PathBuf>> = RwLock::new(None);

impl Adb {
    /// The configured binary, else the one on `PATH`, which other tools on the computer share
    /// a server with, else the fallback.
    pub fn locate(configured: Option<&Path>) -> Result<Self> {
        let bin = match configured {
            Some(p) if p.is_file() => p.to_path_buf(),
            Some(_) => return Err(Error::ToolMissing { tool: "adb" }),
            None => which::which("adb")
                .ok()
                .or_else(|| FALLBACK.read().unwrap().clone().filter(|p| p.is_file()))
                .ok_or(Error::ToolMissing { tool: "adb" })?,
        };
        Ok(Adb { bin })
    }

    /// Sets the adb to use when `PATH` has none: the app's copy of Google's platform-tools.
    pub fn fall_back_to(bin: Option<PathBuf>) {
        *FALLBACK.write().unwrap() = bin;
    }

    pub fn path(&self) -> &Path {
        &self.bin
    }

    fn command(&self) -> Command {
        Command::new(&self.bin)
    }

    pub fn version(&self) -> Result<String> {
        let out = process::run("adb", self.command().arg("version"), Duration::from_secs(5))?;
        Ok(out.stdout.lines().next().unwrap_or_default().trim().to_owned())
    }

    /// Lists devices. Starts the adb server when none is running, which is what
    /// every adb client does; the manager never stops or restarts it.
    pub fn devices(&self) -> Result<Vec<AdbDevice>> {
        let out = process::run("adb", self.command().args(["devices", "-l"]), Duration::from_secs(10))?;
        if !out.success() {
            return Err(Error::Failed {
                tool: "adb",
                detail: out.stderr.trim().to_owned(),
            });
        }
        Ok(parse_devices(&out.stdout))
    }

    pub fn shell(&self, serial: &str, script: &str, timeout: Duration) -> Result<String> {
        let out = process::run("adb", self.command().args(["-s", serial, "shell", script]), timeout)?;
        if out.code != Some(0) && out.stdout.is_empty() {
            return Err(Error::Failed {
                tool: "adb",
                detail: out.stderr.trim().to_owned(),
            });
        }
        Ok(out.stdout)
    }
}

impl Adb {
    /// Copies one file onto the device. adbd creates missing parent directories.
    pub fn push(&self, serial: &str, local: &Path, remote: &str, timeout: Duration, cancel: &AtomicBool) -> Result<()> {
        let mut cmd = self.command();
        cmd.args(["-s", serial, "push"]).arg(local).arg(remote);
        let out = process::run_until("adb", &mut cmd, timeout, Some(cancel))?;
        if out.success() {
            return Ok(());
        }
        let detail = [out.stderr.trim(), out.stdout.trim()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("adb push failed without a message")
            .to_owned();
        Err(Error::Failed { tool: "adb", detail })
    }
}

impl Adb {
    /// Copies one file off the device.
    pub fn pull(&self, serial: &str, remote: &str, local: &Path, timeout: Duration, cancel: &AtomicBool) -> Result<()> {
        let mut cmd = self.command();
        cmd.args(["-s", serial, "pull", remote]).arg(local);
        let out = process::run_until("adb", &mut cmd, timeout, Some(cancel))?;
        if out.success() {
            return Ok(());
        }
        let detail = [out.stderr.trim(), out.stdout.trim()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("adb pull failed without a message")
            .to_owned();
        Err(Error::Failed { tool: "adb", detail })
    }

    /// Runs a device command and returns its output byte for byte; `shell` would
    /// translate line endings on older devices and decode the bytes as text.
    pub fn exec_out(&self, serial: &str, script: &str, timeout: Duration) -> Result<Vec<u8>> {
        let (code, stdout, stderr) =
            process::run_raw("adb", self.command().args(["-s", serial, "exec-out", script]), timeout)?;
        if code != Some(0) {
            return Err(Error::Failed {
                tool: "adb",
                detail: stderr.trim().to_owned(),
            });
        }
        Ok(stdout)
    }
}

pub fn parse_devices(text: &str) -> Vec<AdbDevice> {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('*') && !l.starts_with("List of devices"))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?.to_owned();
            let state_word = fields.next()?;
            let state = match state_word {
                "device" => AdbState::Device,
                "unauthorized" => AdbState::Unauthorized,
                "offline" => AdbState::Offline,
                "recovery" => AdbState::Recovery,
                "sideload" => AdbState::Sideload,
                "bootloader" => AdbState::Bootloader,
                "no" => AdbState::NoPermissions,
                _ => AdbState::Other,
            };
            let mut usb_port = None;
            let mut model = None;
            for kv in fields {
                if let Some(v) = kv.strip_prefix("usb:") {
                    usb_port = Some(v.to_owned());
                } else if let Some(v) = kv.strip_prefix("model:") {
                    model = Some(v.replace('_', " "));
                }
            }
            Some(AdbDevice {
                serial,
                state,
                usb_port,
                model,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_device_list() {
        let text = "* daemon not running; starting now at tcp:5037\n\
* daemon started successfully\n\
List of devices attached\n\
0000test0zx50700       device usb:3-1 product:treble_arm64_bvS model:TrebleDroid_vanilla device:tdgsi_arm64_ab transport_id:39\n\
TESTA55SERIAL01         device usb:3-2 transport_id:37\n\
192.168.240.112:5555   device product:gts9 model:SM_X716N device:gts9 transport_id:3\n\
ABCDEF                 unauthorized usb:1-4 transport_id:5\n\
GHIJKL                 no permissions (missing udev rules? user is in the plugdev group); see [http://developer.android.com/tools/device.html] usb:1-5\n\n";
        let d = parse_devices(text);
        assert_eq!(d.len(), 5);
        assert_eq!(d[0].usb_port.as_deref(), Some("3-1"));
        assert_eq!(d[0].model.as_deref(), Some("TrebleDroid vanilla"));
        assert_eq!(d[1].state, AdbState::Device);
        assert!(d[2].is_network());
        assert!(!d[1].is_network());
        assert_eq!(d[3].state, AdbState::Unauthorized);
        assert_eq!(d[4].state, AdbState::NoPermissions);
        assert_eq!(d[4].usb_port.as_deref(), Some("1-5"));
    }
}
