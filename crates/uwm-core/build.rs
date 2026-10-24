//! Takes in mmhost.exe, the Windows helper that runs Music Center's analysis engine
//! (`mmhost/mmhost.c`), from `UWM_MMHOST_EXE`. A build without it still works; it just
//! cannot analyse music.

use std::path::PathBuf;
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-env-changed=UWM_MMHOST_EXE");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("mmhost.exe");
    let host = match env::var_os("UWM_MMHOST_EXE") {
        Some(path) => {
            let path = PathBuf::from(path);
            println!("cargo:rerun-if-changed={}", path.display());
            fs::read(&path).unwrap_or_else(|e| panic!("UWM_MMHOST_EXE={}: {e}", path.display()))
        }
        None => Vec::new(),
    };
    fs::write(out, host).expect("write mmhost.exe into OUT_DIR");
}
