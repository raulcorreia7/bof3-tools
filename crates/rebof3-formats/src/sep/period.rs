//! The period sequence files: SEP (a package of sequences) and SEQ (one sequence), as Sony's *File
//! Formats* manual, chapter 4, lays them out. A studio exchanged both between its sound tools.
//!
//! The game stores a package as a SEP: `pQES`, a 16-bit version, then per sequence a 13-byte header
//! (id, resolution, tempo, rhythm, data size) and the score data. A SEQ file is one such sequence with
//! its own header: `pQES`, a 32-bit version, the resolution, the tempo, the rhythm and the score data
//! to the end of the file. It has no id and no data size, so the id belongs to the package.
//!
//! Generic codecs for both belong in `psx-formats`; this module holds them until that crate has them.

use psx_formats::sep::{Package, Sequence};

use crate::error::{Error, Result};

/// The `pQES` magic both files open with, in file order.
const MAGIC: &[u8; 4] = b"pQES";
/// The version a SEQ file declares (the manual gives the field as four bytes and no value; Sony's
/// sequencer files carry 1).
const SEQ_VERSION: u32 = 1;
/// A SEQ file's header: magic, version, resolution, tempo and rhythm.
const SEQ_HEADER: usize = 15;
/// A package's header before the first sequence: magic and version.
const SEP_HEADER: usize = 6;
/// A sequence header inside a package.
const SEQUENCE_HEADER: usize = 13;
/// The bytes of a sequence header that a SEQ file shares: resolution, tempo and rhythm.
const SHARED: std::ops::Range<usize> = 2..9;

/// The package as a SEP file. The stored package is that file, so the bytes come back unchanged once
/// they parse as a package.
pub fn export_sep(package: &[u8]) -> Result<Vec<u8>> {
    let parsed = Package::parse(package)?;
    check(&parsed)?;
    Ok(parsed.encode())
}

/// A SEP file as the stored package. The file must parse and carry no rule the format breaks.
pub fn import_sep(file: &[u8]) -> Result<Vec<u8>> {
    export_sep(file)
}

/// Sequence `index` of a package as a SEQ file.
pub fn export_seq(package: &[u8], index: usize) -> Result<Vec<u8>> {
    let parsed = Package::parse(package)?;
    let count = parsed.sequences.len();
    let sequence = parsed.sequences.get(index).ok_or(psx_formats::Error::SepSequenceOutOfRange { index, count })?;
    let single = Package { sequences: vec![sequence.clone()] }.encode();
    let header = &single[SEP_HEADER..SEP_HEADER + SEQUENCE_HEADER];
    let mut file = Vec::with_capacity(single.len());
    file.extend_from_slice(MAGIC);
    file.extend_from_slice(&SEQ_VERSION.to_be_bytes());
    file.extend_from_slice(&header[SHARED]);
    file.extend_from_slice(&single[SEP_HEADER + SEQUENCE_HEADER..]);
    Ok(file)
}

/// One sequence of a SEQ file, with the id the package gives it.
pub fn read_seq(file: &[u8], seq_id: u16) -> Result<Sequence> {
    if file.len() < SEQ_HEADER || &file[..4] != MAGIC {
        return Err(Error::Import("not a SEQ file: it does not open with the pQES magic and a header".into()));
    }
    let score = &file[SEQ_HEADER..];
    let mut single = Vec::with_capacity(SEP_HEADER + SEQUENCE_HEADER + score.len());
    single.extend_from_slice(MAGIC);
    single.extend_from_slice(&0u16.to_be_bytes());
    single.extend_from_slice(&seq_id.to_be_bytes());
    single.extend_from_slice(&file[8..15]);
    single.extend_from_slice(&(score.len() as u32).to_be_bytes());
    single.extend_from_slice(score);
    let mut package = Package::parse(&single)?;
    check(&package)?;
    package.sequences.pop().ok_or_else(|| Error::Import("the SEQ file holds no sequence".into()))
}

/// A package from SEQ files, one sequence per part and in order, each with the id the package gives it.
pub fn import_seq(parts: &[(&[u8], u16)]) -> Result<Vec<u8>> {
    let sequences = parts.iter().map(|&(file, seq_id)| read_seq(file, seq_id)).collect::<Result<Vec<_>>>()?;
    Ok(Package { sequences }.encode())
}

/// `Err` with the first rule the package breaks beyond its layout.
fn check(package: &Package) -> Result<()> {
    match package.problems().first() {
        Some(problem) => Err(Error::Import(format!("the sequence data breaks the format: {problem:?}"))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use psx_formats::sep::Event;

    use super::*;

    fn note(delta: u32, status: u8) -> Event {
        Event { delta, status, data1: 60, data2: 100, status_byte: true, ..Event::default() }
    }

    fn end() -> Event {
        Event { status: 0xff, meta_type: 0x2f, status_byte: true, ..Event::default() }
    }

    fn sequence(seq_id: u16) -> Sequence {
        Sequence {
            seq_id,
            resolution: 48,
            tempo_us: 500_000,
            time_num: 4,
            time_den: 2,
            events: vec![note(0, 0x90), note(24, 0x80), end()],
        }
    }

    fn package() -> Vec<u8> {
        Package { sequences: vec![sequence(0), sequence(7)] }.encode()
    }

    #[test]
    fn a_package_is_its_own_sep_file() {
        assert_eq!(export_sep(&package()).unwrap(), package());
        assert_eq!(import_sep(&package()).unwrap(), package());
    }

    #[test]
    fn a_sequence_is_a_seq_file_with_the_manual_layout() {
        let file = export_seq(&package(), 1).unwrap();
        assert_eq!(&file[..4], b"pQES");
        assert_eq!(&file[4..8], &[0, 0, 0, 1], "version, 4 bytes");
        assert_eq!(&file[8..10], &48u16.to_be_bytes(), "resolution of a quarter note");
        assert_eq!(&file[10..13], &[0x07, 0xa1, 0x20], "tempo, 3 bytes");
        assert_eq!(&file[13..15], &[4, 2], "rhythm");
        assert_eq!(&file[file.len() - 3..], &[0xff, 0x2f, 0x00], "end of SEQ");
    }

    #[test]
    fn seq_files_rebuild_the_package_with_their_ids() {
        let package = package();
        let first = export_seq(&package, 0).unwrap();
        let second = export_seq(&package, 1).unwrap();
        assert_eq!(import_seq(&[(&first, 0), (&second, 7)]).unwrap(), package);
        assert!(export_seq(&package, 2).is_err());
    }

    #[test]
    fn a_file_that_is_not_a_seq_is_refused() {
        assert!(read_seq(b"MThd", 0).is_err());
        let mut broken = export_seq(&package(), 0).unwrap();
        broken.truncate(broken.len() - 1);
        assert!(read_seq(&broken, 0).is_err(), "the end of SEQ is cut");
    }
}
