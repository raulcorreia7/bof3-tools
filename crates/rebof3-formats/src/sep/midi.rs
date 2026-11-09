//! A sequence package as Standard MIDI Files, and MIDI files back to a package.

use crate::error::Result;

/// Sequence `index` of a sequence package (`.seq`) as a format 0 Standard MIDI File.
pub fn export(package: &[u8], index: usize) -> Result<Vec<u8>> {
    Ok(psx_formats::sep::Package::parse(package)?.to_midi(index)?)
}

/// A sequence package (`.seq`) from Standard MIDI Files, one sequence per part and in order. Each part is a MIDI
/// file that [`export`] writes, with the sequence id the package gives it.
pub fn import(parts: &[(&[u8], u16)]) -> Result<Vec<u8>> {
    let sequences = parts
        .iter()
        .map(|&(midi, seq_id)| psx_formats::sep::Sequence::from_midi(midi, seq_id))
        .collect::<psx_formats::Result<Vec<_>>>()?;
    Ok(psx_formats::sep::Package { sequences }.encode())
}
