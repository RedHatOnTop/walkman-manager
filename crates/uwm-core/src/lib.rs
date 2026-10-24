//! Device knowledge and operations shared by the desktop app and the CLI.

pub mod adb;
pub mod error;
pub mod flash;
pub mod library;
pub mod platform_tools;
pub mod probe;
pub mod process;
pub mod profile;
pub mod registry;
pub mod studio;
pub mod usb;

pub use error::{Error, ErrorReport, Result, Why};
