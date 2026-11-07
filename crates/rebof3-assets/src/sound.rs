//! Sound through `bof3player`: every playable item rendered to WAV, FLAC or Ogg Vorbis.
//!
//! The `bof3player` catalog lists every playable item of the disc's sound archives. Music, samples
//! and XA streams render through a [`bof3player::Player`] to 16-bit PCM, which the selected
//! [`AudioFormat`] writes. A music item's sequence package is the `.seq` of the same archive, which
//! the `audio-sequence` kind keeps as stored bytes; its bank is the `.vh`/`.vb` pair.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::thread;

use bof3player::catalog::{Item, Kind as ItemKind};
use bof3player::discover::{self, SourceKind};
use bof3player::export::Tags;
use bof3player::{DiscSource, Library, export};
use psx_cd::ReadSeek;

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

fn audio(error: bof3player::Error) -> Error {
    Error::Audio(error.to_string())
}
