//! The join of a texture to the palette rows it is drawn with.
//!
//! A join reads a whole archive: the texture uploads, the palette banks and the map bundles beside
//! them. It is not a format; it composes the `vram`, `clut` and `map` modules through the registry,
//! and no format module imports it.

use std::collections::HashMap;

use rebof3_emi::texture as draw;

use crate::clut;
use crate::map::MapBundle;
use crate::registry::{Format, classify};
use crate::vram::RowUse;

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
    let colors = words.iter().map(|&word| clut::color_rgba(word)).collect();
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
