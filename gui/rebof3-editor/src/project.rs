//! A project: a read-only disc folder, and a separate project folder that holds the edits and the
//! rebuilt archives.
//!
//! The disc files are only ever read. An edit is the whole new payload of one entry, kept in memory and
//! saved as `edits/<archive>/<index>.bin` in the project folder. Export rebuilds each edited archive with
//! [`rebof3_emi::pack::rebuild`] into `disc/<archive>` in the project folder and reads it back to prove
//! the rebuild holds exactly the edited payloads.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rebof3_emi::classify::{Format, classify};
use rebof3_emi::{Archive, Entry};

/// One archive of the disc: its path relative to the disc folder, with `/` separators.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ArchivePath(pub String);

impl ArchivePath {
    /// The folder part (`WORLD02`) and the file name (`AREA105.EMI`).
    pub fn split(&self) -> (&str, &str) {
        self.0.rsplit_once('/').unwrap_or(("", &self.0))
    }
}

/// An edit's key: the archive and the entry index.
pub type Key = (ArchivePath, usize);

/// An archive read from the disc, with each entry's format.
pub struct Loaded {
    pub path: ArchivePath,
    pub bytes: Arc<Vec<u8>>,
    pub archive: Archive,
    pub formats: Vec<Format>,
}

impl Loaded {
    pub fn entries(&self) -> &[Entry] {
        self.archive.entries()
    }

    /// The payload the disc stores for entry `index`.
    pub fn original(&self, index: usize) -> &[u8] {
        self.archive.payload(&self.bytes, index).unwrap_or(&[])
    }
}

/// One undoable change: the entry, and its edit before the change (`None` when it had none).
struct Change {
    key: Key,
    before: Option<Vec<u8>>,
}

pub struct Project {
    pub disc: PathBuf,
    pub folder: PathBuf,
    pub archives: Vec<ArchivePath>,
    edits: BTreeMap<Key, Vec<u8>>,
    saved: BTreeMap<Key, Vec<u8>>,
    undo: Vec<Change>,
    redo: Vec<Change>,
    cache: BTreeMap<ArchivePath, Arc<Loaded>>,
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
        let mut project = Self {
            disc,
            folder,
            archives,
            edits: BTreeMap::new(),
            saved: BTreeMap::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            cache: BTreeMap::new(),
        };
        project.load_edits();
        Ok(project)
    }

    /// Reads an archive from the disc, once.
    pub fn load(&mut self, path: &ArchivePath) -> Result<Arc<Loaded>, String> {
        if let Some(loaded) = self.cache.get(path) {
            return Ok(loaded.clone());
        }
        let bytes = fs::read(self.disc.join(&path.0)).map_err(|error| format!("{}: {error}", path.0))?;
        let archive = Archive::parse(&bytes).map_err(|error| format!("{}: {error}", path.0))?;
        let formats = (0..archive.entries().len())
            .map(|index| {
                let payload = archive.payload(&bytes, index).unwrap_or(&[]);
                classify(&archive.entries()[index], payload)
            })
            .collect();
        let loaded = Arc::new(Loaded { path: path.clone(), bytes: Arc::new(bytes), archive, formats });
        self.cache.insert(path.clone(), loaded.clone());
        Ok(loaded)
    }

    /// The current payload of an entry: its edit, or the disc's bytes.
    pub fn payload<'a>(&'a self, loaded: &'a Loaded, index: usize) -> &'a [u8] {
        match self.edits.get(&(loaded.path.clone(), index)) {
            Some(bytes) => bytes,
            None => loaded.original(index),
        }
    }

    pub fn is_edited(&self, key: &Key) -> bool {
        self.edits.contains_key(key)
    }

    pub fn archive_is_edited(&self, path: &ArchivePath) -> bool {
        self.edits.keys().any(|(archive, _)| archive == path)
    }

    pub fn edit_count(&self) -> usize {
        self.edits.len()
    }

    /// True when the in-memory edits differ from the saved ones.
    pub fn is_dirty(&self) -> bool {
        self.edits != self.saved
    }

    /// Sets the payload of an entry. A payload equal to the disc's drops the edit.
    pub fn set(&mut self, loaded: &Loaded, index: usize, bytes: Vec<u8>) {
        let key = (loaded.path.clone(), index);
        let next = (bytes.as_slice() != loaded.original(index)).then_some(bytes);
        if self.edits.get(&key) == next.as_ref() {
            return;
        }
        let before = self.replace(&key, next);
        self.undo.push(Change { key, before });
        self.redo.clear();
    }

    /// Drops the edit of an entry, so it reads the disc's bytes again.
    pub fn revert(&mut self, key: &Key) {
        if self.edits.contains_key(key) {
            let before = self.replace(key, None);
            self.undo.push(Change { key: key.clone(), before });
            self.redo.clear();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(change) = self.undo.pop() {
            let before = self.replace(&change.key, change.before);
            self.redo.push(Change { key: change.key, before });
        }
    }

    pub fn redo(&mut self) {
        if let Some(change) = self.redo.pop() {
            let before = self.replace(&change.key, change.before);
            self.undo.push(Change { key: change.key, before });
        }
    }

    fn replace(&mut self, key: &Key, next: Option<Vec<u8>>) -> Option<Vec<u8>> {
        match next {
            Some(bytes) => self.edits.insert(key.clone(), bytes),
            None => self.edits.remove(key),
        }
    }

    fn edits_folder(&self) -> PathBuf {
        self.folder.join("edits")
    }

    fn edit_file(&self, (archive, index): &Key) -> PathBuf {
        self.edits_folder().join(&archive.0).join(format!("{index}.bin"))
    }

    fn load_edits(&mut self) {
        for archive in self.archives.clone() {
            let folder = self.edits_folder().join(&archive.0);
            let Ok(files) = fs::read_dir(&folder) else { continue };
            for file in files.flatten() {
                let path = file.path();
                let index = path.file_stem().and_then(|stem| stem.to_str()).and_then(|stem| stem.parse().ok());
                if let (Some(index), Ok(bytes)) = (index, fs::read(&path)) {
                    self.edits.insert((archive.clone(), index), bytes);
                }
            }
        }
        self.saved = self.edits.clone();
    }

    /// Writes every edit to the project folder and removes the files of dropped edits.
    pub fn save(&mut self) -> Result<usize, String> {
        for key in self.saved.keys() {
            if !self.edits.contains_key(key) {
                let _ = fs::remove_file(self.edit_file(key));
            }
        }
        for (key, bytes) in &self.edits {
            let file = self.edit_file(key);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
            }
            fs::write(&file, bytes).map_err(|error| format!("{}: {error}", file.display()))?;
        }
        self.saved = self.edits.clone();
        Ok(self.edits.len())
    }

    /// Rebuilds every edited archive into `disc/` in the project folder, and checks that each rebuilt
    /// archive reads back with the edited payloads and the disc's other payloads.
    pub fn export(&mut self) -> Result<Vec<PathBuf>, String> {
        let mut by_archive: BTreeMap<ArchivePath, Vec<(usize, Vec<u8>)>> = BTreeMap::new();
        for ((archive, index), bytes) in &self.edits {
            by_archive.entry(archive.clone()).or_default().push((*index, bytes.clone()));
        }
        let mut written = Vec::new();
        for (archive, replaced) in by_archive {
            let loaded = self.load(&archive)?;
            let replace: Vec<(usize, &[u8])> =
                replaced.iter().map(|(index, bytes)| (*index, bytes.as_slice())).collect();
            let rebuilt = rebof3_emi::pack::rebuild(&loaded.bytes, &replace)
                .map_err(|error| format!("{}: {error}", archive.0))?;
            verify(&loaded, &rebuilt, &replace).map_err(|error| format!("{}: {error}", archive.0))?;
            let file = self.folder.join("disc").join(&archive.0);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
            }
            fs::write(&file, &rebuilt).map_err(|error| format!("{}: {error}", file.display()))?;
            written.push(file);
        }
        Ok(written)
    }
}

/// Checks a rebuilt archive: same entries, the replaced payloads where they were replaced, the disc's
/// payloads everywhere else.
fn verify(loaded: &Loaded, rebuilt: &[u8], replace: &[(usize, &[u8])]) -> Result<(), String> {
    let archive = Archive::parse(rebuilt).map_err(|error| format!("rebuilt archive does not parse: {error}"))?;
    if archive.entries().len() != loaded.entries().len() {
        return Err("rebuilt archive has a different entry count".into());
    }
    for index in 0..loaded.entries().len() {
        let expected = replace.iter().find(|(at, _)| *at == index).map_or(loaded.original(index), |(_, bytes)| bytes);
        let actual = archive.payload(rebuilt, index).map_err(|error| error.to_string())?;
        if actual != expected {
            return Err(format!("entry {index} reads back different bytes"));
        }
    }
    Ok(())
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
