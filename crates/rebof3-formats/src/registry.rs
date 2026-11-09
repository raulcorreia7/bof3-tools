//! The format registry: which format an EMI entry holds, and what is known about each format.
//!
//! Each format module owns one [`Description`] per format it reads: the name, the evidence, the
//! rule that selects it and the check that proves its round trip. This module lists them in one
//! table and answers the lookups the extractor, the coverage table and the editor need, so none of
//! those holds a `match` over formats.
//!
//! Selection reads the entry header (file type plus load address) and one payload property, an
//! all-zero body, which is the reserved-slot shape the asset census counts on its own. Names and
//! addresses come from `wiki: formats/area`, `wiki: formats/audio`, `wiki: formats/graphics` and
//! `wiki: data/text`.

use std::ops::RangeInclusive;

use rebof3_emi::Entry;

use crate::{
    cells, clut, cue, encounter, map, message, residue, resources, scene, script, sep, sprite_bank, vab, vram, window,
};

/// The entry type of a code or data image: the load address picks the format.
pub const TYPE_DATA: u16 = 0;

/// What this crate can do with a format today.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// A decode → encode codec in this crate rebuilds the payload bytes.
    Codec,
    /// The layout is documented; no codec exists here yet.
    Specified,
    /// Neither a spec nor a codec.
    Unknown,
    /// Loaded and executed as code, not an asset format.
    Code,
    /// An all-zero payload: a reserved RAM slot.
    Reserve,
}

impl State {
    /// The state's name in the scoreboard.
    pub fn label(self) -> &'static str {
        match self {
            Self::Codec => "codec",
            Self::Specified => "specified",
            Self::Unknown => "unknown",
            Self::Code => "code",
            Self::Reserve => "reserve",
        }
    }
}

/// One entry format, in scoreboard order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    VramUpload,
    ClutBank,
    VabHeader,
    VabBody,
    SepPackage,
    Lz77Blob,
    CharacterBlob,
    AreaMessageBank,
    AreaCellGrid,
    AreaScriptBank,
    BattleScriptBank,
    AreaActorDefsTable,
    AreaEncounterTable,
    AreaMapBundle,
    AreaAuxWindow,
    SharedResourceTable,
    CueTable,
    MagicTable,
    CodeImage,
    Reserve,
    Unclassified,
}

/// How an entry is recognised as a format.
#[derive(Clone, Debug)]
pub enum Selector {
    /// The entry's file type.
    Type(u16),
    /// A type-0 entry loaded at one of these addresses.
    Load(&'static [u32]),
    /// A type-0 entry loaded inside this address window.
    LoadWindow(RangeInclusive<u32>),
    /// The header never selects the format: [`classify`] returns it by a payload property or as
    /// the fallback.
    Fallback,
}

impl Selector {
    /// Whether `entry` matches.
    fn matches(&self, entry: &Entry) -> bool {
        match self {
            Self::Type(file_type) => entry.file_type == *file_type,
            Self::Load(addresses) => entry.file_type == TYPE_DATA && addresses.contains(&entry.ram_ptr),
            Self::LoadWindow(window) => entry.file_type == TYPE_DATA && window.contains(&entry.ram_ptr),
            Self::Fallback => false,
        }
    }
}

/// The check that a format's codec rebuilds a payload.
#[derive(Clone, Copy, Debug)]
pub enum RoundTrip {
    /// The codec reads the entry alone; `Err` names the first difference.
    Payload(fn(&Entry, &[u8]) -> Result<(), String>),
    /// The codec reads a header and its body together, so the archive-level check runs it.
    Bank,
    /// No decode and encode pair exists.
    None,
}

/// What one format module knows about one format it reads.
#[derive(Clone, Debug)]
pub struct Description {
    /// The format this row describes.
    pub format: Format,
    /// The row name in the scoreboard and the coverage table.
    pub name: &'static str,
    /// What the crate can do with the format today.
    pub state: State,
    /// How well the format itself is documented, for the coverage table.
    pub spec_state: &'static str,
    /// The codec this crate holds for the format, for the coverage table.
    pub codec_state: &'static str,
    /// The evidence behind the row's numbers.
    pub evidence: &'static str,
    /// The file extension the entry's stored bytes are written with.
    pub extension: &'static str,
    /// How an entry is recognised as the format.
    pub selector: Selector,
    /// The identity round trip of the format's codec.
    pub round_trip: RoundTrip,
}

/// The extension of an entry whose container is the game's own: the EMI entry with its type tag.
pub const EMI_EXTENSION: &str = "emi0";

/// The number of formats.
const FORMATS: usize = Format::ALL.len();

/// Every description, in scoreboard order (the order of [`Format::ALL`]).
static REGISTRY: [&Description; FORMATS] = [
    &vram::UPLOAD,
    &clut::BANK,
    &vab::HEADER,
    &vab::BODY,
    &sep::PACKAGE,
    &sprite_bank::LZ77_BLOB,
    &sprite_bank::RAW_BLOB,
    &message::BANK,
    &cells::GRID,
    &script::AREA,
    &script::BATTLE,
    &scene::ACTOR_DEFS,
    &encounter::TABLE,
    &map::BUNDLE,
    &window::AUX,
    &resources::SHARED,
    &cue::TABLE,
    &script::MAGIC,
    &residue::CODE,
    &residue::RESERVE,
    &residue::UNCLASSIFIED,
];

impl Format {
    /// Every format, in scoreboard order.
    pub const ALL: [Format; 21] = [
        Format::VramUpload,
        Format::ClutBank,
        Format::VabHeader,
        Format::VabBody,
        Format::SepPackage,
        Format::Lz77Blob,
        Format::CharacterBlob,
        Format::AreaMessageBank,
        Format::AreaCellGrid,
        Format::AreaScriptBank,
        Format::BattleScriptBank,
        Format::AreaActorDefsTable,
        Format::AreaEncounterTable,
        Format::AreaMapBundle,
        Format::AreaAuxWindow,
        Format::SharedResourceTable,
        Format::CueTable,
        Format::MagicTable,
        Format::CodeImage,
        Format::Reserve,
        Format::Unclassified,
    ];

    /// The format module's description of this format.
    pub fn describe(self) -> &'static Description {
        REGISTRY[self as usize]
    }

    /// The row name in the scoreboard and the coverage table.
    pub fn name(self) -> &'static str {
        self.describe().name
    }

    /// What the crate can do with the format today.
    pub fn state(self) -> State {
        self.describe().state
    }

    /// How well the format itself is documented, for the coverage table.
    pub fn spec_state(self) -> &'static str {
        self.describe().spec_state
    }

    /// The codec this crate holds for the format, for the coverage table.
    pub fn codec_state(self) -> &'static str {
        self.describe().codec_state
    }

    /// The evidence behind the row's numbers.
    pub fn evidence(self) -> &'static str {
        self.describe().evidence
    }
}

/// The file extension an entry's stored bytes are written with: the real format's name where the
/// game uses one, and the entry's type tag where the container is the game's own.
///
/// The names are the formats' own: `img` is the game's VRAM upload, `clut` its CLUT bank, `map` its
/// map bundle, `vh`/`vb` the two halves of a VAB and `seq` a SEP package. The rest are the EMI entry
/// with its type tag.
pub fn extension(format: Format) -> &'static str {
    format.describe().extension
}

/// The format one entry belongs to: its file type and load address, with an all-zero payload
/// counted as a reservation wherever it sits.
pub fn classify(entry: &Entry, payload: &[u8]) -> Format {
    if entry.file_type == TYPE_DATA && payload.iter().all(|byte| *byte == 0) {
        return Format::Reserve;
    }
    REGISTRY
        .iter()
        .find(|description| description.selector.matches(entry))
        .map_or(Format::Unclassified, |description| description.format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_lists_every_format_in_order() {
        for (format, description) in Format::ALL.iter().zip(REGISTRY) {
            assert_eq!(*format, description.format, "{}", description.name);
        }
    }
}
