//! Whole-archive pack gate: extract, repack, and compare every retail archive.
//!
//! The test skips when it cannot find the retail archives; `support::retail_root`
//! owns the lookup and honours `BOF3_EMI_ROOT`. Two walks run: the on-disk one
//! (extract to files, write `emi.json`, `pack_manifest`) and the in-memory one
//! the `coverage` verb runs (`coverage::container_round_trip`).

mod support;

use std::fs;

use rebof3_emi::{Archive, coverage, pack_manifest};

#[test]
fn retail_repack_is_byte_identical() {
    let Some(root) = support::retail_root() else {
        eprintln!(
            "skipping repack gate: no retail archives below the checkout \
             (set BOF3_EMI_ROOT to a directory of .EMI containers)"
        );
        return;
    };

    let archives = coverage::archives_under(&root);
    assert!(!archives.is_empty(), "no .EMI containers under {}", root.display());
    let work = support::out_dir("retail_repack-retail_repack_is_byte_identical");
    let mut failures = Vec::new();
    for path in &archives {
        let archive = Archive::open(path).unwrap();
        let entries = work.join("entries");
        archive.extract_all(&entries, true).unwrap();
        fs::write(entries.join("emi.json"), archive.manifest_json(true)).unwrap();
        let repacked = work.join("repacked.EMI");
        pack_manifest(&repacked, entries.join("emi.json")).unwrap();
        if fs::read(path).unwrap() != fs::read(&repacked).unwrap() {
            failures.push(path.display().to_string());
        }
        fs::remove_dir_all(&entries).unwrap();
    }
    assert!(
        failures.is_empty(),
        "{} of {} archives do not repack byte-exactly:\n{}",
        failures.len(),
        archives.len(),
        failures.join("\n")
    );

    let container = coverage::container_round_trip(&root).expect("repack the retail archives");
    assert_eq!(container.archives, archives.len());
    assert!(
        container.failures.is_empty(),
        "{} in-memory failures:\n{}",
        container.failures.len(),
        container.failures.join("\n")
    );
    eprintln!("repack gate: {}/{} archives byte-identical", container.archives, container.archives);
}
