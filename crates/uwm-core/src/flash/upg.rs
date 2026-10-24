//! The vendor SCSI command Sony's own updater sends to start a firmware update: the player
//! sets its update flag and restarts into its updater, which runs the UPG at the root of its
//! drive. Rockbox's scsitool sends it as `do_fw_upgrade` (utils/nwztools/scsitools, by Amaury
//! Pouly), first with 0x80 in byte 8 and then with 0; an NW-A55 refuses the first (ILLEGAL
//! REQUEST, ASC 0x24) and takes the second. A vendor command needs CAP_SYS_RAWIO, so the app
//! sends it from a second copy of itself that pkexec starts with [`HELPER_ARG`], and that copy
//! does nothing else.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::{Error, Result, Why, process};

/// The argument that makes the app's binary send the command and exit.
pub const HELPER_ARG: &str = "--upg-trigger";
const TOOK: &str = "upg-trigger: took flags";

/// Sends the command to `disk` through pkexec and returns the flags byte the player took.
/// pkexec asks the owner to authorise it; nothing is sent when they do not.
pub fn trigger(disk: &Path, cancel: &AtomicBool) -> Result<u8> {
    let pkexec = which::which("pkexec").map_err(|_| Error::ToolMissing { tool: "pkexec" })?;
    let out = process::run_until(
        "pkexec",
        Command::new(pkexec).arg(helper()?).arg(HELPER_ARG).arg(disk),
        Duration::from_secs(10 * 60),
        Some(cancel),
    )?;
    match out.code {
        Some(0) => out
            .stdout
            .lines()
            .find_map(|l| l.strip_prefix(TOOK)?.trim().strip_prefix("0x"))
            .and_then(|f| u8::from_str_radix(f, 16).ok())
            .ok_or_else(|| Error::Failed {
                tool: "upg-trigger",
                detail: out.stdout.trim().to_owned(),
            }),
        // pkexec: 126 when the owner dismissed the dialog, 127 when authorisation failed.
        Some(126 | 127) => Err(Error::unsupported(Why::NotAuthorised, "pkexec was not authorised")
            .with_detail(out.stderr.trim().to_owned())),
        _ => Err(Error::Failed {
            tool: "upg-trigger",
            detail: out.stderr.trim().to_owned(),
        }),
    }
}

/// pkexec is there to send the command with.
pub fn can_elevate() -> bool {
    which::which("pkexec").is_ok()
}

/// The program pkexec starts: the AppImage itself when the app runs from one, whose mount
/// root cannot enter.
fn helper() -> Result<PathBuf> {
    if let Some(image) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(image));
    }
    Ok(std::env::current_exe()?)
}

/// The helper's whole run: `disk` is the argument after [`HELPER_ARG`]. Prints the flags the
/// player took and returns the exit code.
pub fn helper_main(disk: Option<&std::ffi::OsStr>) -> i32 {
    let Some(disk) = disk else {
        eprintln!("usage: {HELPER_ARG} /dev/sdX");
        return 2;
    };
    match send(Path::new(disk)) {
        Ok(flags) => {
            println!("{TOOK} 0x{flags:02x}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// Only a whole SCSI disk, `/dev/sd` and letters: the helper runs as root and opens nothing
/// else.
fn whole_disk(path: &Path) -> bool {
    path.to_str()
        .and_then(|p| p.strip_prefix("/dev/sd"))
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_lowercase()))
}

#[cfg(target_os = "linux")]
fn send(disk: &Path) -> Result<u8> {
    use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
    use std::os::unix::io::AsRawFd;

    if !whole_disk(disk) {
        return Err(
            Error::unsupported(Why::NotWalkmanDrive, "not a whole SCSI disk").with_detail(disk.to_string_lossy())
        );
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(disk)?;
    if !file.metadata()?.file_type().is_block_device() {
        return Err(Error::unsupported(Why::NotWalkmanDrive, "not a block device").with_detail(disk.to_string_lossy()));
    }
    let fd = file.as_raw_fd();
    let mut inquiry = [0u8; 36];
    sg::io(fd, &[0x12, 0, 0, 0, 36, 0], &mut inquiry)?.ok()?;
    let text = |r: std::ops::Range<usize>| String::from_utf8_lossy(&inquiry[r]).trim().to_owned();
    let (vendor, product) = (text(8..16), text(16..32));
    if vendor != "SONY" || !product.starts_with("WALKMAN") {
        return Err(Error::unsupported(Why::NotWalkmanDrive, "the disk is not a WALKMAN")
            .with_detail(format!("{vendor} {product}")));
    }
    let mut refused = Vec::new();
    for flags in [0x80u8, 0] {
        let cdb = [0xfc, 0, 0x04, b'd', b'b', b'm', b'n', 0, flags, 0, 0, 0];
        let mut buf = [0u8; 0x80];
        match sg::io(fd, &cdb, &mut buf)? {
            sg::Status::Good => return Ok(flags),
            other => refused.push(format!("flags {flags:#04x}: {}", other.describe())),
        }
    }
    Err(Error::Failed {
        tool: "upg-trigger",
        detail: refused.join("; "),
    })
}

#[cfg(not(target_os = "linux"))]
fn send(_disk: &Path) -> Result<u8> {
    Err(Error::unsupported(
        Why::NotWalkmanDrive,
        "the UPG trigger is Linux only",
    ))
}

#[cfg(target_os = "linux")]
mod sg {
    use std::os::raw::{c_int, c_uchar, c_uint, c_ushort, c_void};

    use crate::{Error, Result};

    const SG_IO: c_uint = 0x2285;
    const SG_DXFER_FROM_DEV: c_int = -3;
    const SG_FLAG_LUN_INHIBIT: c_uint = 2;

    /// `struct sg_io_hdr` of <scsi/sg.h>.
    #[repr(C)]
    struct Hdr {
        interface_id: c_int,
        dxfer_direction: c_int,
        cmd_len: c_uchar,
        mx_sb_len: c_uchar,
        iovec_count: c_ushort,
        dxfer_len: c_uint,
        dxferp: *mut c_void,
        cmdp: *const c_uchar,
        sbp: *mut c_uchar,
        timeout: c_uint,
        flags: c_uint,
        pack_id: c_int,
        usr_ptr: *mut c_void,
        status: c_uchar,
        masked_status: c_uchar,
        msg_status: c_uchar,
        sb_len_wr: c_uchar,
        host_status: c_ushort,
        driver_status: c_ushort,
        resid: c_int,
        duration: c_uint,
        info: c_uint,
    }

    pub(super) enum Status {
        Good,
        /// CHECK CONDITION, with the sense key and its additional code.
        Sense {
            key: u8,
            asc: u8,
            ascq: u8,
        },
        Other {
            status: u8,
            host: u16,
            driver: u16,
        },
    }

    impl Status {
        pub(super) fn ok(self) -> Result<()> {
            match self {
                Status::Good => Ok(()),
                other => Err(Error::Failed {
                    tool: "upg-trigger",
                    detail: other.describe(),
                }),
            }
        }

        pub(super) fn describe(&self) -> String {
            match *self {
                Status::Good => "good".into(),
                Status::Sense { key, asc, ascq } => format!("sense key {key:#x}, asc {asc:#04x}, ascq {ascq:#04x}"),
                Status::Other { status, host, driver } => {
                    format!("status {status:#x}, host {host:#x}, driver {driver:#x}")
                }
            }
        }
    }

    /// One command that reads into `buf`, as rockbox's rbscsi does it: the host status first,
    /// then the SCSI status with its reserved bits masked.
    pub(super) fn io(fd: c_int, cdb: &[u8], buf: &mut [u8]) -> Result<Status> {
        let mut sense = [0u8; 32];
        let mut hdr = Hdr {
            interface_id: b'S' as c_int,
            dxfer_direction: SG_DXFER_FROM_DEV,
            cmd_len: cdb.len() as c_uchar,
            mx_sb_len: sense.len() as c_uchar,
            iovec_count: 0,
            dxfer_len: buf.len() as c_uint,
            dxferp: buf.as_mut_ptr().cast(),
            cmdp: cdb.as_ptr(),
            sbp: sense.as_mut_ptr(),
            timeout: 5000,
            flags: SG_FLAG_LUN_INHIBIT,
            pack_id: 0,
            usr_ptr: std::ptr::null_mut(),
            status: 0,
            masked_status: 0,
            msg_status: 0,
            sb_len_wr: 0,
            host_status: 0,
            driver_status: 0,
            resid: 0,
            duration: 0,
            info: 0,
        };
        // SAFETY: `hdr` points at `cdb`, `buf` and `sense`, which outlive the call, with
        // their true lengths; SG_IO writes only into `buf`, `sense` and `hdr`.
        if unsafe { libc::ioctl(fd, SG_IO as _, &mut hdr) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let status = hdr.status & 0x7e;
        Ok(if hdr.host_status != 0 {
            Status::Other {
                status,
                host: hdr.host_status,
                driver: hdr.driver_status,
            }
        } else if status == 0 {
            Status::Good
        } else if hdr.sb_len_wr >= 14 {
            Status::Sense {
                key: sense[2] & 0x0f,
                asc: sense[12],
                ascq: sense[13],
            }
        } else {
            Status::Other {
                status,
                host: 0,
                driver: hdr.driver_status,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_opens_only_whole_scsi_disks() {
        for ok in ["/dev/sdb", "/dev/sdaa"] {
            assert!(whole_disk(Path::new(ok)), "{ok}");
        }
        for no in [
            "/dev/sdb1",
            "/dev/sd",
            "/dev/nvme0n1",
            "/dev/sdb/../sda",
            "/etc/passwd",
            "sdb",
        ] {
            assert!(!whole_disk(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn the_helper_wants_its_disk() {
        assert_eq!(helper_main(None), 2);
    }
}
