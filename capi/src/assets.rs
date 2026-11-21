//! The asset codec and extractor functions.

use std::ffi::{CString, c_char};
use std::path::PathBuf;
use std::sync::LazyLock;

use ::rebof3_assets as assets_lib;
use assets_lib::codec::{self, Direction, Index};
use assets_lib::report::Report;
use assets_lib::types::{AssetType, AudioFormat, Mode, Selection};

use psx_capi::{handle_ref, input, opt_out, opt_utf8, out_ref, owned_buffer, owned_string, psx_buffer, psx_status, psx_string, utf8};

use crate::error::{Failure, guarded, invalid, unsupported, usage};

/// The direction of a codec conversion.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum rebof3_direction {
    Export = 0,
    Import = 1,
}

/// How a codec conversion uses its index.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum rebof3_index {
    None = 0,
    Optional = 1,
    Required = 2,
}

/// Which outputs an extraction writes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum rebof3_mode {
    Both = 0,
    Raw = 1,
    Modern = 2,
}

/// The container rendered audio is written in.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum rebof3_audio {
    Wav = 0,
    Flac = 1,
    Ogg = 2,
}

/// The files and options of one codec conversion.
///
/// Borrowed: read during the call only.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct rebof3_assets_job {
    pub input: *const u8,
    pub input_len: usize,
    pub companion: *const u8,
    pub companion_len: usize,
    pub name: *const c_char,
    pub name_len: usize,
    pub index: *const u32,
    pub load_arg: *const u32,
}

/// One codec and the direction's requirements.
///
/// Borrowed: the strings live for the process.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct rebof3_assets_codec {
    pub asset_type: *const c_char,
    pub files: u32,
    pub named: bool,
    pub load_arg: bool,
    pub index: rebof3_index,
    pub extension: *const c_char,
}

/// The inputs and outputs of an asset extraction.
///
/// Borrowed: read during the call only. `jobs` 0 uses every core; `write` false plans the run.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct rebof3_assets_options {
    pub image_path: *const c_char,
    pub image_path_len: usize,
    pub output_dir: *const c_char,
    pub output_dir_len: usize,
    pub types: *const c_char,
    pub types_len: usize,
    pub text_disc: *const c_char,
    pub text_disc_len: usize,
    pub mode: rebof3_mode,
    pub audio: rebof3_audio,
    pub write: bool,
    pub jobs: usize,
}

fn direction_of(value: rebof3_direction) -> Direction {
    match value {
        rebof3_direction::Export => Direction::Export,
        rebof3_direction::Import => Direction::Import,
    }
}

fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Export => "export",
        Direction::Import => "import",
    }
}

fn index_value(index: Index) -> rebof3_index {
    match index {
        Index::No => rebof3_index::None,
        Index::Optional => rebof3_index::Optional,
        Index::Required(_) => rebof3_index::Required,
    }
}

fn mode_of(value: rebof3_mode) -> Mode {
    match value {
        rebof3_mode::Both => Mode::Both,
        rebof3_mode::Raw => Mode::Raw,
        rebof3_mode::Modern => Mode::Modern,
    }
}

fn audio_of(value: rebof3_audio) -> AudioFormat {
    match value {
        rebof3_audio::Wav => AudioFormat::Wav,
        rebof3_audio::Flac => AudioFormat::Flac,
        rebof3_audio::Ogg => AudioFormat::Ogg,
    }
}

fn cstring(text: &str) -> CString {
    CString::new(text.as_bytes()).expect("a codec label has no NUL")
}

struct CodecStrings {
    asset_type: CString,
    export: Option<CString>,
    import: Option<CString>,
}

static CODEC_STRINGS: LazyLock<Vec<CodecStrings>> = LazyLock::new(|| {
    codec::CODECS
        .iter()
        .map(|codec| CodecStrings {
            asset_type: cstring(codec.asset_type),
            export: codec.export.as_ref().map(|op| cstring(op.extension)),
            import: codec.import.as_ref().map(|op| cstring(op.extension)),
        })
        .collect()
});

unsafe fn optional_u32(pointer: *const u32) -> Option<u32> {
    if pointer.is_null() { None } else { Some(unsafe { *pointer }) }
}

/// Reads the number of asset codecs.
///
/// # Safety
/// `out_count` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_assets_codec_count(out_count: *mut usize) -> psx_status {
    guarded(|| {
        let out_count = unsafe { out_ref(out_count, "out_count")? };
        *out_count = codec::CODECS.len();
        Ok(())
    })
}

/// Reads one codec's requirements in one direction.
///
/// # Safety
/// `asset_type` must point to `asset_type_len` readable UTF-8 bytes and `out_codec` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_assets_codec_info(asset_type: *const c_char, asset_type_len: usize, direction: rebof3_direction, out_codec: *mut rebof3_assets_codec) -> psx_status {
    guarded(|| {
        let asset_type = unsafe { utf8(asset_type, asset_type_len, "type")? };
        let direction = direction_of(direction);
        let out_codec = unsafe { out_ref(out_codec, "out_codec")? };
        let codec = codec::find(asset_type).ok_or_else(|| unsupported(format!("no codec named {asset_type:?}")))?;
        let op = codec.op(direction).ok_or_else(|| unsupported(format!("{asset_type:?} has no {} conversion", direction_name(direction))))?;
        let position = codec::CODECS.iter().position(|candidate| candidate.asset_type == codec.asset_type).expect("find returns a codec of CODECS");
        let strings = &CODEC_STRINGS[position];
        let extension = match direction {
            Direction::Export => strings.export.as_ref(),
            Direction::Import => strings.import.as_ref(),
        }
        .expect("the op exists");
        *out_codec = rebof3_assets_codec {
            asset_type: strings.asset_type.as_ptr(),
            files: u32::try_from(op.files).unwrap_or(u32::MAX),
            named: op.named,
            load_arg: op.load_arg,
            index: index_value(op.index),
            extension: extension.as_ptr(),
        };
        Ok(())
    })
}

/// Runs one codec conversion from the supplied bytes.
///
/// # Safety
/// `asset_type` must point to `asset_type_len` readable UTF-8 bytes, `job` must be a readable job whose
/// pointers are readable for their lengths, and `out_bytes` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_assets_convert(asset_type: *const c_char, asset_type_len: usize, direction: rebof3_direction, job: *const rebof3_assets_job, out_bytes: *mut psx_buffer) -> psx_status {
    guarded(|| {
        let asset_type = unsafe { utf8(asset_type, asset_type_len, "type")? };
        let direction = direction_of(direction);
        let job = unsafe { handle_ref(job, "job")? };
        let out_bytes = unsafe { out_ref(out_bytes, "out_bytes")? };
        let codec = codec::find(asset_type).ok_or_else(|| unsupported(format!("no codec named {asset_type:?}")))?;
        let op = codec.op(direction).ok_or_else(|| unsupported(format!("{asset_type:?} has no {} conversion", direction_name(direction))))?;
        if job.input.is_null() {
            return Err(invalid(op.needs_input));
        }
        let input_bytes = unsafe { input(job.input, job.input_len, "job input")? };
        let name = unsafe { opt_utf8(job.name, job.name_len, "job name")? };
        let companion = if job.companion.is_null() && job.companion_len == 0 { None } else { Some(unsafe { input(job.companion, job.companion_len, "job companion")? }) };
        let index = unsafe { optional_u32(job.index) };
        let load_arg = unsafe { optional_u32(job.load_arg) };
        let job = codec::Job { name, input: input_bytes, companion, index, load_arg };
        let bytes = op.run_checked(asset_type, &job)?;
        *out_bytes = owned_buffer(bytes);
        Ok(())
    })
}

/// Runs the extraction `options` select and returns its `assets.jsonl` and summary text.
///
/// # Safety
/// The pointers of `options` must be readable for their lengths.
unsafe fn extract_report(options: &rebof3_assets_options) -> Result<(String, String), Failure> {
    let image = unsafe { utf8(options.image_path, options.image_path_len, "image_path")? };
    let out_dir = unsafe { utf8(options.output_dir, options.output_dir_len, "output_dir")? };
    let types = unsafe { opt_utf8(options.types, options.types_len, "types")? };
    let text_disc = unsafe { opt_utf8(options.text_disc, options.text_disc_len, "text_disc")? };
    let selection = match types {
        Some(types) => Selection::parse(types)?,
        None => Selection::all(),
    };
    let selection = selection.with_mode(mode_of(options.mode)).with_audio(audio_of(options.audio));
    let explicit_text = types.is_some() && selection.contains(AssetType::Text);
    if explicit_text && text_disc.is_none() {
        return Err(usage("the text type needs text_disc"));
    }
    let wants_text = if types.is_some() { explicit_text } else { text_disc.is_some() };
    let write = options.write;
    let out = PathBuf::from(out_dir);
    let image = PathBuf::from(image);
    let jobs = if options.jobs == 0 { std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get) } else { options.jobs };
    let only_text = selection.is_only(AssetType::Text);
    let mut report =
        if only_text { Report::default() } else { assets_lib::extract::run(&assets_lib::extract::Options { image: image.clone(), out: out.clone(), selection: selection.clone(), write, jobs })? };
    if wants_text {
        let disc = text_disc.ok_or_else(|| usage("the text type needs text_disc"))?;
        let text = assets_lib::text::extract_with(&assets_lib::text::Options {
            disc: PathBuf::from(disc),
            out: out.clone(),
            image: Some(image),
            raw: selection.mode().writes("raw", true),
            modern: selection.mode().writes("json", true),
            write,
            list: false,
        })?;
        report.rows.extend(text.rows);
        report.failures.extend(text.failures);
        report.notes.extend(text.notes);
    }
    report.sort();
    if write {
        report.write_tables(&out)?;
    }
    Ok((report.jsonl(), report.summary()))
}

/// Runs the selected asset extraction and returns its `assets.jsonl` and summary.
///
/// # Safety
/// `options` must be a readable options record whose pointers are readable for their lengths;
/// the out pointers may be NULL to skip that result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_assets_extract(options: *const rebof3_assets_options, out_report: *mut psx_string, out_summary: *mut psx_string) -> psx_status {
    guarded(|| {
        let options = unsafe { handle_ref(options, "options")? };
        let report_out = unsafe { opt_out(out_report) };
        let summary_out = unsafe { opt_out(out_summary) };
        let (jsonl, summary) = unsafe { extract_report(options)? };
        if let Some(report_out) = report_out {
            *report_out = owned_string(jsonl);
        }
        if let Some(summary_out) = summary_out {
            *summary_out = owned_string(summary);
        }
        Ok(())
    })
}
