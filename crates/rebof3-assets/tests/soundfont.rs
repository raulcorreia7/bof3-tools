//! SoundFont 2 back to a VAB bank: a synthetic bank, and every bank of the owner's disc.
//!
//! The ADPCM of a sample is lossy, so the first import is not the stored bank. The gate is the second round trip:
//! importing what an export of an imported bank gives reproduces that bank byte for byte, and its SoundFont
//! holds the same zones and frames. The disc test skips when no disc image is present. Point `BOF3_DISC_IMAGE`
//! at its cue sheet (`BOF3_REQUIRE_RETAIL=1` makes a skip a failure).

mod support;

use std::fs;

use psx_formats::sf2::{self, SoundFont};
use rebof3_assets::extract::{self, Options};
use rebof3_assets::kind::{Kind, Selection};
use rebof3_assets::report::Row;
use rebof3_formats::vab::soundfont;

/// The SoundFont with its sample names cleared. A name is `VAG` and the bank's VAG number, which the import
/// numbers in first-use order, so names are compared apart from the rest.
fn without_names(mut font: SoundFont) -> SoundFont {
    for sample in &mut font.samples {
        sample.name.clear();
    }
    font
}

#[test]
fn retail_banks_take_a_second_round_trip_to_the_same_bank() {
    let Some(image) = support::retail_image() else {
        return;
    };
    let dir = support::scratch_dir("soundfont-retail_banks_take_a_second_round_trip_to_the_same_bank");
    let options = Options {
        image,
        out: dir.to_path_buf(),
        selection: Selection::parse("audio-bank").unwrap(),
        write: true,
        jobs: 2,
    };
    let report = extract::run(&options).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let is_bank = |row: &&Row| row.role == "raw" && row.kind == Kind::AudioBank;
    let (mut banks, mut sounds) = (0, 0);
    for header_row in report.rows.iter().filter(is_bank).filter(|row| row.file_type == Some(6)) {
        let body_row = report
            .rows
            .iter()
            .filter(is_bank)
            .find(|row| {
                row.file_type == Some(7) && row.source == header_row.source && row.load_arg == header_row.load_arg
            })
            .expect("every VAB header has its body");
        let header = fs::read(dir.join(header_row.output.as_deref().unwrap())).unwrap();
        let body = fs::read(dir.join(body_row.output.as_deref().unwrap())).unwrap();
        let name = header_row.output.clone().unwrap_or_default();

        let sf2_first = soundfont::export(&header, &body, &name).unwrap();
        let (header_first, body_first) = soundfont::import(&sf2_first).unwrap();
        let sf2_second = soundfont::export(&header_first, &body_first, &name).unwrap();
        let (header_second, body_second) = soundfont::import(&sf2_second).unwrap();
        assert!(
            (&header_second, &body_second) == (&header_first, &body_first),
            "{name}: the second import is not the first"
        );
        assert_eq!(
            without_names(sf2::read(&sf2_second).unwrap()),
            without_names(sf2::read(&sf2_first).unwrap()),
            "{name}: the SoundFont changed on the second round trip"
        );
        banks += 1;
        sounds += sf2::read(&sf2_first).unwrap().samples.len();
    }
    eprintln!("{banks} banks take a second round trip to the same bank; {sounds} samples in them");
    assert!(banks > 0);
}
