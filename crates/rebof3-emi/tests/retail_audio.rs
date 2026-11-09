//! Whole-archive audio gate: every VAB header/body pair and every sequence package parses, breaks none of the format, and encodes back to its bytes. Counts pinned here are `wiki: formats/audio`'s: 1,020 headers (4,904,832 B), 1,020 bodies (79,157,504 B) and 119 packages (1,836,303 B). Like `tests/retail_repack.rs`, the gate skips when it cannot find the retail archives; `support::retail_root` owns the lookup and honours `BOF3_DISC_DIR` (the archives are `BIN/` below it).

mod support;

use std::path::Path;

use psx_formats::vab::Problem;
use psx_formats::{sep, vab};
use rebof3_emi::{Archive, Entry, read_entry};

const HEADERS: usize = 1_020;
const HEADER_BYTES: u64 = 4_904_832;
const BODIES: usize = 1_020;
const BODY_BYTES: u64 = 79_157_504;
const PACKAGES: usize = 119;
const PACKAGE_BYTES: u64 = 1_836_303;

/// The retail archives' one dangling tone reference: `BPLCHAR/DRG04_00#0` declares ten tones over a two-VAG size table, and its third block's tone names VAG 3, whose size-table unit is zero.
const DANGLING_TONES: &[(&str, usize)] = &[("DRG04_00.EMI", 0)];

/// Where the entry with the named exception sits in [`DANGLING_TONES`], if the container and index are the pair the retail archives record.
fn explained(container: &Path, index: usize) -> Option<usize> {
    DANGLING_TONES.iter().position(|(name, at)| *at == index && container.ends_with(name))
}

#[test]
fn retail_audio_round_trips_byte_exactly() {
    let Some(root) = support::retail_root() else {
        return;
    };

    let mut archives = Vec::new();
    support::collect_archives(&root, &mut archives);
    archives.sort();
    assert!(!archives.is_empty(), "archives at {} holds no .EMI container", root.display());

    let mut failures: Vec<String> = Vec::new();
    let mut explained_seen = vec![false; DANGLING_TONES.len()];
    let mut counted = Totals::default();
    let mut sample: Option<(std::path::PathBuf, usize, usize)> = None;

    for path in &archives {
        let archive = Archive::open(path).expect("open retail archive");
        for (index, entry) in archive.entries().iter().enumerate() {
            let where_ = format!("{}#{index}", path.display());
            match entry.file_type {
                vab::HEADER_TYPE => {
                    counted.headers += 1;
                    counted.header_bytes += u64::from(entry.size);
                    if sample.is_none()
                        && let Ok(body) = rebof3_emi::entry::vab_body_index(archive.entries(), index)
                    {
                        sample = Some((path.clone(), index, body));
                    }
                    match check_bank(path, archive.entries(), index, &mut counted) {
                        Ok(problems) if problems.is_empty() => {}
                        Ok(problems) if problems.iter().all(is_dangling_tone) => match explained(path, index) {
                            Some(slot) => explained_seen[slot] = true,
                            None => failures.push(format!("{where_}: {}", render(&problems))),
                        },
                        Ok(problems) => failures.push(format!("{where_}: {}", render(&problems))),
                        Err(problem) => failures.push(format!("{where_}: {problem}")),
                    }
                }
                vab::BODY_TYPE => {
                    counted.bodies += 1;
                    counted.body_bytes += u64::from(entry.size);
                }
                sep::TYPE => {
                    counted.packages += 1;
                    counted.package_bytes += u64::from(entry.size);
                    match check_package(path, index, &mut counted) {
                        Ok(problems) if problems.is_empty() => {}
                        Ok(problems) => failures.push(format!("{where_}: {}", render(&problems))),
                        Err(problem) => failures.push(format!("{where_}: {problem}")),
                    }
                }
                _ => {}
            }
        }
    }

    assert_eq!(counted.headers, HEADERS, "VAB headers in the retail archives");
    assert_eq!(counted.header_bytes, HEADER_BYTES, "VAB header bytes");
    assert_eq!(counted.bodies, BODIES, "VAB bodies in the retail archives");
    assert_eq!(counted.body_bytes, BODY_BYTES, "VAB body bytes");
    assert_eq!(counted.packages, PACKAGES, "sequence packages in the retail archives");
    assert_eq!(counted.package_bytes, PACKAGE_BYTES, "package bytes");

    assert!(
        explained_seen.iter().all(|seen| *seen),
        "the retail archives no longer holds every explained dangling tone reference"
    );
    assert!(failures.is_empty(), "{} audio entries failed:\n{}", failures.len(), failures.join("\n"));

    let out = support::out_dir("retail_audio_round_trips_byte_exactly");
    support::write(
        &out,
        "totals.json",
        format!(
            "{{\"headers\":{},\"header_bytes\":{},\"bodies\":{},\"body_bytes\":{},\"packages\":{},\"package_bytes\":{},\"tones\":{},\"events\":{}}}",
            counted.headers,
            counted.header_bytes,
            counted.bodies,
            counted.body_bytes,
            counted.packages,
            counted.package_bytes,
            counted.tones,
            counted.events
        ),
    );
    if let Some((path, header, body)) = sample {
        support::write(&out, "sample-header.bin", read_entry(&path, header).expect("the sample header"));
        support::write(&out, "sample-body.bin", read_entry(&path, body).expect("the sample body"));
    }
    eprintln!(
        "audio gate: {} headers ({} B), {} bodies ({} B), {} packages ({} B); {} tones, {} \
         events, {} bytes round-trip; {} explained exception(s)",
        counted.headers,
        counted.header_bytes,
        counted.bodies,
        counted.body_bytes,
        counted.packages,
        counted.package_bytes,
        counted.tones,
        counted.events,
        counted.header_bytes + counted.body_bytes + counted.package_bytes,
        explained_seen.len(),
    );
}

#[derive(Default)]
struct Totals {
    headers: usize,
    header_bytes: u64,
    bodies: usize,
    body_bytes: u64,
    packages: usize,
    package_bytes: u64,
    tones: usize,
    events: usize,
}

/// Parses one header with its body, checks the format rules and encodes both back: the header's own bytes must return, and the body the header was paired with must be the range its size table describes.
fn check_bank(path: &Path, entries: &[Entry], index: usize, counted: &mut Totals) -> Result<Vec<Problem>, String> {
    let header = read_entry(path, index).map_err(|error| error.to_string())?;
    let body_index = rebof3_emi::entry::vab_body_index(entries, index).map_err(|error| error.to_string())?;
    let body = read_entry(path, body_index).map_err(|error| error.to_string())?;

    let bank = vab::Bank::parse(&header, &body).map_err(|error| error.to_string())?;
    if bank.encode_header() != header {
        return Err("the encoded header is not the header".into());
    }
    if bank.body_len() != body.len() {
        return Err("the parsed body is not the body entry's length".into());
    }

    counted.tones += bank.tone_count as usize;
    Ok(bank.problems())
}

/// Parses one package and encodes it back; the event stream must tile the payload its header declares, so the bytes are the input again.
fn check_package(path: &Path, index: usize, counted: &mut Totals) -> Result<Vec<sep::Problem>, String> {
    let payload = read_entry(path, index).map_err(|error| error.to_string())?;
    let package = rebof3_emi::entry::sep(path, index).map_err(|error| error.to_string())?;
    if package.encode() != payload {
        return Err("the encoded package is not the payload".into());
    }

    counted.events += package.sequences.iter().map(|sequence| sequence.events.len()).sum::<usize>();
    Ok(package.problems())
}

/// A problem of the one shape the retail archives leave: a tone naming a VAG the size table does not cover.
fn is_dangling_tone(problem: &Problem) -> bool {
    matches!(problem, Problem::ToneVag { .. })
}

fn render<T: ToString>(problems: &[T]) -> String {
    problems.iter().map(std::string::ToString::to_string).collect::<Vec<String>>().join("; ")
}
