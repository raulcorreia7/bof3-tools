//! Previews of a bank through the audio library: a sample as the library auditions it, and one note of a program or
//! of a single tone as the game's sound driver plays it.

use bof3player::catalog::{Item, Kind as ItemKind};
use bof3player::{Library, MemorySource, NoNaming};
use psx_formats::sep::{Event, Package, Sequence};
use psx_formats::{sep, vab};
use rebof3_assets::sound::Pcm;
use rebof3_emi::pack::{Payload, pack_payloads};

/// The path a preview archive goes by in the library; it names the catalog ids only.
const PREVIEW_PATH: &str = "PREVIEW.EMI";
/// The table's version word of a preview archive; the library does not read it.
const PREVIEW_ARCHIVE_VERSION: u32 = 0;
/// A preview note's clock: 60 quarter notes a minute at 60 ticks each, so one tick of the sequence is one 1/60 s tick
/// of the driver.
const PREVIEW_TEMPO_US: u32 = 1_000_000;
const PREVIEW_RESOLUTION: u16 = 60;
const PREVIEW_TICKS_PER_SECOND: f64 = 60.0;
/// Full velocity: the tone sounds at its bank, program and tone volume.
const PREVIEW_VELOCITY: u8 = 127;
/// Status bytes of channel 0 and the end-of-track meta event.
const PROGRAM_CHANGE: u8 = 0xC0;
const NOTE_ON: u8 = 0x90;
const META: u8 = 0xFF;
const END_OF_TRACK: u8 = 0x2F;

/// Which tones a preview note keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Keys {
    /// Every tone of the program that answers the note, as the game's note-on keys them.
    Program(u8),
    /// One tone, by its program block and slot in the header; the other tones of its program stay silent.
    Tone {
        /// The program block that holds the tone.
        block: u8,
        /// The tone's slot in that block.
        slot: u8,
    },
}

/// A preview note: which tones it keys, the note, and how long it is held before the key-off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    /// The tones the note keys.
    pub keys: Keys,
    /// The MIDI note number.
    pub note: u8,
    /// Seconds from the key-on to the key-off, at least one driver tick.
    pub hold: f64,
}

/// Renders sample `sample` of a bank as the library auditions it: keyed on at the centre note of the first audible
/// tone that names it, with that tone's envelope and volume, centred, and released after one pass of the sample.
pub fn sample_pcm(header: &[u8], body: &[u8], sample: u32) -> Result<Pcm, String> {
    let archive = bank_archive(header, body, None)?;
    render_preview(&archive, |item| item.kind == ItemKind::Sample && item.sub_index == sample)
        .ok_or_else(|| format!("the bank has no sample {sample}"))?
}

/// Renders one note as the game's sound driver plays it: keyed on at full velocity on the driver's first tick, held
/// for `note.hold` seconds, then released, with the driver's two-second tail.
pub fn note_pcm(header: &[u8], body: &[u8], note: &Note) -> Result<Pcm, String> {
    let (header, program) = match note.keys {
        Keys::Program(program) => (header.to_vec(), program),
        Keys::Tone { block, slot } => only_tone(header, body, block, slot, note.note)?,
    };
    let hold = (note.hold.max(0.0) * PREVIEW_TICKS_PER_SECOND).round().max(1.0) as u32;
    let package = Package { sequences: vec![one_note(program, note.note, hold)] }.encode();
    let archive = bank_archive(&header, body, Some(&package))?;
    render_preview(&archive, |item| item.kind == ItemKind::Music)
        .ok_or_else(|| "the preview sequence is not in the catalog".to_string())?
}

/// The header with every other tone of the program of tone `block`/`slot` emptied, and that program.
fn only_tone(header: &[u8], body: &[u8], block: u8, slot: u8, note: u8) -> Result<(Vec<u8>, u8), String> {
    let mut bank = vab::Bank::parse(header, body).map_err(|error| error.to_string())?;
    let is_it = |tone: &vab::Tone| tone.storage_block == block && tone.tone_slot == slot;
    let tone = *bank
        .tones
        .iter()
        .find(|tone| is_it(tone))
        .ok_or_else(|| format!("the bank has no tone in block {block}, slot {slot}"))?;
    let program = u8::try_from(tone.program)
        .ok()
        .filter(|_| tone.is_usable(bank.vag_count))
        .ok_or_else(|| "the tone names no sample the bank holds".to_string())?;
    if !(tone.min_note..=tone.max_note).contains(&note) {
        return Err(format!("the tone answers notes {} to {}", tone.min_note, tone.max_note));
    }
    for other in bank.tones.iter_mut().filter(|other| other.program == tone.program && !is_it(other)) {
        other.vag = 0;
    }
    Ok((bank.encode_header(), program))
}

/// A sequence of one note on channel 0: program change, key-on, key-off after `hold` ticks, end.
fn one_note(program: u8, note: u8, hold: u32) -> Sequence {
    let event =
        |delta, status, data1, data2| Event { delta, status, data1, data2, status_byte: true, ..Event::default() };
    Sequence {
        seq_id: 0,
        resolution: PREVIEW_RESOLUTION,
        tempo_us: PREVIEW_TEMPO_US,
        time_num: 4,
        time_den: 2,
        events: vec![
            event(0, PROGRAM_CHANGE, program, 0),
            event(0, NOTE_ON, note, PREVIEW_VELOCITY),
            event(hold, NOTE_ON, note, 0),
            Event { status: META, meta_type: END_OF_TRACK, status_byte: true, ..Event::default() },
        ],
    }
}

/// An archive of one bank and, when given, one sequence package: the bank is the archive's bank 0.
fn bank_archive(header: &[u8], body: &[u8], package: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut payloads = vec![
        Payload { bytes: header, file_type: vab::HEADER_TYPE, ram_ptr: 0 },
        Payload { bytes: body, file_type: vab::BODY_TYPE, ram_ptr: 0 },
    ];
    if let Some(bytes) = package {
        payloads.push(Payload { bytes, file_type: sep::TYPE, ram_ptr: 0 });
    }
    pack_payloads(PREVIEW_ARCHIVE_VERSION, &payloads).map_err(|error| error.to_string())
}

/// Renders the first catalog item of `archive` that `wanted` picks, or `None` when it picks none.
fn render_preview(archive: &[u8], wanted: impl Fn(&Item) -> bool) -> Option<Result<Pcm, String>> {
    let mut source = MemorySource::new();
    source.add(PREVIEW_PATH, archive);
    let library = match Library::open_with(source, &NoNaming) {
        Ok(library) => library,
        Err(error) => return Some(Err(error.to_string())),
    };
    let item = library.catalog().items().iter().find(|item| wanted(item))?;
    Some(
        library
            .player(item)
            .and_then(|mut player| {
                let samples = player.render_all()?;
                Ok(Pcm { sample_rate: player.sample_rate(), channels: player.channels(), samples })
            })
            .map_err(|error| error.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use psx_formats::VAB_SAMPLE_RATE;
    use psx_formats::adpcm::{self, BLOCK_BYTES, BLOCK_SAMPLES, FLAG_END, FLAG_LOOP_START, FLAG_REPEAT};
    use psx_formats::vab::{BankProgram, Program, Tone, encode_bank};

    /// Samples in one period of the test tone: one ADPCM block, so the sample loops on a whole period.
    const PERIOD: usize = BLOCK_SAMPLES;
    /// The test tone's frequency at the sample's recorded rate.
    const TONE_HZ: f64 = VAB_SAMPLE_RATE as f64 / PERIOD as f64;
    const AMPLITUDE: f64 = 8000.0;
    /// The ADPCM byte that holds a block's flags.
    const FLAGS_AT: usize = 1;
    /// Full volume and the centre pan, on the 7-bit scale.
    const FULL: u8 = 127;
    const CENTRE: u8 = 64;
    /// The fastest attack, no decay, full sustain, and the fastest release.
    const ADSR1: u16 = 0x000F;
    const ADSR2: u16 = 0x0000;
    const CENTRE_NOTE: u8 = 60;

    /// A sample of `blocks` periods of a sine that loops over all of them.
    fn looping_sine(blocks: usize) -> Vec<u8> {
        let samples: Vec<i16> = (0..PERIOD * blocks)
            .map(|n| (AMPLITUDE * (std::f64::consts::TAU * n as f64 / PERIOD as f64).sin()) as i16)
            .collect();
        let mut vag = adpcm::encode_stream(&samples);
        vag[FLAGS_AT] |= FLAG_LOOP_START;
        let last = vag.len() - BLOCK_BYTES;
        vag[last + FLAGS_AT] |= FLAG_END | FLAG_REPEAT;
        vag
    }

    /// A sample of silence that loops.
    fn looping_silence(blocks: usize) -> Vec<u8> {
        let mut vag = adpcm::encode_stream(&vec![0; PERIOD * blocks]);
        let last = vag.len() - BLOCK_BYTES;
        vag[last + FLAGS_AT] |= FLAG_END | FLAG_REPEAT;
        vag
    }

    /// A tone of program 0 over every note that plays sample `vag` at `pan`.
    fn tone(vag: i16, pan: u8) -> Tone {
        Tone {
            program: 0,
            vag,
            vol: FULL,
            pan,
            center_note: CENTRE_NOTE,
            min_note: 0,
            max_note: 127,
            adsr1: ADSR1,
            adsr2: ADSR2,
            ..Tone::default()
        }
    }

    /// A bank whose program 0 holds `tones`.
    fn bank(tones: Vec<Tone>, vags: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
        let record = Program { tones: tones.len() as u8, vol: FULL, pan: CENTRE, ..Program::default() };
        encode_bank(0, &[BankProgram { number: 0, record, tones }], vags).expect("the test bank encodes")
    }

    /// One channel of interleaved PCM.
    fn channel(pcm: &Pcm, which: usize) -> Vec<i16> {
        pcm.samples.iter().skip(which).step_by(pcm.channels as usize).copied().collect()
    }

    /// The frequency of the left channel over its first `seconds` after `skip` seconds, from its rising zero
    /// crossings.
    fn frequency(pcm: &Pcm, skip: f64, seconds: f64) -> f64 {
        let rate = f64::from(pcm.sample_rate);
        let left = channel(pcm, 0);
        let window = &left[(skip * rate) as usize..((skip + seconds) * rate) as usize];
        let rising = window.windows(2).filter(|pair| pair[0] < 0 && pair[1] >= 0).count();
        rising as f64 / seconds
    }

    fn note(keys: Keys, note: u8) -> Note {
        Note { keys, note, hold: 1.0 }
    }

    #[test]
    fn a_tone_at_its_centre_note_plays_its_sample_at_the_library_rate() {
        let (header, body) = bank(vec![tone(1, CENTRE)], &[looping_sine(4)]);
        let pcm = note_pcm(&header, &body, &note(Keys::Tone { block: 0, slot: 0 }, CENTRE_NOTE)).unwrap();
        assert_eq!((pcm.sample_rate, pcm.channels), (VAB_SAMPLE_RATE, 2));
        let heard = frequency(&pcm, 0.1, 0.8);
        assert!((heard - TONE_HZ).abs() < TONE_HZ * 0.01, "{heard} Hz, not {TONE_HZ}");
    }

    #[test]
    fn a_tone_one_octave_up_plays_at_twice_the_pitch() {
        let (header, body) = bank(vec![tone(1, CENTRE)], &[looping_sine(4)]);
        let pcm = note_pcm(&header, &body, &note(Keys::Tone { block: 0, slot: 0 }, CENTRE_NOTE + 12)).unwrap();
        let heard = frequency(&pcm, 0.1, 0.8);
        assert!((heard - 2.0 * TONE_HZ).abs() < TONE_HZ * 0.02, "{heard} Hz, not {}", 2.0 * TONE_HZ);
    }

    #[test]
    fn a_tone_panned_left_stays_on_the_left_channel() {
        let (header, body) = bank(vec![tone(1, 0)], &[looping_sine(4)]);
        let pcm = note_pcm(&header, &body, &note(Keys::Program(0), CENTRE_NOTE)).unwrap();
        assert_eq!(pcm.channels, 2);
        assert!(channel(&pcm, 0).iter().any(|&sample| sample != 0), "the left channel sounds");
        assert!(channel(&pcm, 1).iter().all(|&sample| sample == 0), "the right channel is silent");
    }

    #[test]
    fn a_note_is_released_after_its_hold() {
        let (header, body) = bank(vec![tone(1, CENTRE)], &[looping_sine(4)]);
        let pcm = note_pcm(&header, &body, &note(Keys::Program(0), CENTRE_NOTE)).unwrap();
        let left = channel(&pcm, 0);
        let rate = VAB_SAMPLE_RATE as usize;
        assert!(left[rate / 2..rate].iter().any(|&sample| sample != 0), "the note sounds while it is held");
        assert!(left[left.len() - rate..].iter().all(|&sample| sample == 0), "the release ends the note");
    }

    #[test]
    fn a_tone_preview_keys_only_that_tone() {
        let (header, body) = bank(vec![tone(1, CENTRE), tone(2, CENTRE)], &[looping_sine(4), looping_silence(4)]);
        let sounds = |keys| note_pcm(&header, &body, &note(keys, CENTRE_NOTE)).unwrap().samples.iter().any(|&s| s != 0);
        assert!(sounds(Keys::Program(0)), "the program keys both tones");
        assert!(sounds(Keys::Tone { block: 0, slot: 0 }), "the sine tone");
        assert!(!sounds(Keys::Tone { block: 0, slot: 1 }), "the silent tone alone");
    }

    #[test]
    fn a_tone_refuses_a_note_it_does_not_answer() {
        let narrow = Tone { min_note: 60, max_note: 64, ..tone(1, CENTRE) };
        let (header, body) = bank(vec![narrow], &[looping_sine(4)]);
        let error = note_pcm(&header, &body, &note(Keys::Tone { block: 0, slot: 0 }, 72)).unwrap_err();
        assert_eq!(error, "the tone answers notes 60 to 64");
    }

    #[test]
    fn a_sample_plays_as_the_library_auditions_it() {
        let (header, body) = bank(vec![tone(1, CENTRE)], &[looping_sine(64)]);
        let pcm = sample_pcm(&header, &body, 1).unwrap();
        assert_eq!((pcm.sample_rate, pcm.channels), (VAB_SAMPLE_RATE, 2));
        let heard = frequency(&pcm, 0.005, 0.03);
        assert!((heard - TONE_HZ).abs() < TONE_HZ * 0.05, "{heard} Hz, not {TONE_HZ}");
        assert!(sample_pcm(&header, &body, 2).is_err(), "no sample 2");
    }
}
