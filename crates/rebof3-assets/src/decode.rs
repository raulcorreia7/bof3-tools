//! The files a format's decoder adds to the stored bytes of an asset, and the views a loader
//! reads: palette banks, texture words, and textures joined to the palette rows they are drawn
//! with.

use std::collections::HashMap;

use psx_formats::gfx::texture::{Clut, Depth, texel_indices};
use psx_formats::gfx::{clut, png, tim, vram};
use rebof3_emi::area::map::MapBundle;
use rebof3_emi::classify::{Format, classify};
use rebof3_emi::lz77;
use rebof3_emi::texture::{self as draw, Bank};

/// A file derived from a payload.
pub struct Derived {
    /// What the file is for: `png`, `json` or `decoded`.
    pub role: &'static str,
    /// The file extension, without the dot.
    pub extension: &'static str,
    /// The file's bytes.
    pub data: Vec<u8>,
}

/// A palette row a texture is drawn with, joined from a draw site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowUse {
    /// The EMI slot of the palette bank that holds the row.
    pub palette: usize,
    /// The disc path of the archive that loads the bank; the texture's own archive,
    /// unless the row came from another archive of the resident set.
    pub palette_archive: String,
    /// The row's VRAM position.
    pub clut: Clut,
    /// The texel depth the draw site's texture page selects.
    pub depth: Depth,
    /// The row's colors as RGBA (see [`color_rgba`]).
    pub colors: Vec<[u8; 4]>,
    /// The row's color words, as the bank stores them.
    pub words: Vec<u16>,
    /// The source lines that join the row to the texture.
    pub evidence: String,
}

/// A texture read at one palette row: its texel indices and the row's colors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedImage {
    /// The EMI slot of the palette bank that holds the row.
    pub palette: usize,
    /// The disc path of the archive that loads the bank.
    pub palette_archive: String,
    /// The row's VRAM position.
    pub clut: Clut,
    /// The texel depth.
    pub depth: Depth,
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// `width * height` texel indices, row-major.
    pub indices: Vec<u8>,
    /// The row's colors as RGBA.
    pub colors: Vec<[u8; 4]>,
    /// The source lines that join the row to the texture.
    pub evidence: String,
}

/// An indexed PNG of a texture read in one palette row.
pub struct RowFile {
    /// Text between the slot and the extension: `.clut-160-483`.
    pub tag: String,
    /// The draw site that joins the row to the texture.
    pub evidence: String,
    /// The PNG bytes.
    pub data: Vec<u8>,
    /// The standard PS1 TIM file of the same row: the texture's VRAM words as
    /// the image and the row's color words as the CLUT.
    pub tim: Option<Vec<u8>>,
}

/// A palette bank: its color words, their RGBA view, and where the arena loads it in VRAM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteBank {
    /// The color words in stored order.
    pub words: Vec<u16>,
    /// The RGBA view of `words` (see [`color_rgba`]).
    pub colors: Vec<[u8; 4]>,
    /// The VRAM position of the first color, when the bank loads into the palette arena.
    pub vram: Option<Clut>,
}

/// A texture upload's words: the VRAM position of its top-left word and its row-major grid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureWords {
    /// The left edge in VRAM words.
    pub x: u32,
    /// The top edge in VRAM rows.
    pub y: u32,
    /// Width in VRAM words.
    pub width: usize,
    /// Height in VRAM rows.
    pub height: usize,
    /// `width * height` words, row-major.
    pub words: Vec<u16>,
}

/// The files a format's decoder adds to the stored bytes.
pub fn derive(format: Format, load_arg: u32, payload: &[u8]) -> rebof3_emi::Result<Vec<Derived>> {
    Ok(match format {
        Format::VramUpload => {
            let image = vram::upload_image(payload, vram::Descriptor::from_load_argument(load_arg))?;
            vec![Derived { role: "png", extension: "png", data: png::rgba_png(image.width, image.height, &image.rgba) }]
        }
        Format::ClutBank => {
            let palette = clut::Palette::decode(payload)?;
            vec![
                Derived { role: "json", extension: "json", data: palette.json().into_bytes() },
                Derived { role: "png", extension: "png", data: palette.png_strip() },
            ]
        }
        Format::CueTable => vec![Derived {
            // The record is `EmiCue` in `decomp/include/audio/cue.h`, the one C
            // definition of a cue record; its format is
            // `wiki: formats/audio#emi-type-8-cue-records`.
            role: "json",
            extension: "json",
            data: rebof3_emi::cue::CueTable::decode(payload)?.json().into_bytes(),
        }],
        Format::AreaActorDefsTable => vec![Derived {
            // The record is `AreaActorDef` in `decomp/include/world/area.h`, the one C
            // definition of a scene record; its format is
            // `wiki: formats/area#0x800e3800-scenestep-table`.
            role: "json",
            extension: "json",
            data: rebof3_emi::scene::SceneTable::decode(payload)?.json().into_bytes(),
        }],
        Format::AreaScriptBank => vec![Derived {
            // The reader is `Actor_BeginScript` in `decomp/src/slus/func_8014D6DC.c:43-45`;
            // the layout is `wiki: formats/area#0x800d3800-area-script-bank`.
            role: "json",
            extension: "json",
            data: rebof3_emi::area::script::ScriptBank::decode(payload)?.json().into_bytes(),
        }],
        Format::Lz77Blob => vec![Derived { role: "decoded", extension: "bin", data: lz77::decode(payload)? }],
        _ => Vec::new(),
    })
}

/// The rate of a VAG at pitch `0x1000`, in hertz. A VAG has no rate of its own; its samples are written at this one.
pub const VAG_RATE: u32 = 44_100;

/// The PCM of a VAG as a mono 16-bit WAVE file at [`VAG_RATE`].
pub fn pcm_wav(pcm: &[i16]) -> crate::Result<Vec<u8>> {
    psx_formats::wav::pcm(pcm, VAG_RATE, 1).map_err(|error| crate::Error::Audio(error.to_string()))
}

/// Sequence `index` of a sequence package (`.seq`) as a format 0 Standard MIDI File.
pub fn sequence_midi(package: &[u8], index: usize) -> crate::Result<Vec<u8>> {
    Ok(psx_formats::sep::Package::parse(package)?.to_midi(index)?)
}

/// One VAG's stored bytes as a WAVE file at [`VAG_RATE`].
pub fn vag_wav(vag: &[u8]) -> crate::Result<Vec<u8>> {
    pcm_wav(&psx_formats::adpcm::decode_stream(vag))
}

/// The VAG `index` of a VAB bank as a WAVE file: the bank's header and body, and the VAG's index in the
/// header's VAG table.
pub fn vab_vag_wav(header: &[u8], body: &[u8], index: u32) -> crate::Result<Vec<u8>> {
    let vab = psx_formats::vab::Bank::parse(header, body)?;
    pcm_wav(&vab.decode_vag(body, index)?)
}

/// The SoundFont 2 file of one VAB bank, from its header and body. `title` names the file inside it. Each
/// tone's volume envelope is timed by the audio library's own envelope steps, from the registers the voice
/// plays.
pub fn soundfont(header: &[u8], body: &[u8], title: &str) -> crate::Result<Vec<u8>> {
    let vab = psx_formats::vab::Bank::parse(header, body)?;
    Ok(psx_formats::sf2::write_with(&vab, body, title, &volume_envelope)?)
}

/// The SoundFont envelope of a tone's ADSR registers. `None` when a phase never ends, which leaves the
/// SoundFont default on that zone.
pub(crate) fn volume_envelope(adsr1: u16, adsr2: u16) -> Option<psx_formats::sf2::Envelope> {
    let timing = rebof3_audio::spu::envelope_timing(adsr1, adsr2)?;
    Some(psx_formats::sf2::Envelope::new(timing.attack, timing.decay, timing.sustain_level, timing.release))
}

/// The lossless RGBA view of a color word (`clut::word_to_view`): `0x0000` reads as transparent and
/// the STP bit reads as half alpha, so [`crate::import`] returns the word exactly.
pub fn color_rgba(word: u16) -> [u8; 4] {
    clut::word_to_view(word)
}

/// The palette bank a type-0 payload loaded at `load_arg` holds.
pub fn palette_bank(load_arg: u32, payload: &[u8]) -> rebof3_emi::Result<PaletteBank> {
    let words = clut::Palette::decode(payload)?.words().to_vec();
    let colors = words.iter().map(|&word| color_rgba(word)).collect();
    let vram = Bank::new(0, load_arg, payload.len()).map(|bank| draw::clut_at(bank.arena_word));
    Ok(PaletteBank { words, colors, vram })
}

/// The words of a texture upload: where its rectangles land in VRAM and their row-major grid.
pub fn texture_words(load_arg: u32, payload: &[u8]) -> rebof3_emi::Result<TextureWords> {
    let descriptor = vram::Descriptor::from_load_argument(load_arg);
    let grid = vram::word_grid(payload, descriptor)?;
    let side = vram::RECTANGLE_SIDE as u32;
    Ok(TextureWords {
        x: u32::from(descriptor.column) * side,
        y: u32::from(descriptor.row) * side,
        width: grid.width,
        height: grid.height,
        words: grid.words,
    })
}

/// The palette rows each texture of one archive is drawn with, keyed by texture slot.
///
/// The rules are `rebof3_emi::texture::joins` (literal draw sites) and
/// `rebof3_emi::texture::terrain_joins` (the world-map tile descriptors). A join needs exactly one
/// sheet over its page and exactly one distinct bank over its CLUT row. `source` is the archive's
/// disc path, which selects the draw sites and owns the sheets. Each row carries its rule as
/// evidence.
pub fn joined_rows(source: &str, entries: &[rebof3_emi::Entry], payloads: &[&[u8]]) -> HashMap<usize, Vec<RowUse>> {
    joined_rows_in(source, entries, payloads, &[])
}

/// [`joined_rows`], with a resident set of palette banks available as a fallback.
///
/// `resident` holds the banks the caller loaded from other archives. A row uses this archive's
/// own bank when it has one; otherwise a resident bank may serve it, provided every bank at that
/// arena position agrees on the words. Banks of `source` itself are ignored here because
/// [`joined_rows_in`] builds them from `entries`.
pub fn joined_rows_in(
    source: &str,
    entries: &[rebof3_emi::Entry],
    payloads: &[&[u8]],
    resident: &[draw::ResidentBank],
) -> HashMap<usize, Vec<RowUse>> {
    let mut sheets = Vec::new();
    let mut own = Vec::new();
    let mut bundles = Vec::new();
    for (slot, (entry, payload)) in entries.iter().zip(payloads).enumerate() {
        match classify(entry, payload) {
            Format::VramUpload => sheets.extend(draw::sheet(slot, entry.ram_ptr, payload.len())),
            Format::ClutBank => own.extend(draw::ResidentBank::new(source, slot, entry.ram_ptr, payload)),
            Format::AreaMapBundle => {
                if let Ok(bundle) = MapBundle::decode(payload) {
                    bundles.push(bundle);
                }
            }
            _ => {}
        }
    }
    let mut banks: Vec<&draw::ResidentBank> = own.iter().collect();
    banks.extend(resident.iter().filter(|bank| bank.archive != source));

    let mut rows: HashMap<usize, Vec<RowUse>> = HashMap::new();
    for join in draw::joins(source, &sheets, &banks) {
        push_row(&mut rows, source, join);
    }
    for bundle in &bundles {
        for join in draw::terrain_joins(source, &bundle.records, &sheets, &banks) {
            push_row(&mut rows, source, join);
        }
    }
    rows
}

/// Records one join as a palette row of its texture, unless the same row is already there.
fn push_row(rows: &mut HashMap<usize, Vec<RowUse>>, source: &str, join: draw::Join<'_>) {
    let Some(words) = join.palette.row(join.clut, join.depth) else {
        return;
    };
    let texture = join.texture;
    let repeated = rows.get(&texture).is_some_and(|rows| {
        rows.iter()
            .any(|row| row.clut == join.clut && row.depth == join.depth && row.palette_archive == join.palette.archive)
    });
    if repeated {
        return;
    }
    let colors = words.iter().map(|&word| color_rgba(word)).collect();
    let mut evidence = match join.rule {
        draw::Rule::Site(site) => format!(
            "{} overlay {} function {:#010x} ({}): tpage {:#06x}, clut {:#06x}, sheet slot {}",
            site.archive, site.overlay, site.function, site.source, site.tpage, site.clut, texture
        ),
        draw::Rule::Terrain { page_x, page_y } => format!(
            "world-map terrain rule (func_80155560): page ({page_x}, {page_y}), depth {}bpp, \
             clut ({}, {}), sheet slot {texture}",
            join.depth.bits(),
            join.clut.x,
            join.clut.y,
        ),
    };
    if join.palette.archive != source {
        evidence.push_str(&format!("; palette bank {}#{}", join.palette.archive, join.palette.bank.slot));
    }
    rows.entry(texture).or_default().push(RowUse {
        palette: join.palette.bank.slot,
        palette_archive: join.palette.archive.clone(),
        clut: join.clut,
        depth: join.depth,
        colors,
        words: words.to_vec(),
        evidence,
    });
}

/// The standard PS1 TIM file of a texture read in one palette row: the
/// texture's VRAM words as the image and the row's color words as the CLUT.
///
/// The game stores no TIM, but a texture and its palette are what a TIM
/// holds, so the file is exact: the words are the words the disc stores.
pub fn texture_tim(load_arg: u32, payload: &[u8], row: &RowUse) -> crate::Result<Vec<u8>> {
    texture_tim_of(load_arg, payload, row.depth.bits(), &row.words, row.clut.x, row.clut.y)
}

/// The standard PS1 TIM file of a texture drawn with one CLUT row: `bits` is
/// 4 or 8, `words` is the row's color words, and `clut_x` and `clut_y` are the
/// row's place in the frame buffer.
pub fn texture_tim_of(
    load_arg: u32,
    payload: &[u8],
    bits: u32,
    words: &[u16],
    clut_x: u16,
    clut_y: u16,
) -> crate::Result<Vec<u8>> {
    let descriptor = vram::Descriptor::from_load_argument(load_arg);
    let grid = vram::word_grid(payload, descriptor)?;
    let mode = match bits {
        4 => tim::Mode::Four,
        8 => tim::Mode::Eight,
        other => return Err(crate::Error::Usage(format!("a texture is 4- or 8-bit, not {other}-bit"))),
    };
    let side = u16::try_from(vram::RECTANGLE_SIDE).unwrap_or(u16::MAX);
    let tim = tim::File {
        mode,
        clut: Some(tim::Block {
            x: clut_x,
            y: clut_y,
            width: u16::try_from(words.len()).unwrap_or(u16::MAX),
            height: 1,
            words: words.to_vec(),
        }),
        image: tim::Block {
            x: u16::from(descriptor.column).saturating_mul(side),
            y: u16::from(descriptor.row).saturating_mul(side),
            width: u16::try_from(grid.width).unwrap_or(u16::MAX),
            height: u16::try_from(grid.height).unwrap_or(u16::MAX),
            words: grid.words,
        },
    };
    Ok(tim.write())
}

/// The texture of an upload read at each palette row in `rows`: its texel indices at the row's depth.
pub fn indexed_images(load_arg: u32, payload: &[u8], rows: &[RowUse]) -> rebof3_emi::Result<Vec<IndexedImage>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let grid = vram::word_grid(payload, vram::Descriptor::from_load_argument(load_arg))?;
    Ok(rows
        .iter()
        .map(|row| IndexedImage {
            palette: row.palette,
            palette_archive: row.palette_archive.clone(),
            clut: row.clut,
            depth: row.depth,
            width: grid.width * row.depth.texels_per_word(),
            height: grid.height,
            indices: texel_indices(&grid.words, row.depth),
            colors: row.colors.clone(),
            evidence: row.evidence.clone(),
        })
        .collect())
}

/// The JSON of a palette bank, as the export writes it (`<slot>.json`).
pub fn palette_json(payload: &[u8]) -> crate::Result<Vec<u8>> {
    Ok(clut::Palette::decode(payload)?.json().into_bytes())
}

/// The 16 bpp view of a texture upload, as the export writes it (`<slot>.png`), from the payload and
/// its load argument.
pub fn texture_view_png(load_arg: u32, payload: &[u8]) -> crate::Result<Vec<u8>> {
    let image = vram::upload_image(payload, vram::Descriptor::from_load_argument(load_arg))?;
    Ok(png::rgba_png(image.width, image.height, &image.rgba))
}

/// The indexed PNG of a texture upload drawn with one palette row: `bits` is 4 or 8, and `palette` holds
/// the row's `2^bits` color words. This is the export of [`indexed_rows`] for a row given by its words.
pub fn texture_row_png(load_arg: u32, payload: &[u8], bits: u32, palette: &[u16]) -> crate::Result<Vec<u8>> {
    let depth = match bits {
        4 => Depth::Four,
        8 => Depth::Eight,
        other => return Err(crate::Error::Usage(format!("a texture is 4- or 8-bit indexed, not {other}-bit"))),
    };
    if palette.len() != depth.colors() {
        return Err(crate::Error::Usage(format!(
            "a {bits}-bit row holds {} color words, not {}",
            depth.colors(),
            palette.len()
        )));
    }
    let grid = vram::word_grid(payload, vram::Descriptor::from_load_argument(load_arg))?;
    let indices = texel_indices(&grid.words, depth);
    let colors: Vec<[u8; 4]> = palette.iter().map(|&word| color_rgba(word)).collect();
    let width = grid.width * depth.texels_per_word();
    Ok(png::indexed_png(width as u32, grid.height as u32, bits as u8, &colors, &indices))
}

/// One indexed PNG per palette row: the upload's texels at the row's depth, in the row's colors.
pub fn indexed_rows(load_arg: u32, payload: &[u8], rows: &[RowUse]) -> rebof3_emi::Result<Vec<RowFile>> {
    Ok(indexed_images(load_arg, payload, rows)?
        .into_iter()
        .map(|image| {
            let tag = format!(".clut-{}-{}", image.clut.x, image.clut.y);
            let tim = rows
                .iter()
                .find(|row| row.clut == image.clut && row.depth == image.depth)
                .and_then(|row| texture_tim(load_arg, payload, row).ok());
            RowFile {
                tag,
                data: png::indexed_png(
                    image.width as u32,
                    image.height as u32,
                    image.depth.bits() as u8,
                    &image.colors,
                    &image.indices,
                ),
                evidence: image.evidence,
                tim,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slow_attack_is_timed_by_the_library_steps() {
        // Attack shift 16, linear step 7: 4,681 steps of 32 samples reach full scale, about 3.40 s, which is
        // 1200 log2(3.40) = 2117 timecents.
        let envelope = volume_envelope(0x4000, 0).expect("the phases end");
        assert_eq!(envelope.attack, 2117);
        assert_eq!(envelope.sustain, 241, "the decay register's level 0 is 0x800, about 24 dB down");
    }
}
