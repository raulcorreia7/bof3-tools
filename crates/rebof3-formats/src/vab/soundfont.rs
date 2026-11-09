//! SoundFont 2 for a VAB bank: the export of a header and body to the open file, and the import of that
//! file back to the header and body the game loads.
//!
//! [`export`] times each tone's volume envelope by the audio library's own envelope steps.
//!
//! Each preset of bank 0 becomes a program, each zone a tone, and each sample a VAG that the ADPCM encoder writes
//! from the sample's frames. A zone keeps its keys, root, tuning, level, pan, loop and volume envelope. Three
//! things change on the way:
//! - The envelope becomes the ADSR register pair whose timing matches the SoundFont times in every phase. Where no
//!   pair matches, it is the nearest pair. The search runs field by field over the audio library's own envelope
//!   timing.
//! - A loop ends at the last frame of its sample, because a VAG ends at its first end flag. Frames past a loop's
//!   end are cut.
//! - The samples are ADPCM again. An encoded sample decodes to the same frames, so the bank that comes out of
//!   this import, exported and imported again, is the same bank.
//!
//! The fields a SoundFont does not hold take their defaults: priorities, reverb, vibrato, portamento, bend, the
//! program's volume and pan, and the bank id.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use psx_formats::adpcm::{self, BLOCK_BYTES, BLOCK_SAMPLES, FLAG_END, FLAG_LOOP_START, FLAG_REPEAT};
use psx_formats::sf2::{self, Envelope, Sample};
use psx_formats::vab::{self, BankProgram, Program, TONES_PER_PROGRAM, Tone};

use crate::error::{Error, Result};

/// Programs a bank holds, and the volume and pan of a program record the import writes.
const PROGRAM_LIMIT: u16 = 128;
const PROGRAM_VOLUME: u8 = 127;
const PROGRAM_PAN: u8 = 64;
/// Byte of an ADPCM block that holds its flags.
const FLAG_AT: usize = 1;
/// The attack, decay and release fields of ADSR, and the sustain level.
const ATTACK_FIELDS: usize = 256;
const DECAY_FIELDS: usize = 256;
const RELEASE_FIELDS: usize = 64;
const SUSTAIN_LEVELS: usize = 16;
/// The phase time of a field whose envelope never reaches the phase's end. No search picks it.
const NEVER: i16 = i16::MIN;
/// Sustain nibble for full scale: a decay that starts there ends at once, so an attack is timed alone.
const FULL_SUSTAIN: u16 = 0x0F;

/// The SoundFont 2 file of one VAB bank, from its header and body. `title` names the file inside it. Each
/// tone's volume envelope is timed by the audio library's own envelope steps, from the registers the voice
/// plays.
pub fn export(header: &[u8], body: &[u8], title: &str) -> Result<Vec<u8>> {
    let vab = vab::Bank::parse(header, body)?;
    Ok(sf2::write_with(&vab, body, title, &volume_envelope)?)
}

/// The SoundFont envelope of a tone's ADSR registers. `None` when a phase never ends, which leaves the
/// SoundFont default on that zone.
fn volume_envelope(adsr1: u16, adsr2: u16) -> Option<Envelope> {
    let timing = bof3player::spu::envelope_timing(adsr1, adsr2)?;
    Some(Envelope::new(timing.attack, timing.decay, timing.sustain_level, timing.release))
}

/// Reads a SoundFont 2 file back to the VAB bank it describes: the header and the body.
pub fn import(sf2_file: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let font = sf2::read(sf2_file)?;
    let mut presets: Vec<_> = font.presets.iter().filter(|preset| preset.bank == 0).collect();
    presets.sort_by_key(|preset| preset.number);

    let mut vag_of_sample: Vec<Option<u16>> = vec![None; font.samples.len()];
    let mut vags: Vec<Vec<u8>> = Vec::new();
    let mut programs = Vec::new();
    for preset in presets {
        if preset.number >= PROGRAM_LIMIT {
            return Err(Error::Import(format!("preset {} is past the 128 programs a bank holds", preset.number)));
        }
        let instrument = font
            .instruments
            .get(usize::from(preset.instrument))
            .ok_or_else(|| Error::Import(format!("preset {} names no instrument", preset.number)))?;
        let mut tones = Vec::new();
        for zone in &instrument.zones {
            let index = usize::from(zone.sample);
            let sample =
                font.samples.get(index).ok_or_else(|| Error::Import(format!("a zone names no sample {index}")))?;
            if sample.pcm.is_empty() || zone.keys.0 > zone.keys.1 {
                continue;
            }
            if tones.len() == TONES_PER_PROGRAM {
                return Err(Error::Import(format!("program {} has more than 16 tones", preset.number)));
            }
            let vag = match vag_of_sample[index] {
                Some(vag) => vag,
                None => {
                    let looped =
                        zone.looped && (sample.loop_start as usize) < (sample.loop_end as usize).min(sample.pcm.len());
                    vags.push(vag_of(sample, looped));
                    let vag = u16::try_from(vags.len()).map_err(|_| Error::Import("too many samples".into()))?;
                    vag_of_sample[index] = Some(vag);
                    vag
                }
            };
            let (adsr1, adsr2) = registers(zone.envelope);
            tones.push(Tone {
                program: i16::try_from(preset.number).unwrap_or_default(),
                vol: sf2::vol_of_attenuation(zone.attenuation),
                pan: sf2::pan_of_tenths(zone.pan),
                center_note: zone.root.unwrap_or(sample.root),
                shift: sf2::shift_of_tuning(zone.coarse_tune, zone.fine_tune),
                min_note: zone.keys.0,
                max_note: zone.keys.1,
                adsr1,
                adsr2,
                vag: vag as i16,
                ..Tone::default()
            });
        }
        if tones.is_empty() {
            continue;
        }
        programs.push(BankProgram {
            number: preset.number as u8,
            record: Program { tones: tones.len() as u8, vol: PROGRAM_VOLUME, pan: PROGRAM_PAN, ..Program::default() },
            tones,
        });
    }
    Ok(vab::encode_bank(0, &programs, &vags)?)
}

/// The ADPCM of a sample as one VAG. A looping sample is cut at the end of its loop, and that block carries the
/// repeat flag; the loop starts at the block of its first frame.
fn vag_of(sample: &Sample, looped: bool) -> Vec<u8> {
    let mut frames = sample.pcm.clone();
    let loop_span = looped.then(|| {
        let start = sample.loop_start as usize / BLOCK_SAMPLES;
        let end = (sample.loop_end as usize).min(frames.len()).div_ceil(BLOCK_SAMPLES) - 1;
        (start, end)
    });
    if let Some((_, end)) = loop_span {
        frames.truncate((end + 1) * BLOCK_SAMPLES);
    }
    frames.resize(frames.len().div_ceil(BLOCK_SAMPLES) * BLOCK_SAMPLES, 0);
    let mut vag = adpcm::encode_stream(&frames);
    let blocks = vag.len() / BLOCK_BYTES;
    for block in 0..blocks {
        vag[block * BLOCK_BYTES + FLAG_AT] = 0;
    }
    vag[(blocks - 1) * BLOCK_BYTES + FLAG_AT] = FLAG_END;
    if let Some((start, end)) = loop_span {
        vag[end * BLOCK_BYTES + FLAG_AT] = FLAG_END | FLAG_REPEAT;
        vag[start * BLOCK_BYTES + FLAG_AT] |= FLAG_LOOP_START;
    }
    vag
}

/// The phase times the searches pick from, each read once from the audio library's envelope steps. The attack
/// depends on its own field only. The decay depends on its shift and the sustain level. The release depends on
/// the decay too, because the decay ends at a level that depends on its shift (see [`releases`]).
struct Fields {
    attack: Vec<i16>,
    decay: Vec<i16>,
    sustain: Vec<i16>,
}

fn fields() -> &'static Fields {
    static FIELDS: OnceLock<Fields> = OnceLock::new();
    FIELDS.get_or_init(|| Fields {
        attack: (0..ATTACK_FIELDS as u16)
            .map(|field| time(volume_envelope((field << 8) | FULL_SUSTAIN, 0), |envelope| envelope.attack))
            .collect(),
        decay: (0..DECAY_FIELDS as u16).map(|key| time(volume_envelope(key, 0), |envelope| envelope.decay)).collect(),
        sustain: (0..SUSTAIN_LEVELS as u16)
            .map(|level| volume_envelope(level, 0).map_or(0, |envelope| envelope.sustain))
            .collect(),
    })
}

/// The release time of every release field, after a decay with this key: the decay shift and the sustain
/// level in one byte. Built on first use, since a bank uses few keys.
fn releases(key: u16) -> Vec<i16> {
    static CACHE: Mutex<BTreeMap<u16, Vec<i16>>> = Mutex::new(BTreeMap::new());
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    cache
        .entry(key)
        .or_insert_with(|| {
            (0..RELEASE_FIELDS as u16)
                .map(|field| time(volume_envelope(key, field), |envelope| envelope.release))
                .collect()
        })
        .clone()
}

/// The phase time an envelope gives, or [`NEVER`].
fn time(envelope: Option<Envelope>, pick: fn(Envelope) -> i16) -> i16 {
    envelope.map_or(NEVER, pick)
}

/// The ADSR registers for an envelope. The registers come from the exact match when one exists. Otherwise they
/// are the nearest timing, and the envelope those registers give is matched exactly, so a bank imported from
/// its own export gets the same registers again.
fn registers(envelope: Envelope) -> (u16, u16) {
    let picked = exact_registers(envelope).unwrap_or_else(|| nearest_registers(envelope));
    volume_envelope(picked.0, picked.1).and_then(exact_registers).unwrap_or(picked)
}

/// The first registers, in field order, whose timing equals the envelope in every phase.
fn exact_registers(envelope: Envelope) -> Option<(u16, u16)> {
    let fields = fields();
    let level = fields.sustain.iter().position(|&centibels| centibels == envelope.sustain)?;
    let attack = fields.attack.iter().position(|&time| time == envelope.attack)?;
    (0..16).find_map(|shift| {
        if fields.decay[shift << 4 | level] != envelope.decay {
            return None;
        }
        let field = releases((shift << 4 | level) as u16).iter().position(|&time| time == envelope.release)?;
        Some((pack(attack, shift, level), field as u16))
    })
}

/// The registers with the nearest timing in every phase: the sustain level, the attack, then the decay with
/// that level and the release after it.
fn nearest_registers(envelope: Envelope) -> (u16, u16) {
    let fields = fields();
    let level = nearest(&fields.sustain, envelope.sustain);
    let attack = nearest(&fields.attack, envelope.attack);
    let decays: Vec<i16> = (0..16).map(|shift| fields.decay[shift << 4 | level]).collect();
    let shift = nearest(&decays, envelope.decay);
    let field = nearest(&releases((shift << 4 | level) as u16), envelope.release);
    (pack(attack, shift, level), field as u16)
}

/// The attack field, the decay shift and the sustain level as the high byte and the low byte of ADSR1.
fn pack(attack: usize, shift: usize, level: usize) -> u16 {
    ((attack << 8) | (shift << 4) | level) as u16
}

/// The index of the value nearest `target`. A tie takes the first index, so the choice is the same every time.
fn nearest(values: &[i16], target: i16) -> usize {
    (0..values.len()).min_by_key(|&index| (i32::from(values[index]) - i32::from(target)).abs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slow_attack_is_timed_by_the_library_steps() {
        // Attack shift 16, linear step 7: 4,681 steps of 32 samples reach full scale, about 3.40 s, which is
        // 1200 log2(3.40) = 2117 timecents.
        let envelope = volume_envelope(0x4000, 0).expect("the phases end");
        assert_eq!(envelope.attack, 2117);
        assert_eq!(envelope.sustain, 241, "the decay register's level 0 is 0x800, about 24 dB down");
    }
}
