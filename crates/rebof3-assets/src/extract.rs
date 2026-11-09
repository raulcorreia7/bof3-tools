//! Walks the disc image, classifies every file and every EMI slot, and writes the asset tree.

use std::borrow::Cow;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, sync_channel};
use std::thread;

use psx_cd::checksum::sha256_hex;
use psx_cd::{Entry, Image, cue};
use rebof3_emi::Archive;
use rebof3_emi::texture::{ClutBlock, ResidentClutBlock};
use rebof3_formats::{self as classify, Format, classify};

use crate::decode::derive;
use crate::error::{Error, Result};
use crate::kind::{AudioFormat, Kind, Selection};
use crate::report::{DERIVED_TABLE, RENDERED_ITEM, Report, Row};
use crate::sound;
use rebof3_formats::joins::joined_rows_in;
use rebof3_formats::registry::Site;

/// Name of the table written next to the asset tree.
pub const TABLE_FILE: &str = "assets.jsonl";
/// The archive whose entry 0 holds the fixed game tables (`wiki: data/records#gameemi-fixed-tables`, "Equipment and shop data").
const GAME_TABLES_SOURCE: &str = "BIN/ETC/GAME.EMI";
const TABLE_EVIDENCE: &str = "wiki: data/records#gameemi-fixed-tables (fixed table locations and record layouts); \
     C records: include/game/item.h, include/game/ability.h, include/game/character.h";
/// Name of the summary written next to the asset tree.
pub const SUMMARY_FILE: &str = "summary.txt";
/// The container extension that marks an EMI archive on the disc.
const EMI_EXTENSION: &str = "EMI";
/// Where the first sectors of a `.STR` file are searched for the MDEC frame magic that tells a movie from an audio bank (`wiki: formats/str`).
const STR_PROBE_SECTORS: usize = 32;
/// First payload byte of the STR frame header in a 2336-byte sector: the 8-byte XA subheader precedes it.
const STR_MAGIC_OFFSET: usize = 8;
/// MDEC frame header magic (`wiki: formats/str`).
const STR_MAGIC: [u8; 4] = [0x60, 0x01, 0x01, 0x80];
/// Queued discs files per worker before the reader waits.
const QUEUE_PER_WORKER: usize = 2;
/// File name of the license sectors in the disc tree.
const LICENSE_FILE: &str = "license_data.dat";
/// The `state` of a row that comes from a whole disc file.
const FILE_STATE: &str = "file";
/// The `format` of a row that comes from a whole disc file.
const FILE_FORMAT: &str = "disc file";

/// How a run reads and writes.
#[derive(Clone, Debug)]
pub struct Options {
    /// The CUE sheet or image to read.
    pub image: PathBuf,
    /// The directory the tree is written to.
    pub out: PathBuf,
    pub selection: Selection,
    /// `false` plans the run: nothing is written or decoded.
    pub write: bool,
    /// Worker threads.
    pub jobs: usize,
}

/// A file read from the disc, waiting for a worker.
struct Job {
    source: String,
    bytes: Vec<u8>,
}

/// One written or planned file.
struct Output<'a> {
    role: &'static str,
    path: String,
    data: Cow<'a, [u8]>,
    /// The loader evidence behind the output; empty for the stored bytes.
    evidence: String,
}

/// Reads the disc and writes every selected kind the image holds; the `text` kind reads the extracted disc through [`crate::text::extract`].
pub fn run(options: &Options) -> Result<Report> {
    let mut image = Image::open(&options.image)?;
    let entries = image.entries()?;
    let resident = resident_blocks(&mut image, &entries, &options.selection);
    let sheet = image.cue_sheet()?;
    let license = if options.selection.contains(Kind::System) { image.license()? } else { None };
    let (sender, receiver) = sync_channel::<Job>(options.jobs.max(1) * QUEUE_PER_WORKER);
    let receiver = Mutex::new(receiver);
    let report = Mutex::new(Report::default());

    thread::scope(|scope| -> Result<()> {
        for _ in 0..options.jobs.max(1) {
            scope.spawn(|| worker(&receiver, options, &report, &resident));
        }
        if let Some(license) = license {
            send(&sender, LICENSE_FILE.to_string(), license)?;
        }
        for entry in entries.iter().filter(|entry| !entry.is_directory) {
            if !wanted(entry, &options.selection) {
                continue;
            }
            let bytes = image.read_file(entry)?;
            send(&sender, disc_path(entry), bytes)?;
        }
        drop(sender);
        Ok(())
    })?;

    let mut report = report.into_inner().expect("workers do not panic");
    if let Some(sheet) = sheet.filter(|_| options.selection.contains(Kind::Cdda)) {
        cdda_rows(&sheet, options, &mut report)?;
    }
    let sound_kinds = [Kind::AudioBank, Kind::AudioSequence, Kind::XaAudio];
    if options.write && sound_kinds.iter().any(|kind| options.selection.contains(*kind)) {
        sound_rows(&options.image, options, &mut report)?;
    }
    report.sort();
    Ok(report)
}

fn send(sender: &std::sync::mpsc::SyncSender<Job>, source: String, bytes: Vec<u8>) -> Result<()> {
    sender.send(Job { source, bytes }).map_err(|_| Error::Usage("a worker thread stopped early".into()))
}

fn worker(receiver: &Mutex<Receiver<Job>>, options: &Options, report: &Mutex<Report>, resident: &[ResidentClutBlock]) {
    loop {
        let job = receiver.lock().expect("receiver lock").recv();
        let Ok(job) = job else {
            return;
        };
        let mut local = Report::default();
        if let Err(error) = process(&job, options, &mut local, resident) {
            local.failures.push(format!("{}: {error}", job.source));
        }
        let mut shared = report.lock().expect("report lock");
        shared.rows.append(&mut local.rows);
        shared.failures.append(&mut local.failures);
    }
}

/// The disc path of an entry with forward slashes.
fn disc_path(entry: &Entry) -> String {
    entry.path.to_string_lossy().replace('\\', "/")
}

/// Every CLUT block the selected archives carry, with the archive that loads it.
fn resident_blocks(image: &mut Image, entries: &[Entry], selection: &Selection) -> Vec<ResidentClutBlock> {
    if !(selection.contains(Kind::Texture) || selection.contains(Kind::Palette)) {
        return Vec::new();
    }
    let mut blocks = Vec::new();
    for entry in entries.iter().filter(|entry| !entry.is_directory) {
        let source = disc_path(entry);
        if !is_emi(&source) || !wanted(entry, selection) {
            continue;
        }
        let mut reader = DiscRange::new(image, entry);
        let Ok(archive) = Archive::from_reader(&mut reader) else {
            continue;
        };
        for (slot, slot_entry) in archive.entries().iter().enumerate() {
            if ClutBlock::new(slot, slot_entry.ram_ptr, slot_entry.size as usize).is_none() {
                continue;
            }
            let Ok(payload) = archive.read_payload(&mut reader, slot) else {
                continue;
            };
            blocks.extend(ResidentClutBlock::new(&source, slot, slot_entry.ram_ptr, &payload));
        }
    }
    blocks
}

/// A disc file as a `Read + Seek` source: reads its bytes through the image's sector mapping.
struct DiscRange<'a> {
    image: &'a mut Image,
    entry: &'a Entry,
    position: u64,
    size: u64,
}

impl<'a> DiscRange<'a> {
    fn new(image: &'a mut Image, entry: &'a Entry) -> Self {
        Self { image, entry, position: 0, size: u64::from(entry.size) }
    }
}

impl Read for DiscRange<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let left = self.size.saturating_sub(self.position);
        let want = out.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        if want == 0 {
            return Ok(0);
        }
        let read =
            self.image.read_entry_range(self.entry, self.position, &mut out[..want]).map_err(std::io::Error::other)?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for DiscRange<'_> {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let position = match from {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
            SeekFrom::End(offset) => i128::from(self.size) + i128::from(offset),
        };
        if !(0..=i128::from(u64::MAX)).contains(&position) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek outside the file"));
        }
        self.position = position as u64;
        Ok(self.position)
    }
}

fn is_emi(source: &str) -> bool {
    Path::new(source).extension().is_some_and(|extension| extension.eq_ignore_ascii_case(EMI_EXTENSION))
}

/// Whether any selected kind can come from this file, judged by name alone.
fn wanted(entry: &Entry, selection: &Selection) -> bool {
    let source = disc_path(entry);
    if is_emi(&source) {
        return Kind::ALL.iter().any(|kind| !is_file_kind(*kind) && selection.contains(*kind));
    }
    file_kinds(&source).iter().any(|kind| selection.contains(*kind))
}

/// Kinds the image does not produce: `text` is written from the extracted disc.
fn is_file_kind(kind: Kind) -> bool {
    matches!(kind, Kind::Executable | Kind::System | Kind::Video | Kind::XaAudio | Kind::Cdda | Kind::Text)
}

/// The kinds a non-EMI disc file can have, from its name.
fn file_kinds(source: &str) -> &'static [Kind] {
    let name = source.rsplit('/').next().unwrap_or(source);
    let extension = Path::new(name).extension().map(std::ffi::OsStr::to_ascii_uppercase);
    match extension.as_deref().and_then(|value| value.to_str()) {
        Some("EXE") => &[Kind::Executable],
        Some("CNF") => &[Kind::System],
        Some("STR") => &[Kind::Video, Kind::XaAudio],
        _ if name.starts_with("SLUS_") => &[Kind::Executable],
        _ if name == LICENSE_FILE => &[Kind::System],
        _ => &[Kind::Unknown],
    }
}

/// Whether an XA stream opens with MDEC frames rather than audio only.
fn is_movie(bytes: &[u8]) -> bool {
    bytes
        .chunks(psx_cd::XA_SECTOR_SIZE)
        .take(STR_PROBE_SECTORS)
        .any(|sector| sector.get(STR_MAGIC_OFFSET..STR_MAGIC_OFFSET + STR_MAGIC.len()) == Some(&STR_MAGIC))
}

fn process(job: &Job, options: &Options, report: &mut Report, resident: &[ResidentClutBlock]) -> Result<()> {
    if is_emi(&job.source) {
        match Archive::parse(&job.bytes) {
            Ok(archive) => return emi_rows(job, &archive, options, report, resident),
            Err(error) => {
                let note = format!("not an EMI container: {error}");
                return file_rows(job, Kind::Unknown, &note, options, report);
            }
        }
    }
    let kinds = file_kinds(&job.source);
    let kind = match kinds {
        [Kind::Video, Kind::XaAudio] if is_movie(&job.bytes) => Kind::Video,
        [Kind::Video, Kind::XaAudio] => Kind::XaAudio,
        [kind] => *kind,
        _ => Kind::Unknown,
    };
    if !options.selection.contains(kind) {
        return Ok(());
    }
    file_rows(job, kind, "", options, report)
}

/// A whole disc file, copied unchanged.
fn file_rows(job: &Job, kind: Kind, note: &str, options: &Options, report: &mut Report) -> Result<()> {
    let base = Row {
        source: job.source.clone(),
        slot: None,
        offset: 0,
        size: job.bytes.len() as u64,
        file_type: None,
        load_arg: None,
        format: FILE_FORMAT,
        kind,
        state: FILE_STATE,
        role: "raw",
        output: None,
        bytes: 0,
        sha256: String::new(),
        evidence: String::new(),
        note: note.to_string(),
    };
    let output = Output {
        role: "raw",
        path: format!("{}/{}", kind.name(), job.source),
        data: Cow::Borrowed(&job.bytes),
        evidence: String::new(),
    };
    write_output(&base, output, options, report)
}

/// Every selected slot of one container.
fn emi_rows(
    job: &Job,
    archive: &Archive,
    options: &Options,
    report: &mut Report,
    resident: &[ResidentClutBlock],
) -> Result<()> {
    let stem = job.source.strip_suffix(&format!(".{EMI_EXTENSION}")).unwrap_or(&job.source);
    let mut payloads = Vec::with_capacity(archive.entries().len());
    for slot in 0..archive.entries().len() {
        payloads.push(archive.payload(&job.bytes, slot)?);
    }
    let rows = joined_rows_in(&job.source, archive.entries(), &payloads, resident);
    for (slot, entry) in archive.entries().iter().enumerate() {
        let payload = payloads[slot];
        let format = classify(entry, payload);
        let kind = Kind::of_format(format);
        if slot == 0 && job.source == GAME_TABLES_SOURCE && options.selection.contains(Kind::DataTable) {
            game_table_rows(job, entry, payload, options, report)?;
        }
        if !options.selection.contains(kind) {
            continue;
        }
        let base = Row {
            source: job.source.clone(),
            slot: Some(slot),
            offset: entry.offset,
            size: u64::from(entry.size),
            file_type: Some(entry.file_type),
            load_arg: Some(entry.ram_ptr),
            format: format.name(),
            kind,
            state: format.state().label(),
            role: "raw",
            output: None,
            bytes: 0,
            sha256: String::new(),
            evidence: String::new(),
            note: String::new(),
        };
        if format == Format::Reserve {
            let mut row = base;
            row.role = "none";
            report.rows.push(row);
            continue;
        }
        let raw_extension = classify::extension(format);
        let name = |tag: &str, extension: &str| format!("{}/{stem}/{slot}{tag}.{extension}", kind.name());
        let mut outputs = vec![Output {
            role: "raw",
            path: name("", raw_extension),
            data: Cow::Borrowed(payload),
            evidence: String::new(),
        }];
        let mut note = String::new();
        if options.write {
            match derive(format, entry.ram_ptr, payload) {
                Ok(derived) => outputs.extend(derived.into_iter().map(|file| Output {
                    role: file.role,
                    path: name("", file.extension),
                    data: Cow::Owned(file.data),
                    evidence: String::new(),
                })),
                Err(error) => note = format!("derived output skipped: {error}"),
            }
            if let Some(open) = format.describe().open {
                let site = Site {
                    stem,
                    slot,
                    entries: archive.entries(),
                    payloads: &payloads,
                    rows: rows.get(&slot).map_or(&[], Vec::as_slice),
                };
                let opened = open(&site);
                outputs.extend(opened.files.into_iter().map(|file| Output {
                    role: file.role,
                    path: name(&file.tag, file.extension),
                    data: Cow::Owned(file.data),
                    evidence: file.evidence,
                }));
                if let Some(skipped) = opened.skipped {
                    note = skipped;
                }
            }
        }
        for (position, output) in outputs.into_iter().enumerate() {
            let mut row = base.clone();
            if position == 0 {
                row.note.clone_from(&note);
            }
            write_output(&row, output, options, report)?;
        }
    }
    Ok(())
}

/// Writes the fixed game tables of entry 0 of `ETC/GAME.EMI` as one JSON file per table.
fn game_table_rows(
    job: &Job,
    entry: &rebof3_emi::Entry,
    payload: &[u8],
    options: &Options,
    report: &mut Report,
) -> Result<()> {
    let files = match rebof3_formats::game_tables::open::files(payload) {
        Ok(files) => files,
        Err(error) => {
            report.failures.push(format!("{} entry 0 tables: {error}", job.source));
            return Ok(());
        }
    };
    let stored = match rebof3_formats::game_tables::open::stored(payload) {
        Ok(stored) => stored,
        Err(error) => {
            report.failures.push(format!("{} entry 0 tables: {error}", job.source));
            return Ok(());
        }
    };
    let stem = job.source.strip_suffix(&format!(".{EMI_EXTENSION}")).unwrap_or(&job.source);
    for ((name, data), (table, bytes)) in files.into_iter().zip(stored) {
        let base = Row {
            source: job.source.clone(),
            slot: Some(0),
            offset: entry.offset,
            size: u64::from(entry.size),
            file_type: Some(entry.file_type),
            load_arg: Some(entry.ram_ptr),
            format: DERIVED_TABLE,
            kind: Kind::DataTable,
            state: "codec",
            role: "json",
            output: None,
            bytes: 0,
            sha256: String::new(),
            evidence: String::new(),
            note: String::new(),
        };
        let original = Row { offset: table.archive_offset as u64, size: bytes.len() as u64, ..base.clone() };
        let output = Output {
            role: "raw",
            path: format!("{}/{stem}/{name}.bin", Kind::DataTable.name()),
            data: Cow::Borrowed(bytes),
            evidence: String::new(),
        };
        write_output(&original, output, options, report)?;
        let output = Output {
            role: "json",
            path: format!("{}/{stem}/{name}.json", Kind::DataTable.name()),
            data: Cow::Owned(data),
            evidence: TABLE_EVIDENCE.to_string(),
        };
        write_output(&base, output, options, report)?;
    }
    Ok(())
}

/// Whether the asset a row belongs to has a conversion.
fn converts(base: &Row) -> bool {
    match base.kind {
        Kind::Cdda
        | Kind::Texture
        | Kind::Palette
        | Kind::AudioBank
        | Kind::AudioSequence
        | Kind::AudioCues
        | Kind::AreaActorDefs
        | Kind::AreaEncounters
        | Kind::AreaMap
        | Kind::DataTable
        | Kind::XaAudio => true,
        Kind::CharacterSprites => base.format == Format::CharacterSpritesLz77.name(),
        _ => false,
    }
}

/// Hashes `output`, writes it unless the run only plans, and records its row.
fn write_output(base: &Row, output: Output<'_>, options: &Options, report: &mut Report) -> Result<()> {
    if !options.selection.mode().writes(output.role, converts(base)) {
        return Ok(());
    }
    let mut row = base.clone();
    row.role = output.role;
    row.bytes = output.data.len() as u64;
    row.output = Some(output.path.clone());
    row.evidence = output.evidence;
    if options.write {
        row.sha256 = sha256_hex(&output.data);
        let path = options.out.join(&output.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &output.data)?;
    }
    report.rows.push(row);
    Ok(())
}

/// The sound items `bof3player` renders from the disc image in the selected audio format.
fn sound_rows(image: &Path, options: &Options, report: &mut Report) -> Result<()> {
    let library = match sound::open(image) {
        Ok(library) => library,
        Err(error) => {
            report.failures.push(format!("sound: {error}"));
            return Ok(());
        }
    };
    let failures = sound::render_with_jobs(
        &library,
        |kind| options.selection.contains(kind),
        options.selection.mode(),
        options.selection.audio(),
        options.jobs,
        |clip| {
            let base = Row {
                source: clip.source,
                slot: None,
                offset: 0,
                size: 0,
                file_type: None,
                load_arg: None,
                format: RENDERED_ITEM,
                kind: clip.kind,
                state: "codec",
                role: "raw",
                output: None,
                bytes: 0,
                sha256: String::new(),
                evidence: String::new(),
                note: String::new(),
            };
            let output = Output {
                role: clip.extension,
                path: clip.output.clone(),
                data: Cow::Owned(clip.bytes),
                evidence: clip.evidence.clone(),
            };
            write_output(&base, output, options, report)?;
            if let Some(midi) = clip.midi {
                let path = std::path::Path::new(&clip.output).with_extension("mid");
                let output = Output {
                    role: "midi",
                    path: path.to_string_lossy().into_owned(),
                    data: Cow::Owned(midi),
                    evidence: clip.evidence,
                };
                write_output(&base, output, options, report)?;
            }
            if let Some(original) = clip.original {
                let output = Output {
                    role: "vag",
                    path: original.output,
                    data: Cow::Owned(original.bytes),
                    evidence: String::new(),
                };
                write_output(&base, output, options, report)?;
            }
            Ok(())
        },
    )?;
    report.failures.extend(failures);
    Ok(())
}

/// The audio tracks of the CUE sheet: the sectors as the disc stores them (`.pcm`) and the selected audio container.
fn cdda_rows(sheet: &cue::Sheet, options: &Options, report: &mut Report) -> Result<()> {
    let directory = options.out.join(Kind::Cdda.name());
    let mode = options.selection.mode();
    let audio = options.selection.audio();
    let audio_extension = audio.extension();
    let forms = [("raw", "pcm"), (audio_extension, audio_extension)]
        .map(|(role, extension)| (role, extension, mode.writes(role, true)));
    let mut written = Vec::new();
    let mut temporary = None;
    if options.write {
        if forms[0].2 {
            written.extend(cue::extract_raw_audio_tracks(sheet, &directory)?);
        }
        if forms[1].2 {
            if audio == AudioFormat::Wav {
                written.extend(cue::extract_audio_tracks(sheet, &directory)?);
            } else {
                let source = directory.join(".wav-source");
                cue::extract_audio_tracks(sheet, &source)?;
                temporary = Some(source.clone());
                for track in sheet.tracks.iter().filter(|track| track.mode == cue::TrackMode::Audio) {
                    let wav_path = source.join(format!("track{:02}.wav", track.number));
                    let wav = fs::read(&wav_path)?;
                    let info = psx_formats::wav::parse(&wav)?;
                    let start = info.data_offset as usize;
                    let end = start + info.data_bytes as usize;
                    let pcm: Vec<i16> =
                        wav[start..end].as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect();
                    let title = format!("Track {:02}", track.number);
                    let tags = bof3player::export::Tags {
                        title: &title,
                        artist: "Breath of Fire III",
                        album: "Breath of Fire III",
                        track: Some(u32::from(track.number)),
                        loop_start: None,
                        loop_length: None,
                    };
                    let bytes = sound::encode(audio, &pcm, info.rate, info.channels, &tags)?;
                    let path = directory.join(format!("track{:02}.{audio_extension}", track.number));
                    fs::write(&path, bytes)?;
                    written.push(path);
                }
            }
        }
    }
    for (position, track) in sheet.tracks.iter().enumerate().filter(|(_, track)| track.mode == cue::TrackMode::Audio) {
        let stem = format!("track{:02}", track.number);
        let size =
            cue::audio_track_range(sheet, position).map_or(0, |(start, end)| (end - start) * cue::AUDIO_SECTOR_SIZE);
        for (role, extension, wanted) in forms {
            if !wanted {
                continue;
            }
            let name = format!("{stem}.{extension}");
            let path = directory.join(&name);
            let (bytes, sha256) = if written.contains(&path) {
                let data = fs::read(&path)?;
                (data.len() as u64, sha256_hex(&data))
            } else {
                (0, String::new())
            };
            report.rows.push(Row {
                source: stem.clone(),
                slot: None,
                offset: 0,
                size,
                file_type: None,
                load_arg: None,
                format: FILE_FORMAT,
                kind: Kind::Cdda,
                state: FILE_STATE,
                role,
                output: Some(format!("{}/{name}", Kind::Cdda.name())),
                bytes,
                sha256,
                evidence: String::new(),
                note: String::new(),
            });
        }
    }
    if let Some(source) = temporary {
        fs::remove_dir_all(source)?;
    }
    Ok(())
}
