//! Whole-archive map gate: every decodable map bundle re-encodes byte-exactly and
//! reports the exact encoded length.
//!
//! Regression for review-data F2: the four variable record-region word counts
//! were added without the `TILE_BYTES` scale, so `encoded_len` under-reported
//! 196 of the 200 retail maps before the fix.

mod support;

use std::fs;

use rebof3_formats::map::MapBundle;

use support::{collect_archives, retail_root};

#[test]
fn map_bundles_reencode_exactly_and_report_their_size() {
    let Some(root) = retail_root() else {
        eprintln!(
            "skipping map gate: no retail EMI archives at <ancestor>/raw/disc/BIN \
             (set BOF3_EMI_ROOT to a directory of .EMI containers)"
        );
        return;
    };
    let mut archives = Vec::new();
    collect_archives(&root, &mut archives);
    archives.sort();

    let out = support::out_dir("map_bundles_reencode_exactly_and_report_their_size");
    let mut checked = 0_usize;
    for path in archives {
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(archive) = rebof3_emi::Archive::open(&path) else {
            continue;
        };
        for entry in archive.entries() {
            let start = entry.offset as usize;
            let end = start + entry.size as usize;
            let Some(payload) = bytes.get(start..end) else {
                continue;
            };
            let Ok(bundle) = MapBundle::decode(payload) else {
                continue;
            };
            let encoded = bundle.encode();
            assert_eq!(
                bundle.encoded_len(),
                encoded.len(),
                "encoded_len mismatch: {} entry at {start:#x}",
                path.display()
            );
            assert_eq!(encoded, payload, "re-encode mismatch: {} entry at {start:#x}", path.display());
            if checked == 0 {
                support::write(&out, "first-map-bundle.map", &encoded);
            }
            checked += 1;
        }
    }
    assert!(checked >= 200, "expected at least 200 map bundles under {}, found {checked}", root.display());
    eprintln!("checked {checked} map bundles under {}", root.display());
}
