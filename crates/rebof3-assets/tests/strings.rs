//! The strings of the payloads that are not message banks. Every string on the owner's disc exports and imports to
//! its own bytes; the retail check skips when no disc image is present (set `BOF3_DISC_IMAGE`). The synthetic checks
//! cover the edit rules that the disc does not need in a payload of their own.

mod support;

use psx_cd::Image;
use rebof3_assets::text::strings;
use rebof3_emi::Archive;
use rebof3_emi::text::discover;

#[test]
fn every_string_payload_on_the_retail_disc_exports_and_imports_to_its_own_bytes() {
    let Some(cue) = support::retail_image() else {
        eprintln!("skipping the strings: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let mut image = Image::open(&cue).expect("the disc opens");
    let out = support::out_dir("every_string_payload_on_the_retail_disc_exports_and_imports_to_its_own_bytes");
    let (mut payloads, mut with_strings, mut found) = (0usize, 0usize, 0usize);
    for file in image.entries().expect("the file table reads") {
        let is_emi = file.path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("EMI"));
        if file.is_directory || file.is_xa || !is_emi {
            continue;
        }
        let container = image.read_file(&file).expect("the archive reads");
        let Ok(archive) = Archive::parse(&container) else { continue };
        for (index, entry) in archive.entries().iter().enumerate() {
            if !strings::holds_text(entry) {
                continue;
            }
            let payload = archive.payload(&container, index).expect("the payload is in the archive");
            if discover::bank_records(payload).is_some() {
                continue;
            }
            let base = strings::load_address(entry);
            let json = strings::export(payload, base)
                .unwrap_or_else(|error| panic!("{}#{index}: {error}", file.path.display()));
            let rebuilt = strings::import(&json, payload)
                .unwrap_or_else(|error| panic!("{}#{index}: {error}", file.path.display()));
            assert!(
                rebuilt == payload,
                "{}#{index} differs after its strings are exported and imported",
                file.path.display()
            );
            if payloads < 8 {
                support::write(&out, &format!("payload-{payloads}.json"), &json);
            }
            payloads += 1;
            let count = discover::strings(payload, base).len();
            if count > 0 {
                with_strings += 1;
                found += count;
            }
        }
    }
    assert!(with_strings > 0, "the disc has strings outside the message banks");
    eprintln!("{payloads} payloads round trip; {with_strings} hold {found} strings outside the message banks");
}
