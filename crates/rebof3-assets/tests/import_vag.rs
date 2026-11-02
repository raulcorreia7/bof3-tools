//! `import vag` encodes a WAVE file, which carries no loop points, so no block of the VAG has a loop flag.

use rebof3_assets::{decode, import};

const BLOCK_BYTES: usize = 16;
const FLAG_AT: usize = 1;
const FLAG_REPEAT: u8 = 0x02;
const FLAG_LOOP_START: u8 = 0x04;

#[test]
fn vag_import_sets_no_loop_flags() {
    let samples: Vec<i16> = (0..28 * 8).map(|n| ((n * 37) % 2000 - 1000) as i16).collect();
    let wav = decode::pcm_wav(&samples).expect("a synthetic WAVE file");
    let vag = import::vag(&wav).expect("the WAVE file encodes");
    assert!(!vag.is_empty() && vag.len().is_multiple_of(BLOCK_BYTES));
    for (index, block) in vag.chunks(BLOCK_BYTES).enumerate() {
        assert_eq!(block[FLAG_AT] & (FLAG_REPEAT | FLAG_LOOP_START), 0, "block {index} carries a loop flag");
    }
}
