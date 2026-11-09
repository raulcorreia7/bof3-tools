//! Observations on disk: `<root>/build/cache/observe/<disc sha>/<route>.json`.
//!
//! A cache file is fresh when it was made by this [`FORMAT_VERSION`], this
//! [`PLAYER_REVISION`] and, for a route the repository carries, the same route text. Anything
//! else is rebuilt, never read.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, FORMAT_VERSION, Observation, PLAYER_REVISION, Route};

/// The cache folder below a project root.
pub const FOLDER: &str = "build/cache/observe";
/// The extension of a cache file.
const EXTENSION: &str = "json";

/// The cache file of `route` on the disc whose data track hashes to `disc_sha`.
pub fn path(root: &Path, disc_sha: &str, route: &str) -> PathBuf {
    root.join(FOLDER).join(disc_sha).join(format!("{route}.{EXTENSION}"))
}

/// The cached observation of `route`, when it is fresh.
pub fn load(root: &Path, disc_sha: &str, route: &Route) -> Option<Observation> {
    read(&path(root, disc_sha, &route.name)).filter(|observation| observation.route_sha == route.sha)
}

/// Every fresh cached observation of the disc, by route name. A built-in route whose text changed
/// since its file was made is left out.
pub fn load_all(root: &Path, disc_sha: &str) -> Vec<Observation> {
    let Ok(listing) = fs::read_dir(root.join(FOLDER).join(disc_sha)) else { return Vec::new() };
    let mut found: Vec<Observation> = listing
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == EXTENSION))
        .filter_map(|path| read(&path))
        .filter(|observation| Route::built_in(&observation.route).is_none_or(|route| route.sha == observation.route_sha))
        .collect();
    found.sort_by(|a, b| a.route.cmp(&b.route));
    found
}

/// Writes `observation` to its cache file and returns the path.
///
/// # Errors
///
/// [`Error::Cache`] when the folder or the file cannot be written.
pub fn store(root: &Path, observation: &Observation) -> Result<PathBuf, Error> {
    let path = path(root, &observation.disc_sha, &observation.route);
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).map_err(|error| Error::Cache(format!("{}: {error}", folder.display())))?;
    }
    let text = serde_json::to_string(observation).map_err(|error| Error::Cache(error.to_string()))?;
    fs::write(&path, text).map_err(|error| Error::Cache(format!("{}: {error}", path.display())))?;
    Ok(path)
}

/// A cache file made by this format and machine revision.
fn read(path: &Path) -> Option<Observation> {
    let observation: Observation = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let fresh = observation.version == FORMAT_VERSION && observation.player_revision == PLAYER_REVISION && !PLAYER_REVISION.is_empty();
    fresh.then_some(observation)
}
