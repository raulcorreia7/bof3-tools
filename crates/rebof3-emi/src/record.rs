//! Fixed-offset field readers for the game's table records, shared by the format modules.

/// The little-endian halfword at `at`.
pub fn u16_at(record: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes(record, at))
}

/// The `N` bytes at `at`.
pub fn bytes<const N: usize>(record: &[u8], at: usize) -> [u8; N] {
    record[at..at + N].try_into().expect("a fixed-size field")
}
