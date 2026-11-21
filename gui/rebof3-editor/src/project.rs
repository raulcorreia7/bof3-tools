//! A project: a read-only disc folder, and a separate project folder that holds the edits and the
//! rebuilt archives.
//!
//! The disc files are only ever read. An edit is the whole new payload of one entry, kept in memory and
//! saved as `edits/<archive>/<index>.bin` in the project folder. A placement edit is a new load argument for
//! one entry (where a texture or palette lands in VRAM), saved as `edits/<archive>/<index>.load` in hex.
//! The edit store is [`Edits`]; this is its shell: the disc folder, the cache of read archives and the files of the
//! project folder. Export rebuilds each edited archive with [`rebof3_emi::pack::rebuild_placed`] into `disc/<archive>` in
//! the project folder and reads it back to prove the rebuild holds exactly the edited payloads and load
//! arguments.

use std::collections::BTreeMap;
use std::fs;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use rebof3_catalog::archive::{ArchivePath, Key, Loaded};
pub use rebof3_catalog::edits::Edits;

pub struct Project {
    pub disc: PathBuf,
    pub folder: PathBuf,
    pub archives: Vec<ArchivePath>,
    edits: Edits,
    cache: BTreeMap<ArchivePath, Arc<Loaded>>,
}

impl Deref for Project {
    type Target = Edits;

    fn deref(&self) -> &Edits {
        &self.edits
    }
}

impl DerefMut for Project {
    fn deref_mut(&mut self) -> &mut Edits {
        &mut self.edits
    }
}

impl Project {
    /// Opens a disc folder (the one that holds `BIN/`) with a project folder for the edits. The two
    /// folders must not contain each other, so nothing the project writes can land on a disc file.
    pub fn open(disc: &Path, folder: &Path) -> Result<Self, String> {
        let disc = disc.canonicalize().map_err(|error| format!("disc folder {}: {error}", disc.display()))?;
        fs::create_dir_all(folder).map_err(|error| format!("project folder {}: {error}", folder.display()))?;
        let folder = folder.canonicalize().map_err(|error| format!("project folder: {error}"))?;
        if folder.starts_with(&disc) || disc.starts_with(&folder) {
            return Err("the project folder must be outside the disc folder, and the other way round".into());
        }
        let mut archives = Vec::new();
        collect_archives(&disc, &disc, &mut archives);
        if archives.is_empty() {
            return Err(format!("no .EMI archives under {}", disc.display()));
        }
        archives.sort();
        let mut project = Self { disc, folder, archives, edits: Edits::default(), cache: BTreeMap::new() };
        project.load_edits();
        Ok(project)
    }

    /// Reads an archive from the disc, once.
    pub fn load(&mut self, path: &ArchivePath) -> Result<Arc<Loaded>, String> {
        if let Some(loaded) = self.cache.get(path) {
            return Ok(loaded.clone());
        }
        let bytes = fs::read(self.disc.join(&path.0)).map_err(|error| format!("{}: {error}", path.0))?;
        let loaded = Arc::new(Loaded::parse(path.clone(), bytes).map_err(|error| format!("{}: {error}", path.0))?);
        self.cache.insert(path.clone(), loaded.clone());
        Ok(loaded)
    }

    /// An archive that [`Project::load`] has read already.
    pub fn loaded(&self, path: &ArchivePath) -> Option<&Loaded> {
        self.cache.get(path).map(Arc::as_ref)
    }

    fn edits_folder(&self) -> PathBuf {
        self.folder.join("edits")
    }

    fn edit_file(&self, (archive, index): &Key) -> PathBuf {
        self.edits_folder().join(&archive.0).join(format!("{index}.bin"))
    }

    fn load_file(&self, (archive, index): &Key) -> PathBuf {
        self.edits_folder().join(&archive.0).join(format!("{index}.load"))
    }

    fn load_edits(&mut self) {
        let (mut payloads, mut loads) = (BTreeMap::new(), BTreeMap::new());
        for archive in &self.archives {
            let folder = self.edits_folder().join(&archive.0);
            let Ok(files) = fs::read_dir(&folder) else { continue };
            for file in files.flatten() {
                let path = file.path();
                let index = path.file_stem().and_then(|stem| stem.to_str()).and_then(|stem| stem.parse().ok());
                let Some(index) = index else { continue };
                match path.extension().and_then(|extension| extension.to_str()) {
                    Some("bin") => {
                        if let Ok(bytes) = fs::read(&path) {
                            payloads.insert((archive.clone(), index), bytes);
                        }
                    }
                    Some("load") => {
                        let load = fs::read_to_string(&path).ok().and_then(|text| u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok());
                        if let Some(load) = load {
                            loads.insert((archive.clone(), index), load);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.edits.restore(payloads, loads);
    }

    /// Writes every edit to the project folder and removes the files of dropped edits.
    pub fn save(&mut self) -> Result<usize, String> {
        let (payloads, loads) = self.edits.dropped();
        for key in &payloads {
            let _ = fs::remove_file(self.edit_file(key));
        }
        for key in &loads {
            let _ = fs::remove_file(self.load_file(key));
        }
        for (key, bytes) in self.edits.payloads() {
            write_file(&self.edit_file(key), bytes)?;
        }
        for (key, load_arg) in self.edits.loads() {
            write_file(&self.load_file(key), format!("{load_arg:#010x}\n").as_bytes())?;
        }
        self.edits.mark_saved();
        Ok(self.edits.edit_count())
    }

    /// Rebuilds every edited archive into `disc/` in the project folder, and checks that each rebuilt
    /// archive reads back with the edited payloads and the disc's other payloads.
    pub fn export(&mut self) -> Result<Vec<PathBuf>, String> {
        let mut written = Vec::new();
        for plan in self.edits.plan() {
            let loaded = self.load(&plan.archive)?;
            let file = self.folder.join("disc").join(&plan.archive.0);
            write_file(&file, &plan.rebuild(&loaded)?)?;
            written.push(file);
        }
        Ok(written)
    }
}

/// Writes `bytes` to `file`, making its folder first.
fn write_file(file: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(file, bytes).map_err(|error| format!("{}: {error}", file.display()))
}

fn collect_archives(root: &Path, folder: &Path, archives: &mut Vec<ArchivePath>) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_archives(root, &path, archives);
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("emi"))
            && let Ok(relative) = path.strip_prefix(root)
        {
            let parts: Vec<_> = relative.components().map(|part| part.as_os_str().to_string_lossy()).collect();
            archives.push(ArchivePath(parts.join("/")));
        }
    }
}

#[cfg(test)]
mod tests {
    use rebof3_emi::Archive;

    use super::*;
    use crate::fixture::{ARCHIVE, Disc, Entry};

    /// A disc of two type-0 entries, opened as a project with its archive loaded.
    fn open(name: &str) -> (Disc, Project, Arc<Loaded>) {
        let entries = [b"first".as_slice(), b"second".as_slice()].map(|bytes| Entry { file_type: 0, ram_ptr: 0, bytes });
        let disc = Disc::new(name, &entries);
        let mut project = Project::open(&disc.disc(), &disc.project()).expect("project opens");
        let loaded = project.load(&ArchivePath(ARCHIVE.into())).expect("archive loads");
        (disc, project, loaded)
    }

    #[test]
    fn a_placement_undoes_saves_and_rebuilds() {
        let disc = Disc::new("placement", &[Entry { file_type: 3, ram_ptr: 0x1608_0100, bytes: b"texture" }, Entry { file_type: 0, ram_ptr: 0x8003_3800, bytes: b"bank" }]);
        let mut project = Project::open(&disc.disc(), &disc.project()).expect("project opens");
        let loaded = project.load(&ArchivePath(ARCHIVE.into())).expect("archive loads");
        assert_eq!(project.load_arg(&loaded, 0), 0x1608_0100);

        project.set_load_arg(&loaded, 0, 0x0204_0100);
        assert_eq!(project.load_arg(&loaded, 0), 0x0204_0100);
        assert_eq!(project.entries_of(&loaded)[0].ram_ptr, 0x0204_0100);
        assert_eq!(loaded.entries()[0].ram_ptr, 0x1608_0100, "the disc's table is untouched");
        assert!(project.is_edited(&(loaded.path.clone(), 0)) && project.is_dirty());
        let rebuilt = project.current_archive(&loaded).expect("rebuilds");
        let table = Archive::parse(&rebuilt).expect("parses");
        assert_eq!((table.entries()[0].ram_ptr, table.entries()[1].ram_ptr), (0x0204_0100, 0x8003_3800));

        project.save().expect("saves");
        assert!(!project.is_dirty());
        let mut again = Project::open(&disc.disc(), &disc.project()).expect("reopens");
        let loaded = again.load(&ArchivePath(ARCHIVE.into())).expect("loads");
        assert_eq!(again.load_arg(&loaded, 0), 0x0204_0100);
        assert_eq!(again.export().expect("exports").len(), 1);

        again.set_load_arg(&loaded, 0, 0x0306_0100);
        again.undo();
        assert_eq!(again.load_arg(&loaded, 0), 0x0204_0100);
        again.redo();
        assert_eq!(again.load_arg(&loaded, 0), 0x0306_0100);
        again.set_load_arg(&loaded, 0, 0x1608_0100);
        assert!(!again.is_edited(&(loaded.path.clone(), 0)), "the disc's value drops the placement");
        assert!(again.is_dirty(), "the saved file is still to be removed");
        again.save().expect("saves");
        assert!(!again.export().is_ok_and(|written| !written.is_empty()));
    }

    #[test]
    fn two_entries_set_together_undo_as_one_step() {
        let (_disc, mut project, loaded) = open("together");

        project.set_together(&loaded, vec![(0, b"FIRST".to_vec()), (1, b"SECOND".to_vec())]);
        assert_eq!(project.payload(&loaded, 1), b"SECOND");

        project.undo();
        assert_eq!((project.payload(&loaded, 0), project.payload(&loaded, 1)), (&b"first"[..], &b"second"[..]));
        assert!(!project.can_undo());

        project.redo();
        assert_eq!((project.payload(&loaded, 0), project.payload(&loaded, 1)), (&b"FIRST"[..], &b"SECOND"[..]));
    }

    #[test]
    fn a_gesture_undoes_to_where_it_started() {
        let (_disc, mut project, loaded) = open("gesture");
        project.set(&loaded, 0, b"one".to_vec());

        project.begin_step();
        for bytes in [b"two", b"tw3", b"t4o"] {
            project.set(&loaded, 0, bytes.to_vec());
        }
        project.end_step();

        project.undo();
        assert_eq!(project.payload(&loaded, 0), b"one");
        project.undo();
        assert_eq!(project.payload(&loaded, 0), b"first");
    }

    #[test]
    fn undo_ends_an_open_step_first() {
        let (_disc, mut project, loaded) = open("open-step");

        project.begin_step();
        project.set(&loaded, 1, b"changed".to_vec());
        assert!(project.can_undo());
        project.undo();
        assert_eq!(project.payload(&loaded, 1), b"second");

        project.begin_step();
        project.end_step();
        assert!(!project.can_undo(), "an empty step leaves nothing to undo");
    }
}
