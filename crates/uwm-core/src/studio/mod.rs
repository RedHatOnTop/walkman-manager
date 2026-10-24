//! Firmware Studio: the owner's firmware and ROM files, recognised and kept with what they
//! are, and the kits that turn them into images (docs/firmware-studio.md).

pub mod build;
pub mod catalog;
pub mod doctor;
pub mod fetch;
pub mod identify;
pub mod kit;
pub mod licence;
pub mod vault;

pub use catalog::{
    FormatPin, HostNeeds, InstallNeeds, Kit, KitArchive, KitInput, KitLicence, KitSource, KnownFile, Lang, Localized,
};
pub use identify::{Damage, Format, FormatKind, Identified};
pub use vault::{Entry, EntryView, InputView, KitView, Presence, Vault};
