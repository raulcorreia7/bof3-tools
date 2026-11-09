//! The palette joins over the retail archives: 203 textures and 883 rows.
//!
//! The test skips when no retail archive directory is present. Set `BOF3_DISC_DIR` to an
//! extracted disc. The counts
//! are the world-map terrain rule plus the literal sites (`rebof3_emi::texture::SITES`).
//!
//! Every joined row is written to the test artifacts as its palette-applied PNG and its TIM, so
//! the result can be inspected by eye (`target/tmp/retail_textures_join_their_proven_palette_rows`).

mod support;

use std::fs;
use std::path::Path;

use psx_formats::gfx::texture::{Clut, Depth};
use rebof3_emi::Archive;
use rebof3_emi::texture::ResidentClutBlock;
use rebof3_formats::joins::joined_rows;
use rebof3_formats::vram::{indexed_rows, texture_view_png};

/// The proven palette joins: 203 textures and 883 rows.
const TEXTURES: usize = 203;
const ROWS: usize = 883;

/// The archive's payloads, or the slots that failed to read.
fn payloads(archive: &Archive, bytes: &[u8]) -> Vec<Vec<u8>> {
    (0..archive.entries().len()).filter_map(|slot| archive.payload(bytes, slot).ok().map(<[u8]>::to_vec)).collect()
}

/// Writes the texture view and one palette-applied PNG and TIM per joined row, plus the
/// evidence, for inspection.
fn write_artifacts(
    out: &Path,
    source: &str,
    archive: &Archive,
    payloads: &[Vec<u8>],
    rows: &std::collections::HashMap<usize, Vec<rebof3_formats::vram::RowUse>>,
    summary: &mut Vec<serde_json::Value>,
) {
    // The output folder is flat, so the archive's folder joins its file name.
    let stem = source.replace('/', "-");
    let mut slots: Vec<usize> = rows.keys().copied().collect();
    slots.sort_unstable();
    for slot in slots {
        let rows = &rows[&slot];
        let entry = &archive.entries()[slot];
        let payload = &payloads[slot];
        let view = texture_view_png(entry.ram_ptr, payload).expect("the texture view");
        support::write(out, &format!("{stem}-{slot}.view.png"), view);
        support::write(out, &format!("{stem}-{slot}.img"), payload);
        for row in rows {
            summary.push(serde_json::json!({
                "archive": source,
                "texture_slot": slot,
                "palette_slot": row.palette,
                "palette_archive": row.palette_archive,
                "clut_x": row.clut.x,
                "clut_y": row.clut.y,
                "depth": row.depth.bits(),
                "evidence": row.evidence,
            }));
        }
        let files = indexed_rows(entry.ram_ptr, payload, rows).expect("the palette-applied rows");
        for file in files {
            let name = format!("{stem}-{slot}{}", file.tag);
            support::write(out, &format!("{name}.png"), file.data);
            if let Some(tim) = file.tim {
                support::write(out, &format!("{name}.tim"), &tim);
            }
        }
    }
}

#[test]
fn retail_textures_join_their_proven_palette_rows() {
    let Some(root) = support::retail_root() else {
        return;
    };
    let mut archives = rebof3_formats::coverage::archives_under(&root);
    archives.sort();
    assert!(!archives.is_empty(), "no .EMI containers under {}", root.display());

    let out = support::out_dir("retail_textures_join_their_proven_palette_rows");
    let mut summary = Vec::new();
    let mut textures = 0;
    let mut rows = 0;
    let mut area024 = None;
    let mut magic218 = None;
    for path in &archives {
        let archive = match Archive::open(path) {
            Ok(archive) => archive,
            Err(error) => panic!("{}: {error}", path.display()),
        };
        let bytes = fs::read(path).expect("read the retail archive");
        let payloads = payloads(&archive, &bytes);
        assert_eq!(payloads.len(), archive.entries().len(), "{}: unreadable slot", path.display());
        let slices: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
        // The site table matches on the archive's disc path, and its evidence reads that way.
        let relative = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        let joined = joined_rows(&relative, archive.entries(), &slices);
        write_artifacts(&out, &relative, &archive, &payloads, &joined, &mut summary);
        textures += joined.len();
        rows += joined.values().map(Vec::len).sum::<usize>();
        match relative.as_str() {
            "WORLD00/AREA024.EMI" => area024 = Some(joined),
            "BMAGIC/MAGIC218.EMI" => magic218 = Some(joined),
            _ => {}
        }
    }
    let document = serde_json::json!({
        "textures": textures,
        "rows": rows,
        "resolved": summary,
    });
    support::write(&out, "joins.json", document.to_string());

    assert_eq!((textures, rows), (TEXTURES, ROWS), "textures with a proven palette row and their rows");

    let area024 = area024.expect("AREA024.EMI");
    assert_eq!(area024.len(), 1, "AREA024 joins one texture");
    let rows = area024.values().next().expect("the texture's rows");
    assert_eq!(rows.len(), 7, "six terrain rows and one literal row");
    let literal = rows.iter().find(|row| (row.clut.x, row.clut.y) == (160, 483)).expect("the literal site's row");
    assert_eq!(literal.palette, 8);
    assert_eq!(literal.depth.bits(), 4);
    assert!(literal.evidence.contains("sheet slot 7"), "{}", literal.evidence);
    assert!(rows.iter().any(|row| row.evidence.contains("terrain rule")), "the terrain rows");

    let magic218 = magic218.expect("MAGIC218.EMI");
    assert_eq!(magic218.len(), 1, "MAGIC218 joins one texture");
    let rows = magic218.values().next().expect("the texture's rows");
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].clut.x, rows[0].clut.y), (0, 506));
    assert_eq!(rows[0].palette, 5);
    assert_eq!(rows[0].depth.bits(), 8);
}

/// The battle family's cross-archive blocks: row 506 differs between `BATL_OVR.EMI` and
/// `BATL_DRA.EMI`, and row 507 agrees in four archives but differs in `BATL_DRA.EMI`. Either
/// row is ambiguous for a disc-wide resident set, so the join refuses it.
#[test]
fn retail_battle_rows_show_cross_archive_bank_conflicts() {
    let Some(root) = support::retail_root() else {
        return;
    };
    let names = [
        "BATTLE/BATTLE.EMI",
        "BATTLE/BATL_OVR.EMI",
        "BATTLE/BATL_DRA.EMI",
        "BATTLE/BATL_RE2.EMI",
        "BATTLE/BATL_RET.EMI",
        "BATTLE/BATTLE2.EMI",
    ];
    let mut blocks = Vec::new();
    for name in names {
        let path = root.join(name);
        if !path.is_file() {
            eprintln!("skipping the battle resident blocks: {name} is absent");
            return;
        }
        let archive = Archive::open(&path).expect("open the battle archive");
        let bytes = fs::read(&path).expect("read the battle archive");
        let payloads = payloads(&archive, &bytes);
        for (slot, entry) in archive.entries().iter().enumerate() {
            if let Some(block) = ResidentClutBlock::new(name, slot, entry.ram_ptr, &payloads[slot]) {
                blocks.push(block);
            }
        }
    }

    let row_506 = Clut::new(0, 506).expect("row 506");
    let row_507 = Clut::new(0, 507).expect("row 507");
    let holders = |clut| blocks.iter().filter(|block| block.holds(clut, Depth::Eight)).collect::<Vec<_>>();

    let holders_506 = holders(row_506);
    assert!(
        !holders_506.iter().any(|block| block.archive == "BATTLE/BATTLE.EMI"),
        "BATTLE.EMI has no row-506 block; the remaining archives must serve it"
    );
    assert!(
        holders_506.iter().map(|block| &block.values).collect::<std::collections::BTreeSet<_>>().len() > 1,
        "row 506 differs between BATL_OVR.EMI and BATL_DRA.EMI"
    );

    let holders_507 = holders(row_507);
    assert!(holders_507.len() > 1, "several archives carry row 507");
    let distinct_507: std::collections::BTreeSet<&Vec<u16>> = holders_507.iter().map(|block| &block.values).collect();
    assert_eq!(distinct_507.len(), 2, "one archive disagrees at row 507");
    let agreeing: Vec<_> =
        holders_507.iter().filter(|block| block.archive != "BATTLE/BATL_DRA.EMI").map(|block| &block.values).collect();
    assert!(agreeing.len() >= 4, "BATTLE, BATL_RE2, BATL_RET and BATTLE2 carry the same row 507");
    assert!(agreeing.iter().all(|values| *values == agreeing[0]), "the four named archives agree on row 507");
    assert!(
        holders_507.iter().any(|block| block.archive == "BATTLE/BATL_DRA.EMI" && &block.values != agreeing[0]),
        "BATL_DRA.EMI carries a different row 507, which makes the disc-wide resident set refuse it"
    );
}
