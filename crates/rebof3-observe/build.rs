//! Records the bof3-player revision the workspace pins, so a cached observation made by another
//! revision of the machine is rebuilt.

use std::path::Path;

/// The crate whose revision names the machine.
const MACHINE: &str = "psx-machine";

fn main() {
    let lock = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let text = std::fs::read_to_string(&lock).unwrap_or_default();
    let revision = text
        .split("[[package]]")
        .find(|package| package.lines().any(|line| line.trim() == format!("name = \"{MACHINE}\"")))
        .and_then(|package| package.lines().find_map(|line| line.trim().strip_prefix("source = \"")))
        .and_then(|source| source.trim_end_matches('"').rsplit_once('#').map(|(_, revision)| revision.to_string()))
        .unwrap_or_default();
    println!("cargo:rustc-env=BOF3_PLAYER_REVISION={revision}");
}
