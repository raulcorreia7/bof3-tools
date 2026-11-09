//! Observed joins against the draw rules: where a rule and the game's own draws both name a
//! texture's depth and CLUT row, do they agree?
//!
//! The rules are the archive rule (`rebof3_formats::joins::joined_rows`) and the scene rule
//! (`rebof3_emi::texture::scene::scene_joins` in the archive's home scene). Neither side is
//! preferred here: a disagreement is reported with both readings.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use psx_formats::gfx::texture::{Clut, Depth};
use rebof3_emi::replay::scenes;
use rebof3_emi::replay::{self, Vram};
use rebof3_emi::texture::scene::scene_joins;
use rebof3_emi::{Archive, Entry};
use rebof3_formats::joins::joined_rows;
use rebof3_formats::map::MapBundle;
use rebof3_formats::{Format, classify};

use crate::{Error, Observation};

/// What one rule says about one texture, and what the draws say.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// The texture's archive.
    pub archive: String,
    /// The texture's slot.
    pub slot: usize,
    /// The rule's depth.
    pub depth: Depth,
    /// The rule's CLUT row.
    pub clut: Clut,
    /// The rule's evidence line.
    pub rule: String,
    /// What the draws say about it.
    pub verdict: Verdict,
}

/// How the draws compare with a rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// A draw used the rule's depth and CLUT row.
    Agrees,
    /// Draws used the rule's depth, never its row; these are the rows they used.
    OtherRows(Vec<[u16; 2]>),
    /// No draw used the rule's depth; these are the depths they used, in bits.
    OtherDepth(Vec<u32>),
}

/// Compares every observed texture that a rule also joins, reading the archives from `image`.
///
/// # Errors
///
/// [`Error::Disc`] when the image's file system does not read.
pub fn against_rules(observation: &Observation, image: &Path) -> Result<Vec<Comparison>, Error> {
    let mut disc = Disc::open(image)?;
    let mut comparisons = Vec::new();
    for archive in observation.coverage().into_keys() {
        let mut rules: Vec<(usize, Depth, Clut, String)> = Vec::new();
        if let Some(loaded) = disc.archive(archive) {
            let payloads: Vec<&[u8]> = (0..loaded.entries.len()).map(|slot| loaded.payload(slot)).collect();
            for (slot, rows) in joined_rows(archive, &loaded.entries, &payloads) {
                rules.extend(rows.into_iter().map(|row| (slot, row.depth, row.clut, row.evidence)));
            }
        }
        if let Some(scene) = scenes::home(archive) {
            let (vram, descriptors) = disc.replay(&scene);
            for join in scene_joins(&scene, &vram, &descriptors) {
                if let Some((owner, slot)) = join.texture.as_entry()
                    && owner == archive
                {
                    let evidence = format!("{}; scene {}", join.rule.evidence(), scene.name);
                    rules.push((slot, join.depth, join.clut, evidence));
                }
            }
        }
        for (slot, depth, clut, rule) in rules {
            let uses: Vec<_> = observation.of(archive, slot).collect();
            if uses.is_empty() {
                continue;
            }
            let same_depth: Vec<_> = uses.iter().filter(|used| used.depth() == Some(depth)).collect();
            let verdict = if same_depth.iter().any(|used| used.clut() == Some(clut)) {
                Verdict::Agrees
            } else if same_depth.is_empty() {
                let mut bits: Vec<u32> = uses.iter().map(|used| used.bits).collect();
                bits.sort_unstable();
                bits.dedup();
                Verdict::OtherDepth(bits)
            } else {
                Verdict::OtherRows(same_depth.iter().filter_map(|used| used.clut).collect())
            };
            comparisons.push(Comparison { archive: archive.to_string(), slot, depth, clut, rule, verdict });
        }
    }
    Ok(comparisons)
}

/// An archive read from the disc image.
struct Loaded {
    bytes: Vec<u8>,
    archive: Archive,
    entries: Vec<Entry>,
}

impl Loaded {
    fn payload(&self, slot: usize) -> &[u8] {
        self.archive.payload(&self.bytes, slot).unwrap_or(&[])
    }
}

/// The disc image's archives, each read once.
struct Disc {
    image: psx_cd::Image,
    files: HashMap<String, psx_cd::Entry>,
    loaded: BTreeMap<String, Option<Loaded>>,
}

impl Disc {
    fn open(path: &Path) -> Result<Self, Error> {
        let mut image = psx_cd::Image::open(path).map_err(|error| Error::Disc(error.to_string()))?;
        let files = image.entries().map_err(|error| Error::Disc(error.to_string()))?.into_iter().map(|entry| (entry.path.to_string_lossy().replace('\\', "/"), entry)).collect();
        Ok(Self { image, files, loaded: BTreeMap::new() })
    }

    fn archive(&mut self, path: &str) -> Option<&Loaded> {
        if !self.loaded.contains_key(path) {
            let loaded = self.files.get(path).and_then(|entry| {
                let bytes = self.image.read_file(entry).ok()?;
                let archive = Archive::parse(&bytes).ok()?;
                let entries = archive.entries().to_vec();
                Some(Loaded { bytes, archive, entries })
            });
            self.loaded.insert(path.to_string(), loaded);
        }
        self.loaded.get(path)?.as_ref()
    }

    /// The scene's replayed VRAM and the tile descriptors of its area map bundles.
    fn replay(&mut self, scene: &scenes::Scene) -> (Vram, Vec<u32>) {
        for path in scene.archives() {
            self.archive(path);
        }
        let loaded = &self.loaded;
        let vram = Vram::replay(&scene.steps, &mut |path| {
            let archive = loaded.get(path)?.as_ref()?;
            let payloads = (0..archive.entries.len()).map(|slot| archive.payload(slot)).collect();
            Some(replay::Archive { entries: &archive.entries, payloads })
        });
        let mut descriptors = Vec::new();
        for path in scene.archives() {
            let Some(Some(archive)) = loaded.get(path) else { continue };
            for (slot, entry) in archive.entries.iter().enumerate() {
                let payload = archive.payload(slot);
                if classify(entry, payload) == Format::AreaMapBundle
                    && let Ok(bundle) = MapBundle::decode(payload)
                {
                    descriptors.extend(bundle.records);
                }
            }
        }
        (vram, descriptors)
    }
}
