//! A synthetic disc for the tests: one archive of entries built in code, so a test can open a project
//! without the retail disc.

use std::fs;
use std::path::PathBuf;

use rebof3_emi::pack::{Payload, pack_payloads};

/// The archive the disc holds, as a path inside the disc folder.
pub const ARCHIVE: &str = "BIN/TEST.EMI";
/// The container version the archive is packed with.
const ARCHIVE_VERSION: u32 = 0;

/// One entry of the archive: its type, its load argument and its payload.
pub struct Entry<'a> {
    pub file_type: u16,
    pub ram_ptr: u32,
    pub bytes: &'a [u8],
}

/// A disc folder of archives and an empty project folder beside it, both removed when the value drops.
pub struct Disc(PathBuf);

impl Disc {
    /// A disc whose one archive, [`ARCHIVE`], holds `entries`. `name` keeps the folders of two tests apart.
    pub fn new(name: &str, entries: &[Entry<'_>]) -> Self {
        Self::with_archives(name, &[(ARCHIVE, entries)])
    }

    /// A disc of several archives, each a path inside the disc folder and its entries.
    pub fn with_archives(name: &str, archives: &[(&str, &[Entry<'_>])]) -> Self {
        let root = std::env::temp_dir().join(format!("bof3-edit-fixture-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        for (path, entries) in archives {
            let payloads: Vec<Payload<'_>> = entries.iter().map(|entry| Payload { bytes: entry.bytes, file_type: entry.file_type, ram_ptr: entry.ram_ptr }).collect();
            let archive = pack_payloads(ARCHIVE_VERSION, &payloads).expect("the archive packs");

            let file = root.join("disc").join(path);
            fs::create_dir_all(file.parent().expect("the archive has a folder")).expect("the disc folder");
            fs::write(&file, archive).expect("the archive file");
        }
        Self(root)
    }

    /// The folder that holds `BIN/`.
    pub fn disc(&self) -> PathBuf {
        self.0.join("disc")
    }

    /// The project folder beside the disc.
    pub fn project(&self) -> PathBuf {
        self.0.join("project")
    }
}

impl Drop for Disc {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
