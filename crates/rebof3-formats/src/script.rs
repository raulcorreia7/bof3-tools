//! The script bank (`0x800D3800` format): the area scripts, the battle scripts and the
//! magic-effect scripts share one layout, loaded at three addresses (`wiki: formats/area`).

use rebof3_emi::Entry;
use rebof3_emi::area::script::ScriptBank;

use crate::identity::difference;
use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// The load address of an area's script bank.
const AREA_ADDRESS: u32 = 0x800d_3800;
/// The load address of the battle script bank.
const BATTLE_ADDRESS: u32 = 0x800f_0800;
/// The load address of the magic-effect script bank of the `BMAGIC*` archives.
const MAGIC_ADDRESS: u32 = 0x800c_3800;

/// The area script bank as a format-registry row.
pub static AREA: Description = Description {
    format: Format::AreaScriptBank,
    name: "area script bank (0x800D3800)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/area (`0x800D3800`); size-prefixed directory",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[AREA_ADDRESS]),
    round_trip: RoundTrip::Payload(round_trip),
};

/// The battle script bank as a format-registry row.
pub static BATTLE: Description = Description {
    format: Format::BattleScriptBank,
    name: "battle script bank (0x800F0800)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/area: the 0x800D3800 format, 42 copies",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[BATTLE_ADDRESS]),
    round_trip: RoundTrip::Payload(round_trip),
};

/// The magic-effect script bank as a format-registry row.
pub static MAGIC: Description = Description {
    format: Format::MagicTable,
    name: "magic-effect script bank (0x800C3800)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "BMagic_Effect.c sets g_ScriptBank to 0x800C3800: the 0x800D3800 format, one section",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[MAGIC_ADDRESS]),
    round_trip: RoundTrip::Payload(round_trip),
};

/// Decodes the bank and encodes it again; `Err` names the first difference.
fn round_trip(_entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let bank = ScriptBank::decode(payload).map_err(|error| error.to_string())?;
    difference(&bank.encode(), payload)
}
