mod support;

use std::fs;

use rebof3_emi::{Archive, PackEntry, pack, pack_manifest};

#[test]
fn packs_and_roundtrips_entries_byte_exactly() {
    let root = support::out_dir("packs_and_roundtrips_entries_byte_exactly");
    let first = root.join("first.vh");
    let second = root.join("second.bin");
    fs::write(&first, b"ABCDx").unwrap();
    fs::write(&second, b"yz").unwrap();
    let output = root.join("roundtrip.emi");
    pack(&output, &[PackEntry { path: first, file_type: 6, ram_ptr: 0x8010_0000 }, PackEntry { path: second, file_type: 0, ram_ptr: 0 }], 1).unwrap();

    assert_eq!(fs::read(&output).unwrap(), support::fixture());
    let archive = Archive::open(output).unwrap();
    assert_eq!(archive.entries().len(), 2);
}

#[test]
fn repacks_an_extracted_manifest() {
    let root = support::out_dir("repacks_an_extracted_manifest");
    let source = root.join("source.emi");
    fs::write(&source, support::fixture()).unwrap();
    let archive = Archive::open(&source).unwrap();
    let extracted = root.join("extracted");
    archive.extract_all(&extracted, true).unwrap();
    let manifest = extracted.join("emi.json");
    fs::write(&manifest, archive.manifest_json(true)).unwrap();
    let repacked = root.join("repacked.emi");
    pack_manifest(&repacked, &manifest).unwrap();
    assert_eq!(fs::read(repacked).unwrap(), support::fixture());
}

#[test]
fn packs_empty_aligned_and_cross_sector_payloads() {
    let root = support::out_dir("packs_empty_aligned_and_cross_sector_payloads");
    let sizes = [0_usize, 0x800, 0x801];
    let entries: Vec<_> = sizes
        .iter()
        .enumerate()
        .map(|(index, size)| {
            let path = root.join(format!("{index}.bin"));
            fs::write(&path, vec![index as u8; *size]).unwrap();
            PackEntry { path, file_type: 0, ram_ptr: 0 }
        })
        .collect();
    let output = root.join("edges.EMI");
    pack(&output, &entries, 3).unwrap();
    let archive = Archive::open(&output).unwrap();
    assert_eq!(archive.version(), 3);
    assert_eq!(archive.entries().iter().map(|entry| entry.size).collect::<Vec<_>>(), [0, 0x800, 0x801]);
    for (index, size) in sizes.iter().enumerate() {
        let path = root.join(format!("out-{index}"));
        archive.extract_entry(index, &path).unwrap();
        assert_eq!(fs::metadata(path).unwrap().len(), *size as u64);
    }
}
