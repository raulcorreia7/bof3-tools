//! Sound through `bof3player`: every playable item rendered to WAV, FLAC or Ogg Vorbis.
//!
//! The `bof3player` catalog lists every playable item of the disc's sound archives. Music, samples
//! and XA streams render through a [`bof3player::Player`] to 16-bit PCM, which the selected
//! [`AudioFormat`] writes. A music item's sequence package is the `.seq` of the same archive, which
//! the `audio-sequence` kind keeps as stored bytes; its bank is the `.vh`/`.vb` pair.
//!
//! The editor's previews go through the same player: [`sequence_pcm`] plays a sequence,
//! [`sample_pcm`] auditions a sample as the library does, and [`note_pcm`] keys one note of a program or
//! of a single tone, so every preview has the driver's pitch, envelope, loop, interpolation and mix.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::thread;

use bof3player::catalog::{Item, Kind as ItemKind};
use bof3player::discover::{self, SourceKind};
use bof3player::export::Tags;
use bof3player::{DiscSource, Library, MemorySource, NoNaming, export};
use psx_cd::ReadSeek;
use psx_formats::sep::{Event, Package, Sequence};
use psx_formats::{sep, vab};
use rebof3_emi::pack::{Payload, pack_payloads};

use crate::error::{Error, Result};
use crate::kind::{AudioFormat, Kind, Mode};

/// Rendered clips queued per worker before the workers wait for the writer.
const QUEUE_PER_WORKER: usize = 2;

/// One sound item rendered to a file.
pub struct Clip {
    /// The asset kind the file belongs to.
    pub kind: Kind,
    /// The file's path below the output directory.
    pub output: String,
    /// The file's extension: `wav`, `flac` or `ogg`.
    pub extension: &'static str,
    /// The loader evidence: the catalog id and the bank or sector range the item names.
    pub evidence: String,
    /// The archive the item is read from, as the catalog names it.
    pub source: String,
    /// The file's bytes.
    pub bytes: Vec<u8>,
    /// A music item's Standard MIDI File, written beside the rendered audio.
    pub midi: Option<Vec<u8>>,
    /// The item's original bytes in the game's own format, when it has them:
    /// a sample's waveform data, as the VAB body stores it.
    pub original: Option<Original>,
}

/// One item's original bytes, in the format the game stores them.
pub struct Original {
    /// The file's path below the output directory.
    pub output: String,
    /// The file's bytes, exactly as the disc stores them.
    pub bytes: Vec<u8>,
}

/// Opens the disc image as a `bof3player` library. An image too small to be a game disc fails here.
pub fn open(image: &Path) -> Result<Library<'static>> {
    discover::open_with(SourceKind::Disc, image, bof3player::names::game_names()).map_err(audio)
}

/// Renders every playable music, sample and XA item of `library` whose kind `wanted` selects.
///
/// `mode` limits the work: raw mode reads only the samples' original bytes, and modern mode skips them. The items
/// render on all cores. Each file goes to `emit` in catalog order as soon as it and the
/// items before it are rendered, so the run holds a few clips at a time. The result is one line
/// per item that could not be rendered, in catalog order.
pub fn render(
    library: &Library<'_>,
    wanted: impl Fn(Kind) -> bool,
    mode: Mode,
    container: AudioFormat,
    emit: impl FnMut(Clip) -> Result<()>,
) -> Result<Vec<String>> {
    let jobs = thread::available_parallelism().map_or(1, NonZeroUsize::get);
    render_with_jobs(library, wanted, mode, container, jobs, emit)
}

pub(crate) fn render_with_jobs(
    library: &Library<'_>,
    wanted: impl Fn(Kind) -> bool,
    mode: Mode,
    container: AudioFormat,
    jobs: usize,
    mut emit: impl FnMut(Clip) -> Result<()>,
) -> Result<Vec<String>> {
    let work: Vec<(&Item, Kind)> = library
        .catalog()
        .items()
        .iter()
        .filter(|item| item.playable)
        .filter_map(|item| kind_of(item.kind).filter(|kind| wanted(*kind)).map(|kind| (item, kind)))
        .filter(|(item, _)| mode != Mode::Raw || item.kind == ItemKind::Sample)
        .collect();
    let workers = jobs.min(work.len()).max(1);
    let next = AtomicUsize::new(0);
    let (sender, receiver) = sync_channel::<(usize, std::result::Result<Clip, String>)>(workers * QUEUE_PER_WORKER);
    let mut failures = Vec::new();
    thread::scope(|scope| -> Result<()> {
        for _ in 0..workers {
            let sender = sender.clone();
            let (work, next) = (&work, &next);
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(item, kind)) = work.get(index) else {
                        return;
                    };
                    let result =
                        clip(library, item, kind, mode, container).map_err(|error| format!("{}: {error}", item.id));
                    if sender.send((index, result)).is_err() {
                        return;
                    }
                }
            });
        }
        drop(sender);
        let mut arrived = BTreeMap::new();
        let mut due = 0;
        for (index, result) in receiver {
            arrived.insert(index, result);
            while let Some(result) = arrived.remove(&due) {
                match result {
                    Ok(clip) => emit(clip)?,
                    Err(line) => failures.push(line),
                }
                due += 1;
            }
        }
        Ok(())
    })?;
    Ok(failures)
}

/// The asset kind an item's file belongs to, or `None` for items another kind already covers.
fn kind_of(kind: ItemKind) -> Option<Kind> {
    match kind {
        ItemKind::Music => Some(Kind::AudioSequence),
        ItemKind::Sample => Some(Kind::AudioBank),
        ItemKind::Xa => Some(Kind::XaAudio),
        _ => None,
    }
}

fn clip(library: &Library<'_>, item: &Item, kind: Kind, mode: Mode, container: AudioFormat) -> Result<Clip> {
    let extension = container.extension();
    let bytes = if mode.writes(extension, true) {
        let mut player = library.player(item).map_err(audio)?;
        let music = item.kind == ItemKind::Music;
        let window = player.loop_window().filter(|_| music);
        let mut pcm = player.render_all().map_err(audio)?;
        let rate = player.sample_rate();
        let channels = player.channels();
        // FLAC and Ogg music carries the intro and one loop; WAV keeps the full item.
        if matches!(container, AudioFormat::Flac | AudioFormat::Ogg)
            && let Some((_, end)) = window
        {
            let end = (end * f64::from(rate)).round() as usize * channels as usize;
            if pcm.len() > end {
                pcm.truncate(end);
            }
        }
        let (loop_start, loop_length) = window.map_or((None, None), |(start, end)| {
            (Some((start * f64::from(rate)).round() as u64), Some(((end - start) * f64::from(rate)).round() as u64))
        });
        let tags = Tags {
            title: if item.title.is_empty() { item.name.as_str() } else { item.title.as_str() },
            artist: if music { "Breath of Fire III" } else { "" },
            album: if music { "Breath of Fire III" } else { "" },
            track: music.then_some(item.index),
            loop_start,
            loop_length,
        };
        encode(container, &pcm, rate, channels, &tags)?
    } else {
        Vec::new()
    };
    // The item directory drops the source extension, so it never shares a name with a stored file.
    let stem = Path::new(&item.path).with_extension("");
    let name = item.id.replace([':', '#'], "-");
    let evidence = match item.kind {
        ItemKind::Music => format!(
            "bof3player catalog item {}; bank {} of {} (its .vh/.vb pair in audio-bank)",
            item.id, item.bank, item.path
        ),
        ItemKind::Sample => {
            format!("bof3player catalog item {}; bank {}, sample {}", item.id, item.bank, item.sub_index)
        }
        _ => format!("bof3player catalog item {}; {} sectors from sector {}", item.id, item.sectors, item.sub_index),
    };
    // A sample's original bytes are its waveform data, as the VAB body stores them.
    let original = match item.kind {
        ItemKind::Sample if mode.writes("vag", true) => library
            .export_vag(item)
            .ok()
            .map(|bytes| Original { output: format!("{}/{}/{name}.vag", kind.name(), stem.display()), bytes }),
        _ => None,
    };
    Ok(Clip {
        kind,
        output: format!("{}/{}/{name}.{extension}", kind.name(), stem.display()),
        extension,
        evidence,
        source: item.path.clone(),
        bytes,
        midi: (item.kind == ItemKind::Music && mode.writes("midi", true))
            .then(|| library.export_midi(item).map_err(audio))
            .transpose()?,
        original,
    })
}

/// Encodes interleaved PCM in the selected container, with `title` in the tags.
pub(crate) fn encode(
    container: AudioFormat,
    pcm: &[i16],
    rate: u32,
    channels: u32,
    tags: &Tags<'_>,
) -> Result<Vec<u8>> {
    match container {
        AudioFormat::Wav => export::wav_bytes(pcm, rate, channels),
        AudioFormat::Flac => export::flac_bytes(pcm, rate, channels, tags),
        AudioFormat::Ogg => export::ogg_bytes(pcm, rate, channels, export::DEFAULT_OGG_QUALITY, tags),
    }
    .map_err(audio)
}

/// The kind of a `bof3player` catalog item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoundKind {
    /// A sequence: exported as a Standard MIDI File.
    Music,
    /// A VAB bank: its samples are the sample items.
    Bank,
    /// One VAG sample of a bank: rendered as PCM.
    Sample,
    /// A streaming XA channel: rendered as PCM.
    Xa,
    /// A CD-DA track, which exists only on a `.cue` sheet's audio track.
    Cdda,
}

impl From<ItemKind> for SoundKind {
    fn from(kind: ItemKind) -> Self {
        match kind {
            ItemKind::Music => Self::Music,
            ItemKind::Bank => Self::Bank,
            ItemKind::Sample => Self::Sample,
            ItemKind::Xa => Self::Xa,
            ItemKind::Cdda => Self::Cdda,
        }
    }
}

/// One catalog item of a disc's sound.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundItem {
    /// What the item is.
    pub kind: SoundKind,
    /// The catalog id, for example `music:BGM000#0`.
    pub id: String,
    /// The archive the item is read from.
    pub path: String,
    /// Whether the item has audio to play.
    pub playable: bool,
    /// For banks, samples and music: the archive's bank number.
    pub bank: u32,
    /// The file-level index: sequence id, bank number or channel.
    pub index: u32,
    /// The sample number or the first sector.
    pub sub_index: u32,
    /// For XA items: the sectors in the item's range.
    pub sectors: u32,
    /// The length in seconds, when it is known without rendering.
    pub duration: Option<f64>,
}

fn item_view(item: &Item) -> SoundItem {
    SoundItem {
        kind: item.kind.into(),
        id: item.id.clone(),
        path: item.path.clone(),
        playable: item.playable,
        bank: item.bank,
        index: item.index,
        sub_index: item.sub_index,
        sectors: item.sectors,
        duration: item.duration,
    }
}

/// Decoded audio: interleaved 16-bit PCM.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pcm {
    /// Frames per second.
    pub sample_rate: u32,
    /// Interleaved channels per frame.
    pub channels: u32,
    /// `frames * channels` samples.
    pub samples: Vec<i16>,
}

/// A disc's sound catalog, read through `bof3player`.
pub struct Sounds {
    library: Library<'static>,
}

impl Sounds {
    /// Opens a disc image from a path: a `.cue` sheet, an image, or a directory that holds one.
    pub fn open_path(image: &Path) -> Result<Self> {
        Ok(Self { library: open(image)? })
    }

    /// Opens a single-track disc image read through `reader`, which may come from memory or from
    /// read callbacks. A `.cue` sheet's audio track is not available this way.
    pub fn open_reader(reader: Box<dyn ReadSeek>) -> Result<Self> {
        let source = DiscSource::from_reader(reader).map_err(audio)?;
        Ok(Self { library: Library::open_with(source, bof3player::names::game_names()).map_err(audio)? })
    }

    /// The number of catalog items, playable or not.
    pub fn len(&self) -> usize {
        self.library.catalog().items().len()
    }

    /// Whether the catalog has no items.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The item at `index`, if there is one.
    pub fn item(&self, index: usize) -> Option<SoundItem> {
        self.library.catalog().items().get(index).map(item_view)
    }

    /// Renders a sample or XA item to PCM with the stored loop mode (one shot by default).
    pub fn pcm(&self, index: usize) -> Result<Pcm> {
        let item = self.catalog_item(index)?;
        if !matches!(item.kind, ItemKind::Sample | ItemKind::Xa) {
            return Err(Error::Audio(format!("{} has no PCM sample data", item.id)));
        }
        let mut player = self.library.player(item).map_err(audio)?;
        let samples = player.render_all().map_err(audio)?;
        Ok(Pcm { sample_rate: player.sample_rate(), channels: player.channels(), samples })
    }

    /// Exports a music item's sequence as a Standard MIDI File.
    pub fn midi(&self, index: usize) -> Result<Vec<u8>> {
        let item = self.catalog_item(index)?;
        if item.kind != ItemKind::Music {
            return Err(Error::Audio(format!("{} is not a music item", item.id)));
        }
        self.library.export_midi(item).map_err(audio)
    }

    fn catalog_item(&self, index: usize) -> Result<&Item> {
        self.library.catalog().items().get(index).ok_or_else(|| Error::Audio(format!("no sound item {index}")))
    }
}

/// Interleaved samples rendered per call while a sequence plays.
const RENDER_CHUNK: usize = 4096;

/// Renders the sequence whose id is `sequence` in one archive, as the game's sound driver plays it with
/// the archive's own sound bank, for at most `seconds`. `path` is the archive's disc path (`BIN/...`)
/// and `archive` its bytes, so an edited archive plays as edited.
pub fn sequence_pcm(path: &str, archive: &[u8], sequence: u32, seconds: f64) -> Result<Pcm> {
    let mut source = MemorySource::new();
    source.add(path, archive);
    let library = Library::open_with(source, &NoNaming).map_err(audio)?;
    let item = library
        .catalog()
        .items()
        .iter()
        .find(|item| item.kind == ItemKind::Music && item.path == path && item.index == sequence)
        .ok_or_else(|| Error::Audio(format!("{path} has no playable sequence {sequence}")))?;
    let mut player = library.player(item).map_err(audio)?;

    let (sample_rate, channels) = (player.sample_rate(), player.channels());
    let limit = (seconds.max(0.0) * f64::from(sample_rate)) as usize * channels as usize;
    let mut samples = Vec::new();
    let mut chunk = vec![0; RENDER_CHUNK];
    while samples.len() < limit {
        let frames = player.render_i16(&mut chunk).map_err(audio)?;
        if frames == 0 {
            break;
        }
        samples.extend_from_slice(&chunk[..frames * channels as usize]);
    }
    samples.truncate(limit);
    Ok(Pcm { sample_rate, channels, samples })
}

/// The path a preview archive goes by in the library; it names the catalog ids only.
const PREVIEW_PATH: &str = "PREVIEW.EMI";
/// The table's version word of a preview archive; the library does not read it.
const PREVIEW_ARCHIVE_VERSION: u32 = 0;
/// A preview note's clock: 60 quarter notes a minute at 60 ticks each, so one tick of the sequence is one
/// 1/60 s tick of the driver.
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

/// Renders sample `sample` of a bank as the library auditions it: keyed on at the centre note of the first
/// audible tone that names it, with that tone's envelope and volume, centred, and released after one pass
/// of the sample. `header` and `body` are the bank's `.vh` and `.vb`, so an edited bank plays as edited.
pub fn sample_pcm(header: &[u8], body: &[u8], sample: u32) -> Result<Pcm> {
    let archive = bank_archive(header, body, None)?;
    render_preview(&archive, |item| item.kind == ItemKind::Sample && item.sub_index == sample)
        .ok_or_else(|| Error::Audio(format!("the bank has no sample {sample}")))?
}

/// Renders one note as the game's sound driver plays it: keyed on at full velocity on the driver's first
/// tick, held for `note.hold` seconds, then released, with the driver's two-second tail. Every tone the
/// note keys has its pitch from its centre note and fine tune, its envelope, its sample's loop and its
/// volume and pan.
pub fn note_pcm(header: &[u8], body: &[u8], note: &Note) -> Result<Pcm> {
    let (header, program) = match note.keys {
        Keys::Program(program) => (header.to_vec(), program),
        Keys::Tone { block, slot } => only_tone(header, body, block, slot, note.note)?,
    };
    let hold = (note.hold.max(0.0) * PREVIEW_TICKS_PER_SECOND).round().max(1.0) as u32;
    let package = Package { sequences: vec![one_note(program, note.note, hold)] }.encode();
    let archive = bank_archive(&header, body, Some(&package))?;
    render_preview(&archive, |item| item.kind == ItemKind::Music)
        .ok_or_else(|| Error::Audio("the preview sequence is not in the catalog".into()))?
}

/// The header with every other tone of the program of tone `block`/`slot` emptied, and that program.
fn only_tone(header: &[u8], body: &[u8], block: u8, slot: u8, note: u8) -> Result<(Vec<u8>, u8)> {
    let mut bank = vab::Bank::parse(header, body)?;
    let is_it = |tone: &vab::Tone| tone.storage_block == block && tone.tone_slot == slot;
    let tone = *bank
        .tones
        .iter()
        .find(|tone| is_it(tone))
        .ok_or_else(|| Error::Audio(format!("the bank has no tone in block {block}, slot {slot}")))?;
    let program = u8::try_from(tone.program)
        .ok()
        .filter(|_| tone.is_usable(bank.vag_count))
        .ok_or_else(|| Error::Audio("the tone names no sample the bank holds".into()))?;
    if !(tone.min_note..=tone.max_note).contains(&note) {
        return Err(Error::Audio(format!("the tone answers notes {} to {}", tone.min_note, tone.max_note)));
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
fn bank_archive(header: &[u8], body: &[u8], package: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut payloads = vec![
        Payload { bytes: header, file_type: vab::HEADER_TYPE, ram_ptr: 0 },
        Payload { bytes: body, file_type: vab::BODY_TYPE, ram_ptr: 0 },
    ];
    if let Some(bytes) = package {
        payloads.push(Payload { bytes, file_type: sep::TYPE, ram_ptr: 0 });
    }
    Ok(pack_payloads(PREVIEW_ARCHIVE_VERSION, &payloads)?)
}

/// Renders the first catalog item of `archive` that `wanted` picks, or `None` when it picks none.
fn render_preview(archive: &[u8], wanted: impl Fn(&Item) -> bool) -> Option<Result<Pcm>> {
    let mut source = MemorySource::new();
    source.add(PREVIEW_PATH, archive);
    let library = match Library::open_with(source, &NoNaming) {
        Ok(library) => library,
        Err(error) => return Some(Err(audio(error))),
    };
    let item = library.catalog().items().iter().find(|item| wanted(item))?;
    Some(
        library
            .player(item)
            .and_then(|mut player| {
                let samples = player.render_all()?;
                Ok(Pcm { sample_rate: player.sample_rate(), channels: player.channels(), samples })
            })
            .map_err(audio),
    )
}

fn audio(error: bof3player::Error) -> Error {
    Error::Audio(error.to_string())
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

    /// The frequency of the left channel over its first `seconds` after `skip` seconds, from its rising
    /// zero crossings.
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
        assert_eq!(error.to_string(), "the tone answers notes 60 to 64");
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
