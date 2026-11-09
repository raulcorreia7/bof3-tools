//! What the field code does with a cell's byte code (`g_AreaCellCodes`, the bundle's `+0x14` section) and with a
//! section) and with a cell's value in the packed battle grid (`0x80032000`).
//! `Area_GetCellCode` (`exe/slus_004_22@0x80166CB0`) returns `codes[y * width + x]`; the walkers and

/// One meaning of a byte code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Meaning {
    /// The first and last code the meaning covers.
    pub codes: (u8, u8),
    pub name: &'static str,
    /// The function that tests the value, `Name (image@0xADDR)`.
    pub reader: &'static str,
    /// Whether that function's C matches the retail bytes; a draft reading is weaker evidence.
    pub matched: bool,
}

const fn meaning(first: u8, last: u8, name: &'static str, reader: &'static str, matched: bool) -> Meaning {
    Meaning { codes: (first, last), name, reader, matched }
}

/// The meanings the readers prove, in code order.
pub const MEANINGS: [Meaning; 20] = [
    meaning(
        0x00,
        0x00,
        "floor; on the world map the only code an encounter starts on",
        "func_801BB2B8 (emi/etc/game/00@0x801BB2B8)",
        false,
    ),
    meaning(0x10, 0x10, "raised layer edge", "func_801BB2B8 (emi/etc/game/00@0x801BB2B8)", false),
    meaning(
        0x11,
        0x11,
        "raised top layer: passable only with the actor's layer bit; stops projectiles",
        "func_801BB2B8 (emi/etc/game/00@0x801BB2B8)",
        false,
    ),
    meaning(
        0x20,
        0x2F,
        "linked step: passes when the next cell's code & 0xF1 is the same",
        "func_801C476C (emi/etc/game/00@0x801C476C)",
        true,
    ),
    meaning(
        0x30,
        0x30,
        "counter: talk to the cell two steps ahead",
        "func_801B6258 (emi/etc/game/00@0x801B6258)",
        true,
    ),
    meaning(
        0x50,
        0x55,
        "interact: 0x51 calls the scenario handler, 0x50/0x52/0x53/0x55 set mode 7",
        "func_801B6258 (emi/etc/game/00@0x801B6258)",
        true,
    ),
    meaning(0x70, 0x7F, "step test pair", "func_801BB2B8 (emi/etc/game/00@0x801BB2B8)", false),
    meaning(
        0x80,
        0x88,
        "damage floor (HP, AP or status by the low nibble)",
        "func_801C2BEC (emi/etc/game/00@0x801C2BEC)",
        true,
    ),
    meaning(
        0xA0,
        0xA0,
        "world map location (looked up in the area map points)",
        "func_801B690C (emi/etc/game/00@0x801B690C)",
        true,
    ),
    meaning(0xA1, 0xA1, "world map step kind 1", "Area_RunCellKindStep (emi/etc/game/00)", true),
    meaning(0xA2, 0xA3, "blocked", "func_801BB2B8 (emi/etc/game/00@0x801BB2B8)", false),
    meaning(
        0xA4,
        0xA4,
        "switch: arms step 7 and sets the cell's palette",
        "func_801B146C (emi/etc/game/00@0x801B146C)",
        true,
    ),
    meaning(0xA6, 0xA6, "return to the saved area and position", "func_801B146C (emi/etc/game/00@0x801B146C)", true),
    meaning(
        0xA7,
        0xA7,
        "field exit: the area change the area's exit table resolves",
        "func_801B146C (emi/etc/game/00@0x801B146C)",
        true,
    ),
    meaning(0xAE, 0xAE, "world map step kind 3", "Area_RunCellKindStep (emi/etc/game/00)", true),
    meaning(0xAF, 0xAF, "world map: immediate area change", "func_801B146C (emi/etc/game/00@0x801B146C)", true),
    meaning(
        0xB0,
        0xB8,
        "one-way ledge: passes facing the low nibble (8: facing 3 or 5)",
        "func_801BB6F0 (emi/etc/game/00@0x801BB6F0)",
        true,
    ),
    meaning(0xC0, 0xC0, "world map exit (the exit resolver)", "func_801B146C (emi/etc/game/00@0x801B146C)", true),
    meaning(0xF0, 0xF0, "wall", "func_801BB2B8 (emi/etc/game/00@0x801BB2B8)", false),
    meaning(0xFD, 0xFD, "breakable: a projectile clears it", "func_8019EEE4 (emi/etc/game/00@0x8019EEE4)", true),
];

/// The meaning of byte code `code`, when a reader tests it.
pub fn meaning_of(code: u8) -> Option<&'static Meaning> {
    MEANINGS.iter().find(|meaning| (meaning.codes.0..=meaning.codes.1).contains(&code))
}

/// Whether `code` is an exit: a field exit, a return, or a world-map area change.
pub fn is_exit(code: u8) -> bool {
    matches!(code, 0xA6 | 0xA7 | 0xAF | 0xC0)
}

/// What a battle-grid value says, as `func_801C7518` (`emi/etc/game/00@0x801C7518`, matched) and `func_801C7608`
pub fn battle_meaning(value: u8) -> String {
    if value == 0 {
        return "no battler stands here".into();
    }
    let reach = (value - 1) / 4;
    let room = (value - 1) % 4;
    format!("battlers of reach {reach} and less; room in x {}, in y {}", room & 1 == 1, room & 2 == 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meanings_do_not_overlap() {
        for pair in MEANINGS.windows(2) {
            assert!(pair[0].codes.1 < pair[1].codes.0, "{pair:?}");
        }
        assert_eq!(meaning_of(0xA7).map(|meaning| meaning.reader), Some("func_801B146C (emi/etc/game/00@0x801B146C)"));
        assert!(meaning_of(0x60).is_none(), "no reader tests 0x60");
        assert!(is_exit(0xA7) && !is_exit(0xA4));
        assert_eq!(battle_meaning(6), "battlers of reach 1 and less; room in x true, in y false");
    }
}
