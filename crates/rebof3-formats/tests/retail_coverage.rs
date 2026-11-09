//! Whole-archive coverage gate: the scoreboard's totals are the pinned ones and
//! every format with a codec rebuilds its payloads byte-exactly.
//!
//! The totals come from the US `Breath of Fire III (USA)` archives (`wiki: formats/emi`). Like the
//! other archive gates this one skips when it cannot find the retail archives;
//! `support::retail_root` owns the lookup and honours `BOF3_DISC_DIR` (the archives are `BIN/` below it).

mod support;

use rebof3_formats::coverage;
use rebof3_formats::{Format, State};

/// Retail archive totals, pinned by the asset census.
const ENTRIES: usize = 6_344;
const BYTES: u64 = 250_503_535;
const ARCHIVES: usize = 880;

/// Entries and bytes the codecs own.
const CODEC_ENTRIES: usize = 4_955;
const CODEC_BYTES: u64 = 213_643_148;
/// Entries and bytes whose layout is documented without a codec.
const SPECIFIED_ENTRIES: usize = 788;
const SPECIFIED_BYTES: u64 = 21_400_121;
/// The remainder: no entry without an owner, the code images, the reservations.
const UNKNOWN_ENTRIES: usize = 0;
const UNKNOWN_BYTES: u64 = 0;
const CODE_ENTRIES: usize = 548;
const CODE_BYTES: u64 = 14_694_370;
const RESERVE_ENTRIES: usize = 53;
const RESERVE_BYTES: u64 = 765_896;

#[test]
fn retail_coverage_reaches_the_pinned_totals() {
    let Some(root) = support::retail_root() else {
        return;
    };

    let report = coverage::check(&root).expect("scoreboard over the retail archives");
    print!("{}", report.table());

    assert_eq!(report.entries(), ENTRIES, "entries in the retail archives");
    assert_eq!(report.bytes(), BYTES, "payload bytes in the retail archives");
    assert_eq!(report.container.archives, ARCHIVES, "archives in the retail archives");
    assert_eq!(report.container.entries, ENTRIES, "entries the container walk saw");
    assert_eq!(report.container.bytes, BYTES, "bytes the container walk saw");

    for (state, entries, bytes) in [
        (State::Codec, CODEC_ENTRIES, CODEC_BYTES),
        (State::Specified, SPECIFIED_ENTRIES, SPECIFIED_BYTES),
        (State::Unknown, UNKNOWN_ENTRIES, UNKNOWN_BYTES),
        (State::Code, CODE_ENTRIES, CODE_BYTES),
        (State::Reserve, RESERVE_ENTRIES, RESERVE_BYTES),
    ] {
        assert_eq!(report.state_entries(state), entries, "{} entries", state.label());
        assert_eq!(report.state_bytes(state), bytes, "{} bytes", state.label());
    }

    for format in Format::ALL {
        if format.state() != State::Codec {
            continue;
        }
        let row = report.row(format);
        assert!(row.entries > 0, "{}: no entries in the retail archives", format.name());
        assert_eq!(row.covered_entries, row.entries, "{}: entries the codec rebuilt", format.name());
        assert_eq!(row.covered_bytes, row.bytes, "{}: bytes the codec rebuilt", format.name());
    }

    let failures = report.all_failures();
    assert!(failures.is_empty(), "{} failures over the retail archives:\n{}", failures.len(), failures.join("\n"));
    eprintln!(
        "coverage gate: {} entries, {} bytes; {} codec-owned entries ({}) rebuild byte-exact",
        report.entries(),
        report.bytes(),
        report.covered_entries(),
        report.covered_bytes()
    );
}
