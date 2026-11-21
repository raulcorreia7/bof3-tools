//! Fixed-offset field reader for the game's table records.

/// The `N` bytes at `at`.
///
/// # Panics
///
/// Panics when the record ends before `at + N`; callers slice whole records first.
pub fn bytes<const N: usize>(record: &[u8], at: usize) -> [u8; N] {
    record[at..at + N].try_into().expect("a fixed-size field")
}
