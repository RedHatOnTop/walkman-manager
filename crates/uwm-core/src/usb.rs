use nusb::MaybeFuture;
use serde::Serialize;

use crate::error::{Error, Result};

pub const VID_SONY: u16 = 0x054c;
pub const VID_MEDIATEK: u16 = 0x0e8d;
pub const VID_NXP: u16 = 0x1fc9;

/// What a USB device offers, read from its descriptors without opening it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Function {
    Adb,
    Fastboot,
    MassStorage,
    Mtp,
    MtkBrom,
    MtkPreloader,
    NxpSerialDownload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbDevice {
    /// Bus and port chain, e.g. "3-1.2"; stable while the cable stays in one port.
    pub port: String,
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub speed: Option<&'static str>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, Copy)]
pub struct InterfaceClass {
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
}

pub fn functions_for(vid: u16, pid: u16, interfaces: &[(InterfaceClass, Option<&str>)]) -> Vec<Function> {
    let mut out = Vec::new();
    match (vid, pid) {
        (VID_MEDIATEK, 0x0003) => out.push(Function::MtkBrom),
        (VID_MEDIATEK, 0x2000 | 0x2001 | 0x20ff | 0x3000 | 0x6000) => out.push(Function::MtkPreloader),
        (VID_NXP, 0x0134 | 0x0135 | 0x012b | 0x0146) => out.push(Function::NxpSerialDownload),
        _ => {}
    }
    for (c, name) in interfaces {
        let f = match (c.class, c.subclass, c.protocol) {
            (0xff, 0x42, 0x01) => Some(Function::Adb),
            (0xff, 0x42, 0x03) => Some(Function::Fastboot),
            (0x08, _, _) => Some(Function::MassStorage),
            (0x06, 0x01, 0x01) => Some(Function::Mtp),
            (0xff, _, _) if name.is_some_and(|n| n.contains("MTP")) => Some(Function::Mtp),
            _ => None,
        };
        if let Some(f) = f {
            out.push(f);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn is_relevant(vid: u16, functions: &[Function]) -> bool {
    matches!(vid, VID_SONY | VID_MEDIATEK | VID_NXP)
        || functions
            .iter()
            .any(|f| matches!(f, Function::Adb | Function::Fastboot))
}

fn speed_label(speed: Option<nusb::Speed>) -> Option<&'static str> {
    Some(match speed? {
        nusb::Speed::Low => "1.5 Mb/s",
        nusb::Speed::Full => "12 Mb/s",
        nusb::Speed::High => "480 Mb/s",
        nusb::Speed::Super => "5 Gb/s",
        nusb::Speed::SuperPlus => "10 Gb/s",
        _ => return None,
    })
}

fn port_of(d: &nusb::DeviceInfo) -> String {
    #[cfg(target_os = "linux")]
    {
        if let Some(name) = d.sysfs_path().file_name() {
            return name.to_string_lossy().into_owned();
        }
    }
    format!("{}-{}", d.bus_id(), d.device_address())
}

/// Walkman-related USB devices, plus anything exposing ADB or fastboot so a
/// player on a generic bootloader VID is not missed.
pub fn list() -> Result<Vec<UsbDevice>> {
    let devices = nusb::list_devices().wait().map_err(|e| Error::Usb(e.to_string()))?;
    let mut out: Vec<UsbDevice> = devices
        .filter_map(|d| {
            let interfaces: Vec<(InterfaceClass, Option<&str>)> = d
                .interfaces()
                .map(|i| {
                    (
                        InterfaceClass {
                            class: i.class(),
                            subclass: i.subclass(),
                            protocol: i.protocol(),
                        },
                        i.interface_string(),
                    )
                })
                .collect();
            let functions = functions_for(d.vendor_id(), d.product_id(), &interfaces);
            if !is_relevant(d.vendor_id(), &functions) {
                return None;
            }
            Some(UsbDevice {
                port: port_of(&d),
                vid: d.vendor_id(),
                pid: d.product_id(),
                serial: d.serial_number().map(str::to_owned).filter(|s| !s.is_empty()),
                manufacturer: d.manufacturer_string().map(str::to_owned),
                product: d.product_string().map(str::to_owned),
                speed: speed_label(d.speed()),
                functions,
            })
        })
        .collect();
    out.sort_by(|a, b| a.port.cmp(&b.port));
    Ok(out)
}

/// Blocks the calling thread and invokes `on_change` after every connect or
/// disconnect. Returns only if the platform watcher fails to start.
pub fn watch(mut on_change: impl FnMut()) -> Result<()> {
    let watch = nusb::watch_devices().map_err(|e| Error::Usb(e.to_string()))?;
    for _event in futures_lite::stream::block_on(watch) {
        on_change();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ic(class: u8, subclass: u8, protocol: u8) -> InterfaceClass {
        InterfaceClass {
            class,
            subclass,
            protocol,
        }
    }

    #[test]
    fn a55_stock_is_storage_plus_adb() {
        let f = functions_for(
            VID_SONY,
            0x0bc1,
            &[(ic(8, 6, 0x50), Some("Mass Storage")), (ic(0xff, 0x42, 1), None)],
        );
        assert_eq!(f, [Function::Adb, Function::MassStorage]);
    }

    #[test]
    fn boot_modes_by_id() {
        assert_eq!(functions_for(VID_MEDIATEK, 0x0003, &[]), [Function::MtkBrom]);
        assert_eq!(functions_for(VID_MEDIATEK, 0x2000, &[]), [Function::MtkPreloader]);
        assert_eq!(functions_for(VID_NXP, 0x0134, &[]), [Function::NxpSerialDownload]);
        assert_eq!(
            functions_for(0x18d1, 0x4ee0, &[(ic(0xff, 0x42, 3), None)]),
            [Function::Fastboot]
        );
    }

    #[test]
    fn mtp_by_class_or_name() {
        assert_eq!(functions_for(VID_SONY, 1, &[(ic(6, 1, 1), None)]), [Function::Mtp]);
        assert_eq!(
            functions_for(VID_SONY, 1, &[(ic(0xff, 0xff, 0), Some("MTP"))]),
            [Function::Mtp]
        );
        assert!(!is_relevant(
            0x046d,
            &functions_for(0x046d, 0xc548, &[(ic(3, 1, 1), None)])
        ));
    }
}
