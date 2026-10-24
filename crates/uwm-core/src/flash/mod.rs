//! Installing what the Firmware Studio built: the install set a kit wrote, fastboot, the plan
//! a player's state allows, and the read-back after it boots (roadmap P4). A storage set is the
//! other kind: files for the player's own drive and the UPG that takes them in, copied onto the
//! drive and started with Sony's update command.

pub mod drive;
pub mod fastboot;
pub mod plan;
pub mod set;
pub mod storage;
pub mod upg;
pub mod verify;

pub use fastboot::Fastboot;
pub use plan::{Facts, Flasher, Gate, Mode, Plan, Stage};
pub use set::InstallSet;
pub use storage::{DriveFacts, DriveVerdict, StorageSet};
pub use verify::Verdict;
