//! The LZSS codec of the transfer queue's packed images.
//!
//! `GFX_UpdateTransferQueue` (`exe/slus_004_22@0x8014E6D0`) hands an image record of kind 2 to
//! `GFX_DecodeLzss` (`exe/slus_004_22@0x8014E8FC`), which unpacks the stream into the shared image
//! buffer before its `LoadImage`. The stream is a classic LZSS:
//!
//! - A flag byte gives the kind of the next eight tokens, low bit first: 1 is a literal byte, 0 a
//!   two-byte back-reference.
//! - A back-reference is `pos_low, (pos_high << 4) | length_code`: it copies `length_code + 3` bytes
//!   from ring position `pos`, one byte at a time, so a copy may read bytes it has just written.
//! - Every output byte also goes into a [`RING`]-byte ring that starts at [`RING_START`]. The decoder
//!   zeroes ring bytes `0..RING_START`; the rest hold whatever the scratchpad held, so a stream that
//!   reads them before writing them has no defined result and [`decode`] refuses it.
//!
//! The decoder stops once it has written the image's byte count. [`encode`] writes a stream that
//! decoder reads back to the same bytes; it is not the retail packer, so a stored stream is kept as
//! stored and only an edited image is packed again.

use crate::{Error, Result};

/// Bytes in the ring buffer: positions are masked with `RING - 1` (`& 0x1FF`).
pub const RING: usize = 0x200;
/// The ring position of the first output byte (`r = 0x1EE`).
pub const RING_START: usize = 0x1EE;
/// Tokens one flag byte describes.
const TOKENS_PER_FLAG: u32 = 8;
/// The flag bits after a fresh flag byte: the byte in the low half, a marker in the high half that
/// tells when all eight have been shifted out (`flags = c | 0xFF00`, tested with `& 0x100`).
const FLAG_MARKER: u16 = 0xFF00;
const FLAG_SPENT: u16 = 0x100;
/// The bits of a back-reference's second byte: the high bits of the position and the length code.
const POSITION_HIGH_MASK: u8 = 0xF0;
const POSITION_HIGH_SHIFT: u32 = 4;
const LENGTH_MASK: u8 = 0x0F;
/// Bytes a back-reference copies: its length code plus this (`(l & 0xF) + 2`, copied `<= len`).
pub const MIN_MATCH: usize = 3;
/// The longest back-reference.
pub const MAX_MATCH: usize = MIN_MATCH + LENGTH_MASK as usize;
/// The high bits of a position the encoder writes; the decoder masks positions to the ring.
const POSITION_LOW_MASK: usize = 0xFF;
const POSITION_HIGH_BITS: usize = 8;

/// The ring as both sides see it: its bytes, and whether each one is defined yet.
struct Ring {
    bytes: [u8; RING],
    defined: [bool; RING],
    at: usize,
}

impl Ring {
    /// The ring `GFX_DecodeLzss` starts from: bytes before [`RING_START`] are zero, the rest undefined.
    fn new() -> Self {
        let mut defined = [false; RING];
        defined[..RING_START].fill(true);
        Self { bytes: [0; RING], defined, at: RING_START }
    }

    fn push(&mut self, byte: u8) {
        self.bytes[self.at] = byte;
        self.defined[self.at] = true;
        self.at = (self.at + 1) % RING;
    }
}

/// The `count` bytes the stream decodes to, and the stream bytes it read.
///
/// The stream must define every ring byte it reads and must not end before `count` bytes. A
/// back-reference that runs past `count` is cut there, as the upload reads only `count` bytes.
pub fn decode(stream: &[u8], count: usize) -> Result<(Vec<u8>, usize)> {
    let truncated = || Error::Lzss(format!("the stream ends before its {count} bytes"));
    let mut ring = Ring::new();
    let mut out = Vec::with_capacity(count);
    let mut at = 0;
    let mut flags: u16 = 0;

    while out.len() < count {
        flags >>= 1;
        if flags & FLAG_SPENT == 0 {
            flags = u16::from(*stream.get(at).ok_or_else(truncated)?) | FLAG_MARKER;
            at += 1;
        }

        if flags & 1 != 0 {
            let byte = *stream.get(at).ok_or_else(truncated)?;
            at += 1;
            out.push(byte);
            ring.push(byte);
            continue;
        }

        let [low, high] = [*stream.get(at).ok_or_else(truncated)?, *stream.get(at + 1).ok_or_else(truncated)?];
        at += 2;
        let position = usize::from(low) | (usize::from(high & POSITION_HIGH_MASK) << POSITION_HIGH_SHIFT);
        let length = usize::from(high & LENGTH_MASK) + MIN_MATCH;
        for step in 0..length {
            let from = (position + step) % RING;
            if !ring.defined[from] {
                return Err(Error::Lzss(format!("a back-reference reads ring byte {from:#X} before the stream writes it")));
            }
            let byte = ring.bytes[from];
            ring.push(byte);
            if out.len() < count {
                out.push(byte);
            }
        }
    }
    Ok((out, at))
}

/// A stream [`decode`] reads back to `bytes`: greedy longest matches over the ring.
pub fn encode(bytes: &[u8]) -> Vec<u8> {
    let mut ring = Ring::new();
    let mut out = Vec::new();
    let mut flag_at = 0;
    let mut tokens = TOKENS_PER_FLAG;
    let mut at = 0;

    while at < bytes.len() {
        if tokens == TOKENS_PER_FLAG {
            flag_at = out.len();
            out.push(0);
            tokens = 0;
        }

        let (position, length) = longest_match(&ring, &bytes[at..]);
        if length >= MIN_MATCH {
            out.push((position & POSITION_LOW_MASK) as u8);
            out.push((((position >> POSITION_HIGH_BITS) << POSITION_HIGH_SHIFT) as u8) | (length - MIN_MATCH) as u8);
            for &byte in &bytes[at..at + length] {
                ring.push(byte);
            }
            at += length;
        } else {
            out[flag_at] |= 1 << tokens;
            out.push(bytes[at]);
            ring.push(bytes[at]);
            at += 1;
        }
        tokens += 1;
    }
    out
}

/// The ring position and length of the longest copy that writes the start of `ahead`.
fn longest_match(ring: &Ring, ahead: &[u8]) -> (usize, usize) {
    let limit = ahead.len().min(MAX_MATCH);
    let mut best = (0, 0);
    for position in 0..RING {
        let mut length = 0;
        while length < limit {
            let from = (position + length) % RING;
            // A copy reads what it has written itself once it reaches the write position.
            let written = (from + RING - ring.at) % RING;
            let byte = if written < length {
                ahead[written]
            } else if ring.defined[from] {
                ring.bytes[from]
            } else {
                break;
            };
            if byte != ahead[length] {
                break;
            }
            length += 1;
        }
        if length > best.1 {
            best = (position, length);
            if length == limit {
                break;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes with runs, repeats and noise, the shapes an image row has.
    fn sample() -> Vec<u8> {
        let mut bytes = vec![0u8; 300];
        bytes.extend((0..200u32).map(|index| (index * 7 % 31) as u8));
        bytes.extend([5, 6, 7].repeat(40));
        bytes.extend((0..64u32).map(|index| (index.wrapping_mul(2_654_435_761) >> 24) as u8));
        bytes
    }

    #[test]
    fn encode_then_decode_gives_the_bytes_back() {
        let bytes = sample();
        let stream = encode(&bytes);
        let (decoded, used) = decode(&stream, bytes.len()).expect("the stream decodes");
        assert_eq!(decoded, bytes);
        assert_eq!(used, stream.len());
        assert!(stream.len() < bytes.len(), "{} bytes pack to {}", bytes.len(), stream.len());
    }

    #[test]
    fn a_copy_reads_the_bytes_it_writes() {
        // A literal, then one back-reference that repeats it from the byte it has just written.
        let stream = [0b0000_0001, 9, RING_START as u8, ((RING_START >> 8) << 4) as u8 | 5];
        let (decoded, _) = decode(&stream, 9).expect("decodes");
        assert_eq!(decoded, [9; 9]);
    }

    #[test]
    fn a_read_of_an_unwritten_ring_byte_is_refused() {
        // A back-reference to the last ring byte, which the decoder never zeroes.
        let stream = [0, 0xFF, (1 << 4) as u8];
        assert!(decode(&stream, MIN_MATCH).is_err());
    }

    #[test]
    fn the_zeroed_ring_packs_a_run_of_zeros_from_the_start() {
        let stream = encode(&[0; 18]);
        assert_eq!(stream.len(), 3, "one flag byte and one back-reference into the zeroed ring");
        assert_eq!(decode(&stream, 18).expect("decodes").0, [0; 18]);
    }

    #[test]
    fn a_short_stream_is_an_error() {
        assert!(decode(&[0xFF, 1, 2], 4).is_err());
    }
}
