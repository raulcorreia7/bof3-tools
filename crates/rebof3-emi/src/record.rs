//! Fixed-offset field readers for the game's table records, shared by the format modules.

/// The little-endian halfword at `at`.
///
/// # Panics
///
/// Panics when the record ends before `at + 2`; callers slice whole records first.
pub fn u16_at(record: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes(record, at))
}

/// The `N` bytes at `at`.
///
/// # Panics
///
/// Panics when the record ends before `at + N`; callers slice whole records first.
pub fn bytes<const N: usize>(record: &[u8], at: usize) -> [u8; N] {
    record[at..at + N].try_into().expect("a fixed-size field")
}
