//! A scene's replayed VRAM, from the project's archives with their edits: what the VRAM view shows
//! and where the texture view finds its joins and resident palette rows.

use std::rc::Rc;

use rebof3_emi::classify::{Format, classify};
use rebof3_emi::map::MapBundle;
use rebof3_emi::replay::scenes::{self, Scene};
use rebof3_emi::replay::{self, Region, Vram};
use rebof3_emi::texture::scene::{Join, scene_joins};

use crate::project::{ArchivePath, Project};

/// A replayed scene and what the views read from it.
pub struct Replayed {
    pub scene: Scene,
    pub vram: Vram,
    pub regions: Vec<Region>,
    pub joins: Vec<Join>,
    /// The project revision the replay read.
    revision: u64,
}

impl Replayed {
    /// Whether this replay is of `scene` at the project's current revision.
    pub fn is_current(&self, scene: &Scene, project: &Project) -> bool {
        self.scene == *scene && self.revision == project.revision()
    }
}

/// Every default scene the project's archives allow.
pub fn catalog(project: &Project) -> Vec<Scene> {
    scenes::catalog(project.archives.iter().map(|path| path.0.as_str()))
}

/// Replays `scene` over the project, reading any archive it has not read yet. An archive the disc
/// lacks is skipped and named in [`Vram::missing`].
pub fn replay(project: &mut Project, scene: &Scene) -> Rc<Replayed> {
    for path in scene.archives() {
        let _ = project.load(&ArchivePath(path.to_string()));
    }
    let project = &*project;
    let vram = Vram::replay(&scene.steps, &mut |path| {
        let loaded = project.loaded(&ArchivePath(path.to_string()))?;
        let payloads = (0..loaded.entries().len()).map(|index| project.payload(loaded, index)).collect();
        Some(replay::Archive { entries: loaded.entries(), payloads })
    });
    let mut descriptors = Vec::new();
    for path in scene.archives() {
        let Some(loaded) = project.loaded(&ArchivePath(path.to_string())) else { continue };
        for (index, entry) in loaded.entries().iter().enumerate() {
            let payload = project.payload(loaded, index);
            if classify(entry, payload) == Format::AreaMapBundle
                && let Ok(bundle) = MapBundle::decode(payload)
            {
                descriptors.extend(bundle.records);
            }
        }
    }
    let joins = scene_joins(scene, &vram, &descriptors);
    let regions = vram.regions();
    Rc::new(Replayed { scene: scene.clone(), vram, regions, joins, revision: project.revision() })
}
