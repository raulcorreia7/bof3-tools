//! The asset codec and extractor functions.

use std::ffi::{CString, c_char};
use std::path::PathBuf;
use std::sync::LazyLock;

use ::rebof3_assets as assets_lib;
use assets_lib::codec::{self, Direction, Index};
use assets_lib::kind::{AudioFormat, Kind, Mode, Selection};
use assets_lib::report::Report;

use crate::error::{guarded, invalid, rebof3_status, unsupported, usage};
use crate::ffi::{input, optional_out, optional_utf8, out_ref, utf8};
use crate::{rebof3_buffer, rebof3_string};

/// The direction of a codec conversion.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// The variants are passed in from C, which the Rust dead-code pass cannot see.
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
// The variants are passed in from C, which the Rust dead-code pass cannot see.
#[allow(dead_code)]
pub enum rebof3_mode {
    Both = 0,
    Raw = 1,
    Modern = 2,
}

/// The container rendered audio is written in.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// The variants are passed in from C, which the Rust dead-code pass cannot see.
#[allow(dead_code)]
pub enum rebof3_audio {
    Wav = 0,
    Flac = 1,
    Ogg = 2,
}

/// The files and options of one codec conversion.
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
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct rebof3_assets_codec {
    pub kind: *const c_char,
    pub files: u32,
    pub named: u32,
    pub load_arg: u32,
    pub index: rebof3_index,
    pub extension: *const c_char,
}

/// The inputs and outputs of an asset extraction.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct rebof3_assets_options {
    pub image_path: *const c_char,
    pub image_path_len: usize,
    pub output_dir: *const c_char,
    pub output_dir_len: usize,
    pub kinds: *const c_char,
    pub kinds_len: usize,
    pub text_disc: *const c_char,
    pub text_disc_len: usize,
    pub mode: rebof3_mode,
    pub audio: rebof3_audio,
    pub write: u32,
    pub reserved: u32,
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
    kind: CString,
    export: Option<CString>,
    import: Option<CString>,
}

static CODEC_STRINGS: LazyLock<Vec<CodecStrings>> = LazyLock::new(|| {
    codec::CODECS
        .iter()
        .map(|codec| CodecStrings { kind: cstring(codec.kind), export: codec.export.as_ref().map(|op| cstring(op.extension)), import: codec.import.as_ref().map(|op| cstring(op.extension)) })
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
pub unsafe extern "C" fn rebof3_assets_codec_count(out_count: *mut usize) -> rebof3_status {
    match guarded(|| {
        let out_count = unsafe { out_ref(out_count, "out_count")? };
        *out_count = codec::CODECS.len();
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Reads one codec's requirements in one direction.
///
/// # Safety
/// `kind` must point to `kind_len` readable UTF-8 bytes and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_assets_codec_info(kind: *const c_char, kind_len: usize, direction: rebof3_direction, out: *mut rebof3_assets_codec) -> rebof3_status {
    match guarded(|| {
        let kind = unsafe { utf8(kind, kind_len, "kind")? };
        let direction = direction_of(direction);
        let out = unsafe { out_ref(out, "out")? };
        let codec = codec::find(kind).ok_or_else(|| unsupported(format!("no codec named {kind:?}")))?;
        let op = codec.op(direction).ok_or_else(|| unsupported(format!("{kind:?} has no {} conversion", direction_name(direction))))?;
        let position = codec::CODECS.iter().position(|candidate| candidate.kind == codec.kind).expect("find returns a codec of CODECS");
        let strings = &CODEC_STRINGS[position];
        let extension = match direction {
            Direction::Export => strings.export.as_ref(),
            Direction::Import => strings.import.as_ref(),
        }
        .expect("the op exists");
        *out = rebof3_assets_codec {
            kind: strings.kind.as_ptr(),
            files: u32::try_from(op.files).unwrap_or(u32::MAX),
            named: u32::from(op.named),
            load_arg: u32::from(op.load_arg),
            index: index_value(op.index),
            extension: extension.as_ptr(),
        };
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Runs one codec conversion from the supplied bytes.
///
/// # Safety
/// `kind` must point to `kind_len` readable UTF-8 bytes, `job` must be a readable job whose
/// pointers are readable for their lengths, and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_assets_convert(kind: *const c_char, kind_len: usize, direction: rebof3_direction, job: *const rebof3_assets_job, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let kind = unsafe { utf8(kind, kind_len, "kind")? };
        let direction = direction_of(direction);
        let job = unsafe { job.as_ref() }.ok_or_else(|| invalid("the job pointer is NULL"))?;
        let out = unsafe { out_ref(out, "out")? };
        let codec = codec::find(kind).ok_or_else(|| unsupported(format!("no codec named {kind:?}")))?;
        let op = codec.op(direction).ok_or_else(|| unsupported(format!("{kind:?} has no {} conversion", direction_name(direction))))?;
        if job.input.is_null() {
            return Err(invalid(op.needs_input));
        }
        let input_bytes = unsafe { input(job.input, job.input_len, "job input")? };
        let name = unsafe { optional_utf8(job.name, job.name_len, "job name")? };
        let companion = if job.companion.is_null() && job.companion_len == 0 { None } else { Some(unsafe { input(job.companion, job.companion_len, "job companion")? }) };
        let index = unsafe { optional_u32(job.index) };
        let load_arg = unsafe { optional_u32(job.load_arg) };
        if op.named && name.is_none() {
            return Err(invalid(op.needs_input));
        }
        if !op.named && name.is_some() {
            return Err(invalid(format!("the {kind} conversion takes no name")));
        }
        if op.files >= 2 && companion.is_none() {
            return Err(invalid(op.needs_input));
        }
        if let Some(flag) = op.companion_flag {
            if companion.is_none() {
                return Err(invalid(flag.missing));
            }
        } else if op.files < 2 && companion.is_some() {
            return Err(invalid(format!("the {kind} conversion takes no companion")));
        }
        if op.index == Index::No && index.is_some() {
            return Err(invalid(format!("the {kind} conversion takes no index")));
        }
        if let Index::Required(missing) = op.index
            && index.is_none()
        {
            return Err(invalid(missing));
        }
        if !op.load_arg && load_arg.is_some() {
            return Err(invalid(format!("the {kind} conversion takes no load argument")));
        }
        let job = codec::Job { name, input: input_bytes, companion, index, load_arg };
        let bytes = (op.run)(&job)?;
        *out = rebof3_buffer::from_vec(bytes);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Runs the selected asset extraction and returns its `assets.jsonl` and summary.
///
/// # Safety
/// `options` must be a readable options record whose pointers are readable for their lengths;
/// the out pointers may be NULL to skip that result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_assets_extract(options: *const rebof3_assets_options, report_jsonl: *mut rebof3_string, summary: *mut rebof3_string) -> rebof3_status {
    match guarded(|| {
        let options = unsafe { options.as_ref() }.ok_or_else(|| invalid("the options pointer is NULL"))?;
        let image = unsafe { utf8(options.image_path, options.image_path_len, "image_path")? };
        let out_dir = unsafe { utf8(options.output_dir, options.output_dir_len, "output_dir")? };
        let kinds = unsafe { optional_utf8(options.kinds, options.kinds_len, "kinds")? };
        let text_disc = unsafe { optional_utf8(options.text_disc, options.text_disc_len, "text_disc")? };
        let report_out = unsafe { optional_out(report_jsonl) };
        let summary_out = unsafe { optional_out(summary) };

        let selection = match kinds {
            Some(kinds) => Selection::parse(kinds)?,
            None => Selection::all(),
        };
        let selection = selection.with_mode(mode_of(options.mode)).with_audio(audio_of(options.audio));
        let explicit_text = kinds.is_some() && selection.contains(Kind::Text);
        if explicit_text && text_disc.is_none() {
            return Err(usage("the text kind needs text_disc"));
        }
        let wants_text = if kinds.is_some() { explicit_text } else { text_disc.is_some() };
        let write = options.write != 0;
        let out = PathBuf::from(out_dir);
        let image = PathBuf::from(image);
        let jobs = if options.jobs == 0 { std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get) } else { options.jobs };
        let only_text = selection.is_only(Kind::Text);
        let mut report =
            if only_text { Report::default() } else { assets_lib::extract::run(&assets_lib::extract::Options { image: image.clone(), out: out.clone(), selection: selection.clone(), write, jobs })? };
        if wants_text {
            let disc = text_disc.ok_or_else(|| usage("the text kind needs text_disc"))?;
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
        let jsonl = report.jsonl();
        let summary = report.summary();
        if write {
            std::fs::create_dir_all(&out)?;
            std::fs::write(out.join(assets_lib::extract::TABLE_FILE), &jsonl)?;
            std::fs::write(out.join(assets_lib::extract::SUMMARY_FILE), &summary)?;
        }
        if let Some(report_out) = report_out {
            *report_out = rebof3_string::from_string(jsonl);
        }
        if let Some(summary_out) = summary_out {
            *summary_out = rebof3_string::from_string(summary);
        }
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}
