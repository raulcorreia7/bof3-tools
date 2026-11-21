//! The edit store: the payloads and load arguments a project changes, kept in memory over the disc's
//! archives, with undo steps and the plan that rebuilds each edited archive.
//!
//! An edit is the whole new payload of one entry. A placement edit is a new load argument for one entry
//! (where a texture or palette lands in VRAM). Nothing here touches a file.

use std::collections::BTreeMap;

use rebof3_emi::{Archive, Entry};

use crate::archive::{ArchivePath, Key, Loaded};

/// What a change touched, and its state before the step (`None` when the entry had no edit).
enum Before {
    Payload(Option<Vec<u8>>),
    Load(Option<u32>),
}

/// One entry's part of an undo step: the entry, and its edit before the step.
struct Change {
    key: Key,
    before: Before,
}

/// One undoable step: every entry a gesture, a typed field or an import changed, in the order it first
/// changed them.
type Step = Vec<Change>;

/// The edits of a project and their undo history.
#[derive(Default)]
pub struct Edits {
    edits: BTreeMap<Key, Vec<u8>>,
    saved: BTreeMap<Key, Vec<u8>>,
    /// The load arguments the project changes, by entry.
    loads: BTreeMap<Key, u32>,
    saved_loads: BTreeMap<Key, u32>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// The step that collects the changes while a gesture lasts.
    open: Option<Step>,
    /// Counts every change to the edits, so a view can tell when what it derived is stale.
    revision: u64,
}

impl Edits {
    /// The current payload of an entry: its edit, or the disc's bytes.
    pub fn payload<'a>(&'a self, loaded: &'a Loaded, index: usize) -> &'a [u8] {
        match self.edits.get(&(loaded.path.clone(), index)) {
            Some(bytes) => bytes,
            None => loaded.original(index),
        }
    }

    pub fn is_edited(&self, key: &Key) -> bool {
        self.edits.contains_key(key) || self.loads.contains_key(key)
    }

    pub fn archive_is_edited(&self, path: &ArchivePath) -> bool {
        self.edits.keys().chain(self.loads.keys()).any(|(archive, _)| archive == path)
    }

    /// The load argument of an entry: the project's placement, or the disc's.
    pub fn load_arg(&self, loaded: &Loaded, index: usize) -> u32 {
        self.loads.get(&(loaded.path.clone(), index)).copied().unwrap_or_else(|| loaded.entries().get(index).map_or(0, |entry| entry.ram_ptr))
    }

    /// The table entries of an archive with the project's placements: what a loader replay reads.
    pub fn entries_of(&self, loaded: &Loaded) -> Vec<Entry> {
        let mut entries = loaded.entries().to_vec();
        for ((archive, index), load_arg) in &self.loads {
            if *archive == loaded.path
                && let Some(entry) = entries.get_mut(*index)
            {
                entry.ram_ptr = *load_arg;
            }
        }
        entries
    }

    /// Sets the load argument of an entry. The disc's own value drops the placement.
    pub fn set_load_arg(&mut self, loaded: &Loaded, index: usize, load_arg: u32) {
        let key = (loaded.path.clone(), index);
        let original = loaded.entries().get(index).map(|entry| entry.ram_ptr);
        let next = (Some(load_arg) != original).then_some(load_arg);
        if self.loads.get(&key) == next.as_ref() {
            return;
        }
        let before = self.replace_load(&key, next);
        self.record(Change { key, before: Before::Load(before) });
    }

    /// A number that changes whenever an edit is made, undone or dropped.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn edit_count(&self) -> usize {
        self.edits.keys().chain(self.loads.keys()).collect::<std::collections::BTreeSet<_>>().len()
    }

    /// True when the in-memory edits differ from the saved ones.
    pub fn is_dirty(&self) -> bool {
        self.edits != self.saved || self.loads != self.saved_loads
    }

    /// Sets the payload of an entry. A payload equal to the disc's drops the edit.
    pub fn set(&mut self, loaded: &Loaded, index: usize, bytes: Vec<u8>) {
        let key = (loaded.path.clone(), index);
        let next = (bytes.as_slice() != loaded.original(index)).then_some(bytes);
        if self.edits.get(&key) == next.as_ref() {
            return;
        }
        let before = self.replace(&key, next);
        self.record(Change { key, before: Before::Payload(before) });
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

    /// Drops the edits of an entry, so it reads the disc's bytes and load argument again.
    pub fn revert(&mut self, key: &Key) {
        if self.edits.contains_key(key) {
            let before = self.replace(key, None);
            self.record(Change { key: key.clone(), before: Before::Payload(before) });
        }
        if self.loads.contains_key(key) {
            let before = self.replace_load(key, None);
            self.record(Change { key: key.clone(), before: Before::Load(before) });
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
        let same = |earlier: &Change| earlier.key == change.key && std::mem::discriminant(&earlier.before) == std::mem::discriminant(&change.before);
        match self.open.as_mut() {
            Some(step) if step.iter().any(same) => {}
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
                let before = match change.before {
                    Before::Payload(bytes) => Before::Payload(self.replace(&change.key, bytes)),
                    Before::Load(load_arg) => Before::Load(self.replace_load(&change.key, load_arg)),
                };
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

    fn replace_load(&mut self, key: &Key, next: Option<u32>) -> Option<u32> {
        self.revision += 1;
        match next {
            Some(load_arg) => self.loads.insert(key.clone(), load_arg),
            None => self.loads.remove(key),
        }
    }

    /// The bytes of an archive as the project has it now: the disc's archive rebuilt with the archive's
    /// edits, or the disc's bytes when it has none.
    pub fn current_archive(&self, loaded: &Loaded) -> Result<Vec<u8>, String> {
        let replace: Vec<(usize, &[u8])> = self.edits.iter().filter(|((archive, _), _)| *archive == loaded.path).map(|((_, index), bytes)| (*index, bytes.as_slice())).collect();
        let placed = self.placed(&loaded.path);
        if replace.is_empty() && placed.is_empty() {
            return Ok(loaded.bytes.to_vec());
        }
        rebof3_emi::pack::rebuild_placed(&loaded.bytes, &replace, &placed).map_err(|error| format!("{}: {error}", loaded.path.0))
    }

    /// The load arguments the project changes in an archive, by entry.
    fn placed(&self, path: &ArchivePath) -> Vec<(usize, u32)> {
        self.loads.iter().filter(|((archive, _), _)| archive == path).map(|((_, index), load)| (*index, *load)).collect()
    }

    /// What rebuilds each edited archive: one plan per archive with a payload or a placement edit.
    pub fn plan(&self) -> Vec<RebuildPlan> {
        let mut by_archive: BTreeMap<ArchivePath, Vec<(usize, Vec<u8>)>> = BTreeMap::new();
        for ((archive, index), bytes) in &self.edits {
            by_archive.entry(archive.clone()).or_default().push((*index, bytes.clone()));
        }
        for (archive, _) in self.loads.keys() {
            by_archive.entry(archive.clone()).or_default();
        }
        by_archive.into_iter().map(|(archive, replace)| RebuildPlan { placed: self.placed(&archive), archive, replace }).collect()
    }

    /// The edited payloads, by entry.
    pub fn payloads(&self) -> &BTreeMap<Key, Vec<u8>> {
        &self.edits
    }

    /// The placed load arguments, by entry.
    pub fn loads(&self) -> &BTreeMap<Key, u32> {
        &self.loads
    }

    /// The saved edits the project no longer holds: the payloads and the placements.
    pub fn dropped(&self) -> (Vec<Key>, Vec<Key>) {
        let gone = |saved: Vec<&Key>, now: Vec<&Key>| saved.into_iter().filter(|key| !now.contains(key)).cloned().collect();
        (gone(self.saved.keys().collect(), self.edits.keys().collect()), gone(self.saved_loads.keys().collect(), self.loads.keys().collect()))
    }

    /// Takes `payloads` and `loads` as both the edits and the saved state: a project just read from its folder.
    pub fn restore(&mut self, payloads: BTreeMap<Key, Vec<u8>>, loads: BTreeMap<Key, u32>) {
        self.saved = payloads.clone();
        self.saved_loads = loads.clone();
        self.edits = payloads;
        self.loads = loads;
    }

    /// Marks the edits as saved.
    pub fn mark_saved(&mut self) {
        self.saved = self.edits.clone();
        self.saved_loads = self.loads.clone();
    }
}

/// One edited archive and what it takes to rebuild it: the replaced payloads and the placed load arguments.
pub struct RebuildPlan {
    pub archive: ArchivePath,
    replace: Vec<(usize, Vec<u8>)>,
    placed: Vec<(usize, u32)>,
}

impl RebuildPlan {
    /// Rebuilds `loaded` with the edits, and checks that the result reads back with the edited payloads
    /// and the disc's other payloads.
    pub fn rebuild(&self, loaded: &Loaded) -> Result<Vec<u8>, String> {
        let replace: Vec<(usize, &[u8])> = self.replace.iter().map(|(index, bytes)| (*index, bytes.as_slice())).collect();
        let rebuilt = rebof3_emi::pack::rebuild_placed(&loaded.bytes, &replace, &self.placed).map_err(|error| format!("{}: {error}", self.archive.0))?;
        verify(loaded, &rebuilt, &replace, &self.placed).map_err(|error| format!("{}: {error}", self.archive.0))?;
        Ok(rebuilt)
    }
}

/// Checks a rebuilt archive: same entries, the replaced payloads where they were replaced, the disc's
/// payloads everywhere else, and the placed load arguments where they were placed.
fn verify(loaded: &Loaded, rebuilt: &[u8], replace: &[(usize, &[u8])], placed: &[(usize, u32)]) -> Result<(), String> {
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
        let load = placed.iter().find(|(at, _)| *at == index).map_or(loaded.entries()[index].ram_ptr, |(_, load)| *load);
        if archive.entries()[index].ram_ptr != load {
            return Err(format!("entry {index} reads back a different load argument"));
        }
    }
    Ok(())
}
