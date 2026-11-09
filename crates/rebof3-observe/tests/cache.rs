//! A cache file is read only when the route, the machine and the layout that made it are the
//! current ones.

use std::fs;
use std::path::PathBuf;

use rebof3_observe::cache::{load, load_all, store};
use rebof3_observe::{FORMAT_VERSION, Observation, PLAYER_REVISION, Route};

fn observation(route: &Route) -> Observation {
    Observation {
        version: FORMAT_VERSION,
        route: route.name.clone(),
        route_sha: route.sha.clone(),
        disc_sha: "d1sc".into(),
        player_revision: PLAYER_REVISION.into(),
        frames: 1,
        seconds: 0.0,
        files: Vec::new(),
        uploads: 0,
        attributed_uploads: 0,
        textured_draws: 0,
        textures: Vec::new(),
    }
}

#[test]
fn a_stale_cache_is_not_read() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("observe-cache");
    let _ = fs::remove_dir_all(&root);
    let route = Route::parse("name = \"walk\"\nframes = 1\n").expect("a route");
    let fresh = observation(&route);
    store(&root, &fresh).expect("the cache writes");
    assert_eq!(load(&root, "d1sc", &route), Some(fresh.clone()));
    assert_eq!(load_all(&root, "d1sc"), vec![fresh.clone()]);

    let edited = Route::parse("name = \"walk\"\nframes = 2\n").expect("a route");
    assert_eq!(load(&root, "d1sc", &edited), None, "the route changed");
    store(&root, &Observation { player_revision: "other".into(), ..fresh.clone() }).expect("the cache writes");
    assert_eq!(load(&root, "d1sc", &route), None, "another machine made it");
    store(&root, &Observation { version: FORMAT_VERSION + 1, ..fresh }).expect("the cache writes");
    assert_eq!(load(&root, "d1sc", &route), None, "another layout");
}

#[test]
fn the_player_revision_is_the_pinned_one() {
    assert_eq!(PLAYER_REVISION.len(), 40, "Cargo.lock pins psx-machine by a full git revision");
}
