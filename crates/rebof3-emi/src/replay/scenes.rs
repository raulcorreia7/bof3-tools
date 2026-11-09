//! The default resident sets: which archives a scene has loaded, in order, and the palette steps
//! between them.
//!
//! This is the one place that lists them. Each list is read from the loader in the decompilation
//! and named in the function's doc comment; it stands in until those loaders run themselves. What
//! the loads write comes only from the archives' tables ([`Vram::load`](super::Vram::load)).
//!
//! Not listed: the party archives (`PL`, `BPLD`, `BPLU`), which write no VRAM or palette arena;
//! `SCENA17` and `ENDKANJI`, whose loaders are not read; and leftovers of an earlier screen,
//! which depend on the route the player took. A scene starts from the cleared frame buffer.

use super::Step;

/// The archive every scene starts from (`EMI_SLOT_FIRST`).
pub const FIRST: &str = "BIN/ETC/FIRST.EMI";
/// The title screen's archive (`EMI_SLOT_DEMO`).
pub const DEMO: &str = "BIN/ETC/DEMO.EMI";
/// The battle archive below scenario 8 (`EMI_SLOT_BATTLE`).
pub const BATTLE: &str = "BIN/BATTLE/BATTLE.EMI";

/// The areas whose `AreaLoad` flag byte (`+0x32`) has bit 0 set, the world map
/// (`wiki: data/programs#map-kinds`). The flags live in the executable, not in an archive.
pub const WORLD_MAP_AREAS: [u16; 11] = [16, 33, 45, 65, 87, 88, 104, 115, 121, 151, 152];

/// What kind of screen a scene is.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    /// `FIRST` alone, before the title.
    Frontend,
    /// The title screen.
    Title,
    /// A walked area.
    Field,
    /// A world-map area.
    WorldMap,
    /// A menu, shop or save screen.
    Menu,
    /// A Fairy Village event.
    Village,
    /// A battle.
    Battle,
    /// A spell or summon effect in battle.
    Effect,
}

impl Kind {
    /// The kind's name in a list.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Frontend => "Frontend",
            Kind::Title => "Title",
            Kind::Field => "Field",
            Kind::WorldMap => "World map",
            Kind::Menu => "Menu",
            Kind::Village => "Fairy Village",
            Kind::Battle => "Battle",
            Kind::Effect => "Battle effect",
        }
    }
}

/// A screen of the game and the loader steps that build its VRAM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scene {
    /// The scene's name: its kind and the archives it adds.
    pub name: String,
    /// What kind of screen it is.
    pub kind: Kind,
    /// The steps, in order.
    pub steps: Vec<Step>,
}

impl Scene {
    /// The archives the scene loads, in order.
    pub fn archives(&self) -> impl Iterator<Item = &str> {
        self.steps.iter().filter_map(|step| match step {
            Step::Load(path) => Some(path.as_str()),
            _ => None,
        })
    }

    fn then(mut self, kind: Kind, name: String, steps: impl IntoIterator<Item = Step>) -> Self {
        self.kind = kind;
        self.name = name;
        self.steps.extend(steps);
        self
    }
}

/// `SYS_StartFrontend`: `GFX_ClearRect(0, 0, 1024, 512)`, stream `FIRST`, `GFX_ResetPalette`;
/// `SYS_Main` then runs `GFX_UploadPalette`.
pub fn frontend() -> Scene {
    Scene {
        name: "Frontend".into(),
        kind: Kind::Frontend,
        steps: vec![Step::Clear, Step::Load(FIRST.into()), Step::ResetPalette, Step::UploadPalette],
    }
}

/// `Game_BeginTitle (emi/etc/game/01)`: stream `DEMO`, then `GFX_ResetPalette`.
pub fn title() -> Scene {
    frontend().then(Kind::Title, "Title".into(), [Step::Load(DEMO.into()), Step::ResetPalette, Step::UploadPalette])
}

/// `func_8019FAA0`, the area loader: stream the area archive, then `GFX_ResetPalette`.
pub fn field(area: &str) -> Scene {
    let kind = if area_id(area).is_some_and(|id| WORLD_MAP_AREAS.contains(&id)) { Kind::WorldMap } else { Kind::Field };
    frontend().then(
        kind,
        format!("{}: {}", kind.label(), stem(area)),
        [Step::Load(area.into()), Step::ResetPalette, Step::UploadPalette],
    )
}

/// A menu overlay over `base`. The open handler streams the archive, then copies the palette:
///
/// - `START`: `Game_LoadCampMenu` runs `GFX_ResetPalette`.
/// - `STATUS`, `SHOP`, `BATE`, `SHISU`: `Game_LoadMenuOverlay`, `Game_LoadShopOverlay`,
///   `Game_LoadAbilityMenu` and `Game_LoadStatusMenu` (`GAME_START_STREAM_AND_STAGE_PALETTE`)
///   run `GFX_CopyPalette(1)` and `GFX_CopyPalette(2)`.
/// - `SISYOU`: `Menu_OpenMaster` runs `GFX_CopyPalette(2)`.
///
/// Returns `None` for an archive that is not a menu overlay.
pub fn menu(base: Scene, archive: &str) -> Option<Scene> {
    let copies: &[Step] = match stem(archive) {
        "START" => &[Step::ResetPalette],
        "STATUS" | "SHOP" | "BATE" | "SHISU" => &[Step::CopyPalette(1), Step::CopyPalette(2)],
        "SISYOU" => &[Step::CopyPalette(2)],
        _ => return None,
    };
    let name = format!("{}: {}", Kind::Menu.label(), stem(archive));
    let mut steps = vec![Step::Load(archive.into())];
    steps.extend(copies.iter().cloned());
    steps.push(Step::UploadPalette);
    Some(base.then(Kind::Menu, name, steps))
}

/// A Fairy Village event over `base`: `Fairy_LoadCommu01` to `Fairy_LoadCommu05` stream
/// `COMMU01` to `COMMU05`, and `Fairy_FinishEventLoad (commu00/00)` runs `GFX_ResetPalette` once the
/// stream is ready.
pub fn village(base: Scene, archive: &str) -> Scene {
    let name = format!("{}: {}", Kind::Village.label(), stem(archive));
    base.then(Kind::Village, name, [Step::Load(archive.into()), Step::ResetPalette, Step::UploadPalette])
}

/// A battle over `base`. `Battle_LoadBattleArchive` streams `BATTLE`, `BATTLE2` or a `BOSS`
/// archive; the battle reset `func_801D1228 (battle/03)` copies the 16 words at arena row 15,
/// column 240, then rows 26 and 27 (`GFX_CopyPalette(0x1A)`, `GFX_CopyPalette(0x1B)`).
pub fn battle(base: Scene, archive: &str) -> Scene {
    let name = format!("{}: {}", Kind::Battle.label(), stem(archive));
    base.then(
        Kind::Battle,
        name,
        [
            Step::Load(archive.into()),
            Step::CopyWords { row: 15, column: 240, count: 16 },
            Step::CopyPalette(26),
            Step::CopyPalette(27),
            Step::UploadPalette,
        ],
    )
}

/// An archive a battle streams part-way, over the battle `base`, with the copy its ready step
/// runs:
///
/// - `BATL_DRA`: `Battle_LoadDraArchive` streams it; `Battle_FinishDraLoad` copies rows 26 and 27.
/// - `BATL_RE2`: `Battle_LoadRe2Archive` streams it; `Battle_FinishRe2Load` copies row 27.
/// - `BATL_RET`: `func_801D3E80` streams it; `Battle_FinishEffectLoad` copies row 26.
/// - `BATL_OVR`: `Battle_LoadEndOverlay` streams it; its `Battle_StartGameOver (batl_ovr/00)` copies row 26.
/// - `BATL_SE`, `COMN_SE`: `func_801D25A0` and `func_801D656C` stream them; they carry no palette.
/// - `BMAGIC` archives (slots `0x122` to `0x1B1`, `KAIZAR_*` and `MAGIC*`):
///   `Battle_LoadSelectionEffect (battle/15)` streams one; `Battle_FinishEffectLoad` clears the
///   effect-loading state and copies row 26 once the stream is ready.
///
/// Returns `None` for an archive a battle does not stream.
pub fn in_battle(base: Scene, archive: &str) -> Option<Scene> {
    let name = stem(archive);
    let (kind, copies): (Kind, &[u8]) = match name {
        "BATL_DRA" => (Kind::Battle, &[26, 27]),
        "BATL_RE2" => (Kind::Battle, &[27]),
        "BATL_RET" | "BATL_OVR" => (Kind::Battle, &[26]),
        "BATL_SE" | "COMN_SE" => (Kind::Battle, &[]),
        _ if archive.contains("/BMAGIC/") => (Kind::Effect, &[26]),
        _ => return None,
    };
    let mut steps = vec![Step::Load(archive.into())];
    steps.extend(copies.iter().map(|&row| Step::CopyPalette(row)));
    steps.push(Step::UploadPalette);
    let name = format!("{} + {name}", base.name);
    Some(base.then(kind, name, steps))
}

/// Every default scene the disc's archives allow, field areas first.
pub fn catalog<'a>(archives: impl IntoIterator<Item = &'a str>) -> Vec<Scene> {
    let archives: Vec<&str> = archives.into_iter().collect();
    let mut scenes = vec![frontend(), title()];
    for &archive in &archives {
        if area_id(archive).is_some() {
            scenes.push(field(archive));
        }
    }
    for &archive in &archives {
        if let Some(scene) = menu(frontend(), archive) {
            scenes.push(scene);
        }
        if is_village_event(archive) {
            scenes.push(village(frontend(), archive));
        }
        if is_battle_archive(archive) {
            scenes.push(battle(frontend(), archive));
        }
    }
    for &archive in &archives {
        if let Some(scene) = in_battle(battle(frontend(), BATTLE), archive) {
            scenes.push(scene);
        }
    }
    scenes
}

/// The default scene that loads `archive` last, over the frontend, or `None` when no scene does.
pub fn home(archive: &str) -> Option<Scene> {
    if archive == FIRST {
        return Some(frontend());
    }
    if archive == DEMO {
        return Some(title());
    }
    if area_id(archive).is_some() {
        return Some(field(archive));
    }
    if is_battle_archive(archive) {
        return Some(battle(frontend(), archive));
    }
    if is_village_event(archive) {
        return Some(village(frontend(), archive));
    }
    menu(frontend(), archive).or_else(|| in_battle(battle(frontend(), BATTLE), archive))
}

/// The area id of a `BIN/WORLDnn/AREAnnn.EMI` path.
pub fn area_id(archive: &str) -> Option<u16> {
    let (folder, file) = archive.rsplit_once('/')?;
    if !folder.rsplit('/').next()?.starts_with("WORLD") {
        return None;
    }
    file.strip_prefix("AREA")?.strip_suffix(".EMI")?.parse().ok()
}

/// Whether the archive is one `Battle_LoadBattleArchive` streams: `BATTLE`, `BATTLE2` or a boss.
fn is_battle_archive(archive: &str) -> bool {
    let name = stem(archive);
    name == "BATTLE" || name == "BATTLE2" || (archive.contains("/BOSS/") && name.starts_with("BOSS"))
}

/// Whether the archive is one of the village events `COMMU01` to `COMMU05`.
fn is_village_event(archive: &str) -> bool {
    stem(archive).strip_prefix("COMMU").is_some_and(|rest| rest != "00" && !rest.is_empty())
}

/// The file name without its folder and `.EMI`.
fn stem(archive: &str) -> &str {
    let file = archive.rsplit_once('/').map_or(archive, |(_, file)| file);
    file.strip_suffix(".EMI").unwrap_or(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_scene_loads_first_then_the_area() {
        let scene = field("BIN/WORLD00/AREA024.EMI");
        assert_eq!(scene.kind, Kind::Field);
        assert_eq!(scene.archives().collect::<Vec<_>>(), [FIRST, "BIN/WORLD00/AREA024.EMI"]);
        assert_eq!(scene.steps.last(), Some(&Step::UploadPalette));
        assert_eq!(field("BIN/WORLD00/AREA016.EMI").kind, Kind::WorldMap);
    }

    #[test]
    fn each_menu_copies_the_rows_its_open_handler_copies() {
        let start = menu(frontend(), "BIN/ETC/START.EMI").expect("a menu");
        assert!(start.steps.ends_with(&[Step::ResetPalette, Step::UploadPalette]));
        let status = menu(frontend(), "BIN/ETC/STATUS.EMI").expect("a menu");
        assert!(status.steps.ends_with(&[Step::CopyPalette(1), Step::CopyPalette(2), Step::UploadPalette]));
        assert!(menu(frontend(), "BIN/ETC/GAME.EMI").is_none());
    }

    #[test]
    fn a_battle_copies_rows_26_and_27_and_an_effect_row_26() {
        let battle = battle(frontend(), BATTLE);
        assert!(battle.steps.contains(&Step::CopyWords { row: 15, column: 240, count: 16 }));
        let effect = in_battle(battle, "BIN/BMAGIC/MAGIC218.EMI").expect("an effect");
        assert_eq!(effect.kind, Kind::Effect);
        assert!(effect.steps.ends_with(&[Step::CopyPalette(26), Step::UploadPalette]));
    }

    #[test]
    fn home_finds_the_scene_that_loads_an_archive() {
        assert_eq!(home(FIRST).map(|scene| scene.kind), Some(Kind::Frontend));
        assert_eq!(home("BIN/BOSS/BOSS001.EMI").map(|scene| scene.kind), Some(Kind::Battle));
        assert_eq!(home("BIN/ETC/COMMU03.EMI").map(|scene| scene.kind), Some(Kind::Village));
        assert_eq!(home("BIN/BATTLE/BATL_DRA.EMI").map(|scene| scene.kind), Some(Kind::Battle));
        assert_eq!(home("BIN/ETC/GAME.EMI"), None);
        assert_eq!(area_id("BIN/WORLD03/AREA151.EMI"), Some(151));
        assert_eq!(area_id("BIN/ETC/AREA151.EMI"), None);
    }

    #[test]
    fn the_catalog_lists_one_scene_per_screen_the_archives_allow() {
        let archives = [FIRST, DEMO, "BIN/WORLD00/AREA000.EMI", "BIN/ETC/SHOP.EMI", BATTLE, "BIN/BATTLE/BATL_RE2.EMI"];
        let names: Vec<String> = catalog(archives).into_iter().map(|scene| scene.name).collect();
        assert_eq!(
            names,
            ["Frontend", "Title", "Field: AREA000", "Menu: SHOP", "Battle: BATTLE", "Battle: BATTLE + BATL_RE2"]
        );
    }
}
