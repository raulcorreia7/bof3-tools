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

use rebof3_emi::{Archive, Entry};
use rebof3_formats::{Format, classify};

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

/// One entry's part of an undo step: the entry, and its edit before the step (`None` when it had none).
struct Change {
    key: Key,
    before: Option<Vec<u8>>,
}

/// One undoable step: every entry a gesture, a typed field or an import changed, in the order it first
/// changed them.
type Step = Vec<Change>;

pub struct Project {
    pub disc: PathBuf,
    pub folder: PathBuf,
    pub archives: Vec<ArchivePath>,
    edits: BTreeMap<Key, Vec<u8>>,
    saved: BTreeMap<Key, Vec<u8>>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// The step that collects the changes while a gesture lasts.
    open: Option<Step>,
    cache: BTreeMap<ArchivePath, Arc<Loaded>>,
    /// Counts every change to the edits, so a view can tell when what it derived is stale.
    revision: u64,
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
            open: None,
            cache: BTreeMap::new(),
            revision: 0,
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

    /// An archive that [`Project::load`] has read already.
    pub fn loaded(&self, path: &ArchivePath) -> Option<&Loaded> {
        self.cache.get(path).map(Arc::as_ref)
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

    /// A number that changes whenever an edit is made, undone or dropped.
    pub fn revision(&self) -> u64 {
        self.revision
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
        self.record(Change { key, before });
    }

    /// Sets the payloads of several entries of one archive as one undo step.
    pub fn set_together(&mut self, loaded: &Loaded, payloads: Vec<(usize, Vec<u8>)>) {
        let gesture = self.open.is_some();
        if !gesture {
            self.begin_step();
        }
        for (index, bytes) in payloads {
            self.set(loaded, index, bytes);
        }
        if !gesture {
            self.end_step();
        }
    }

    /// Drops the edit of an entry, so it reads the disc's bytes again.
    pub fn revert(&mut self, key: &Key) {
        if self.edits.contains_key(key) {
            let before = self.replace(key, None);
            self.record(Change { key: key.clone(), before });
        }
    }

    /// Starts a step that collects every change until [`Project::end_step`], so one undo takes back a
    /// whole drag or a whole typed field. A step still open ends first.
    pub fn begin_step(&mut self) {
        self.end_step();
        self.open = Some(Step::new());
    }

    /// Ends the open step. A step that changed nothing leaves no undo step.
    pub fn end_step(&mut self) {
        if let Some(step) = self.open.take().filter(|step| !step.is_empty()) {
            self.undo.push(step);
        }
    }

    /// Adds a change to the open step, or makes it a step of its own. A step keeps the first state of
    /// each entry it changed.
    fn record(&mut self, change: Change) {
        self.redo.clear();
        match self.open.as_mut() {
            Some(step) if step.iter().any(|earlier| earlier.key == change.key) => {}
            Some(step) => step.push(change),
            None => self.undo.push(vec![change]),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.open.as_ref().is_some_and(|step| !step.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        self.end_step();
        if let Some(step) = self.undo.pop() {
            let back = self.apply(step);
            self.redo.push(back);
        }
    }

    pub fn redo(&mut self) {
        self.end_step();
        if let Some(step) = self.redo.pop() {
            let back = self.apply(step);
            self.undo.push(back);
        }
    }

    /// Puts each entry of `step` back to its recorded state, last change first. Returns the step that
    /// undoes this one.
    fn apply(&mut self, step: Step) -> Step {
        let mut back: Step = step
            .into_iter()
            .rev()
            .map(|change| {
                let before = self.replace(&change.key, change.before);
                Change { key: change.key, before }
            })
            .collect();
        back.reverse();
        back
    }

    fn replace(&mut self, key: &Key, next: Option<Vec<u8>>) -> Option<Vec<u8>> {
        self.revision += 1;
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

    /// The bytes of an archive as the project has it now: the disc's archive rebuilt with the archive's
    /// edits, or the disc's bytes when it has none.
    pub fn current_archive(&self, loaded: &Loaded) -> Result<Vec<u8>, String> {
        let replace: Vec<(usize, &[u8])> = self
            .edits
            .iter()
            .filter(|((archive, _), _)| *archive == loaded.path)
            .map(|((_, index), bytes)| (*index, bytes.as_slice()))
            .collect();
        if replace.is_empty() {
            return Ok(loaded.bytes.to_vec());
        }
        rebof3_emi::pack::rebuild(&loaded.bytes, &replace).map_err(|error| format!("{}: {error}", loaded.path.0))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{ARCHIVE, Disc, Entry};

    /// A disc of two type-0 entries, opened as a project with its archive loaded.
    fn open(name: &str) -> (Disc, Project, Arc<Loaded>) {
        let entries =
            [b"first".as_slice(), b"second".as_slice()].map(|bytes| Entry { file_type: 0, ram_ptr: 0, bytes });
        let disc = Disc::new(name, &entries);
        let mut project = Project::open(&disc.disc(), &disc.project()).expect("project opens");
        let loaded = project.load(&ArchivePath(ARCHIVE.into())).expect("archive loads");
        (disc, project, loaded)
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
