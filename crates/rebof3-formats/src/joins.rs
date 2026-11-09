//! The join of a texture to the palette rows it is drawn with.

use std::collections::HashMap;

use rebof3_emi::texture as draw;

use crate::clut;
use crate::map::MapBundle;
use crate::registry::{Format, classify};
use crate::vram::RowUse;

/// The palette rows each texture of one archive is drawn with, keyed by texture slot.
pub fn joined_rows(source: &str, entries: &[rebof3_emi::Entry], payloads: &[&[u8]]) -> HashMap<usize, Vec<RowUse>> {
    joined_rows_in(source, entries, payloads, &[])
}

/// [`joined_rows`], with a resident set of CLUT blocks available as a fallback.
pub fn joined_rows_in(
    source: &str,
    entries: &[rebof3_emi::Entry],
    payloads: &[&[u8]],
    resident: &[draw::ResidentClutBlock],
) -> HashMap<usize, Vec<RowUse>> {
    let mut sheets = Vec::new();
    let mut own = Vec::new();
    let mut bundles = Vec::new();
    for (slot, (entry, payload)) in entries.iter().zip(payloads).enumerate() {
        match classify(entry, payload) {
            Format::VramUpload => sheets.extend(draw::sheet(slot, entry.ram_ptr, payload.len())),
            Format::ClutBlock => own.extend(draw::ResidentClutBlock::new(source, slot, entry.ram_ptr, payload)),
            Format::AreaMapBundle => {
                if let Ok(bundle) = MapBundle::decode(payload) {
                    bundles.push(bundle);
                }
            }
            _ => {}
        }
    }
    let mut blocks: Vec<&draw::ResidentClutBlock> = own.iter().collect();
    blocks.extend(resident.iter().filter(|block| block.archive != source));

    let mut rows: HashMap<usize, Vec<RowUse>> = HashMap::new();
    for join in draw::joins(source, &sheets, &blocks) {
        push_row(&mut rows, source, join);
    }
    for bundle in &bundles {
        for join in draw::terrain_joins(source, &bundle.records, &sheets, &blocks) {
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
        evidence.push_str(&format!("; CLUT block {}#{}", join.palette.archive, join.palette.block.slot));
    }
    rows.entry(texture).or_default().push(RowUse {
        palette: join.palette.block.slot,
        palette_archive: join.palette.archive.clone(),
        clut: join.clut,
        depth: join.depth,
        colors,
        words: words.to_vec(),
        evidence,
    });
}
