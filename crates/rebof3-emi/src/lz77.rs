//! Type-1 LZ77 payload codec.
//!
//! A type-1 EMI entry is `[u32 LE decoded size][u16 0xFFFF][control stream]`.
//! The control stream is the codec the game decompresses in
//! `src/engine/slus/LZ77_DecodeStep.c`; a frame is either a literal run or a
//! back-reference into the last [`WINDOW`] decoded bytes:
//!
//! - `ctrl & 0x10` set: literal run of `0x20 - (ctrl & 0x1F)` bytes; a count of
//!   16 adds one length byte; `0xFF` ends the stream.
//! - otherwise: distance `0x800 - (((ctrl & 0xE0) << 3) | byte)`, length
//!   `(ctrl & 0x0F) + 3`, extended by one byte when the low nibble is 15.

use std::collections::HashMap;

use crate::error::{Error, Result};

/// Distance origin used by the back-reference code.
pub const WINDOW: usize = 0x800;
/// Maximum candidate distance considered by the encoder.
const MAX_SEARCH_DISTANCE: usize = WINDOW - 1;
const DECODED_SIZE_SIZE: usize = 4;
const HEADER_MARKER_SIZE: usize = 2;
/// Payload header: `[u32 decoded size][u16 0xFFFF]`.
pub const HEADER: usize = DECODED_SIZE_SIZE + HEADER_MARKER_SIZE;
const HEADER_MARKER: u16 = 0xffff;
const END_MARKER: u8 = 0xff;
const LITERAL_CONTROL_FLAG: u8 = 0x10;
const LITERAL_COUNT_MASK: u8 = 0x1f;
const LITERAL_COUNT_BASE: usize = 0x20;
const LITERAL_EXTENDED_COUNT: usize = 16;
const BACK_REFERENCE_DISTANCE_MASK: u8 = 0xe0;
const BACK_REFERENCE_DISTANCE_SHIFT: usize = 3;
const BACK_REFERENCE_LENGTH_MASK: u8 = 0x0f;
const MAX_EXTENSION: u32 = 0xff;
/// Longest literal run: the 16 base count plus the 255-byte extension.
pub const MAX_LITERAL: u32 = LITERAL_EXTENDED_COUNT as u32 + MAX_EXTENSION;
/// Shortest back-reference; a 3-byte match still costs two bytes.
pub const MIN_MATCH: u32 = 3;
/// Longest back-reference: the base length plus the length code and extension.
pub const MAX_MATCH: u32 = MIN_MATCH + BACK_REFERENCE_LENGTH_MASK as u32 + MAX_EXTENSION;

/// Decode a type-1 payload, validating the header and the declared size.
pub fn decode(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() < HEADER {
        return Err(Error::Lz77Truncated);
    }
    let declared = u32::from_le_bytes(payload[..DECODED_SIZE_SIZE].try_into().unwrap()) as usize;
    let marker = u16::from_le_bytes(payload[DECODED_SIZE_SIZE..HEADER].try_into().unwrap());
    if marker != HEADER_MARKER {
        return Err(Error::Lz77BadMarker(marker));
    }

    let stream = &payload[HEADER..];
    let mut out = Vec::with_capacity(declared);
    let mut pos = 0;
    while let Some(&ctrl) = stream.get(pos) {
        pos += 1;
        if ctrl == END_MARKER {
            break;
        }
        if ctrl & LITERAL_CONTROL_FLAG != 0 {
            let mut len = LITERAL_COUNT_BASE - (ctrl as usize & LITERAL_COUNT_MASK as usize);
            if len == LITERAL_EXTENDED_COUNT {
                len += *stream.get(pos).ok_or(Error::Lz77TruncatedStream)? as usize;
                pos += 1;
            }
            let run = stream.get(pos..pos + len).ok_or(Error::Lz77TruncatedStream)?;
            out.extend_from_slice(run);
            pos += len;
        } else {
            let low = (((ctrl & BACK_REFERENCE_DISTANCE_MASK) as usize) << BACK_REFERENCE_DISTANCE_SHIFT) | *stream.get(pos).ok_or(Error::Lz77TruncatedStream)? as usize;
            pos += 1;
            let distance = WINDOW - low;
            let length_code = ctrl & BACK_REFERENCE_LENGTH_MASK;
            let mut len = length_code as usize + MIN_MATCH as usize;
            if length_code == BACK_REFERENCE_LENGTH_MASK {
                len += *stream.get(pos).ok_or(Error::Lz77TruncatedStream)? as usize;
                pos += 1;
            }
            if distance > out.len() {
                return Err(Error::Lz77DistanceUnderflow { distance, produced: out.len() });
            }
            for _ in 0..len {
                let byte = out[out.len() - distance];
                out.push(byte);
            }
        }
    }
    if out.len() != declared {
        return Err(Error::Lz77SizeMismatch { declared, decoded: out.len() });
    }
    Ok(out)
}

/// Search key: the first `KEY_LENGTH` input bytes at `pos`.
const KEY_LENGTH: usize = 3;

fn key(input: &[u8], pos: usize) -> Option<u32> {
    let bytes = input.get(pos..pos + KEY_LENGTH)?;
    let mut key = 0u32;
    for (shift, byte) in bytes.iter().enumerate() {
        key |= (*byte as u32) << (8 * shift);
    }
    Some(key)
}

/// One frame of a type-1 control stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    /// Bytes this frame produces.
    pub length: u32,
    /// Back-reference distance; `None` for a literal run.
    pub distance: Option<usize>,
}

/// Split a type-1 control stream without its header into frames.
pub fn frames(stream: &[u8]) -> Vec<Frame> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(&ctrl) = stream.get(pos) {
        pos += 1;
        if ctrl == END_MARKER {
            break;
        }
        if ctrl & LITERAL_CONTROL_FLAG != 0 {
            let mut length = LITERAL_COUNT_BASE as u32 - (ctrl as u32 & LITERAL_COUNT_MASK as u32);
            if length == LITERAL_EXTENDED_COUNT as u32 {
                length += stream[pos] as u32;
                pos += 1;
            }
            pos += length as usize;
            out.push(Frame { length, distance: None });
        } else {
            let low = (((ctrl & BACK_REFERENCE_DISTANCE_MASK) as usize) << BACK_REFERENCE_DISTANCE_SHIFT) | stream[pos] as usize;
            pos += 1;
            let length_code = ctrl & BACK_REFERENCE_LENGTH_MASK;
            let mut length = length_code as u32 + MIN_MATCH;
            if length_code == BACK_REFERENCE_LENGTH_MASK {
                length += stream[pos] as u32;
                pos += 1;
            }
            out.push(Frame { length, distance: Some(WINDOW - low) });
        }
    }
    out
}

/// Stateful type-1 encoder.
///
/// Search rule: the longest match in the window wins, the nearest candidate
/// wins a length tie, and a match shorter than the one starting at the next
/// byte is deferred (lazy matching). The position is in the search structure
/// before the lookahead runs, and every consumed byte is indexed.
///
/// This is the closest measured approximation of the retail encoder, not a
/// reproduction; `wiki: formats/lz77` records the residual. The
/// 65-entry gate in `tests/lz77.rs` reports each first-token divergence.
pub struct Encoder<'a> {
    input: &'a [u8],
    chain: HashMap<u32, Vec<u32>>,
    literals: Vec<u8>,
    stream: Vec<u8>,
    pos: usize,
}

impl<'a> Encoder<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, chain: HashMap::new(), literals: Vec::new(), stream: Vec::new(), pos: 0 }
    }

    /// Encode the whole input; call once.
    pub fn run(mut self) -> Vec<u8> {
        while self.pos < self.input.len() {
            let current = self.best(self.pos);
            let linked = self.pos;
            self.insert(linked);
            if current.0 >= MIN_MATCH && self.pos + 1 < self.input.len() {
                let next = self.best(self.pos + 1);
                if next.0 > current.0 {
                    self.push_literal();
                    continue;
                }
            }
            if current.0 >= MIN_MATCH {
                self.emit(current);
                continue;
            }
            self.push_literal();
        }
        self.flush_literals();
        let mut payload = Vec::with_capacity(HEADER + self.stream.len() + 1);
        payload.extend_from_slice(&(self.input.len() as u32).to_le_bytes());
        payload.extend_from_slice(&HEADER_MARKER.to_le_bytes());
        payload.extend_from_slice(&self.stream);
        payload.push(END_MARKER);
        payload
    }

    fn push_literal(&mut self) {
        self.literals.push(self.input[self.pos]);
        self.pos += 1;
        if self.literals.len() == MAX_LITERAL as usize {
            self.flush_literals();
        }
    }

    fn emit(&mut self, candidate: (u32, usize)) {
        let (length, distance) = candidate;
        self.flush_literals();
        let low = WINDOW - distance;
        let extra = length - MIN_MATCH;
        self.stream.push((((low >> BACK_REFERENCE_DISTANCE_SHIFT) as u8) & BACK_REFERENCE_DISTANCE_MASK) | extra.min(BACK_REFERENCE_LENGTH_MASK as u32) as u8);
        self.stream.push(low as u8);
        if extra >= BACK_REFERENCE_LENGTH_MASK as u32 {
            self.stream.push((extra - BACK_REFERENCE_LENGTH_MASK as u32) as u8);
        }
        // The frame start was indexed before lookahead; index the remaining bytes.
        for offset in self.pos + 1..self.pos + length as usize {
            self.insert(offset);
        }
        self.pos += length as usize;
    }

    fn flush_literals(&mut self) {
        if self.literals.is_empty() {
            return;
        }
        let length = self.literals.len();
        if length < LITERAL_EXTENDED_COUNT {
            self.stream.push(((LITERAL_COUNT_BASE - length) as u8) | LITERAL_CONTROL_FLAG);
        } else {
            self.stream.push(LITERAL_CONTROL_FLAG);
            self.stream.push((length - LITERAL_EXTENDED_COUNT) as u8);
        }
        self.stream.extend_from_slice(&self.literals);
        self.literals.clear();
    }

    fn insert(&mut self, pos: usize) {
        if let Some(key) = key(self.input, pos) {
            self.chain.entry(key).or_default().push(pos as u32);
        }
    }

    /// Longest match in the window, nearest distance on a length tie;
    /// `(0, 0)` when no candidate reaches [`MIN_MATCH`].
    fn best(&self, pos: usize) -> (u32, usize) {
        let Some(positions) = key(self.input, pos).and_then(|key| self.chain.get(&key)) else {
            return (0, 0);
        };
        let limit = self.input.len().min(pos + MAX_MATCH as usize);
        let oldest = pos.saturating_sub(MAX_SEARCH_DISTANCE);
        let mut best_len = 0;
        let mut best_dist = 0;
        for &candidate in positions.iter().rev() {
            let candidate = candidate as usize;
            if candidate == pos {
                continue;
            }
            if candidate < oldest {
                break;
            }
            if best_len > 0 {
                let reach = pos + best_len as usize;
                if reach >= self.input.len() || self.input[candidate + best_len as usize] != self.input[reach] {
                    continue;
                }
            }
            let mut length = 0;
            while pos + length < limit && self.input[candidate + length] == self.input[pos + length] {
                length += 1;
            }
            let length = length as u32;
            let distance = pos - candidate;
            let better = length > best_len || (length == best_len && distance < best_dist);
            if better {
                best_len = length;
                best_dist = distance;
                if best_len == MAX_MATCH {
                    break;
                }
            }
        }
        (best_len, best_dist)
    }
}

/// Encode `input` as a type-1 payload.
pub fn encode(input: &[u8]) -> Vec<u8> {
    Encoder::new(input).run()
}
