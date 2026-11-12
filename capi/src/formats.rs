//! The entry-format functions: classification, metadata and the common image and sound outputs.

use std::ffi::{CString, c_char};
use std::sync::LazyLock;

use rebof3_formats::Format;

use crate::emi::{entry_from_view, rebof3_emi_entry};
use crate::error::{Failure, guarded, invalid, rebof3_status, unsupported};
use crate::ffi::{array, input, out_ref, utf8};
use crate::rebof3_buffer;

/// The format of one entry, in the registry's order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum rebof3_format {
    VramUpload = 0,
    ClutBlock = 1,
    VabHeader = 2,
    VabBody = 3,
    SepPackage = 4,
    CharacterSpritesLz77 = 5,
    CharacterSprites = 6,
    AreaMessageTable = 7,
    AreaCellGrid = 8,
    AreaScriptTable = 9,
    BattleScriptTable = 10,
    AreaActorDefsTable = 11,
    AreaEncounterTable = 12,
    AreaMapBundle = 13,
    AreaAuxWindow = 14,
    SharedResourceTable = 15,
    CueTable = 16,
    MagicScriptTable = 17,
    CodeImage = 18,
    Reserve = 19,
    Unclassified = 20,
}

impl rebof3_format {
    fn from_index(index: usize) -> Option<Self> {
        Some(match index {
            0 => Self::VramUpload,
            1 => Self::ClutBlock,
            2 => Self::VabHeader,
            3 => Self::VabBody,
            4 => Self::SepPackage,
            5 => Self::CharacterSpritesLz77,
            6 => Self::CharacterSprites,
            7 => Self::AreaMessageTable,
            8 => Self::AreaCellGrid,
            9 => Self::AreaScriptTable,
            10 => Self::BattleScriptTable,
            11 => Self::AreaActorDefsTable,
            12 => Self::AreaEncounterTable,
            13 => Self::AreaMapBundle,
            14 => Self::AreaAuxWindow,
            15 => Self::SharedResourceTable,
            16 => Self::CueTable,
            17 => Self::MagicScriptTable,
            18 => Self::CodeImage,
            19 => Self::Reserve,
            20 => Self::Unclassified,
            _ => return None,
        })
    }
}

/// What one format is: its names, its state and the asset kind it is written under.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct rebof3_format_info {
    pub name: *const c_char,
    pub extension: *const c_char,
    pub state: *const c_char,
    pub spec_state: *const c_char,
    pub codec_state: *const c_char,
    pub kind: *const c_char,
}

/// The PNG and JSON of a character sprite sheet.
#[repr(C)]
#[derive(Debug)]
pub struct rebof3_sprite_sheet {
    pub json: rebof3_buffer,
    pub png: rebof3_buffer,
}

const FORMAT_COUNT: usize = 21;

fn format_of(value: rebof3_format) -> Result<Format, Failure> {
    Format::ALL.get(value as usize).copied().ok_or_else(|| invalid("the format value is out of range"))
}

fn format_value(format: Format) -> rebof3_format {
    let index = Format::ALL.iter().position(|candidate| *candidate == format).expect("every format is in ALL");
    rebof3_format::from_index(index).expect("the format table has every value")
}

struct InfoStrings {
    name: CString,
    extension: CString,
    state: CString,
    spec_state: CString,
    codec_state: CString,
    kind: CString,
}

/// The NUL-terminated metadata of every format, valid for the process.
static INFO_STRINGS: LazyLock<[InfoStrings; FORMAT_COUNT]> = LazyLock::new(|| {
    std::array::from_fn(|index| {
        let description = Format::ALL[index].describe();
        InfoStrings {
            name: CString::new(description.name.as_bytes()).expect("a format name has no NUL"),
            extension: CString::new(description.extension.as_bytes()).expect("a format extension has no NUL"),
            state: CString::new(description.state.label().as_bytes()).expect("a format state has no NUL"),
            spec_state: CString::new(description.spec_state.as_bytes()).expect("a specification state has no NUL"),
            codec_state: CString::new(description.codec_state.as_bytes()).expect("a codec state has no NUL"),
            kind: CString::new(description.kind.as_bytes()).expect("a kind name has no NUL"),
        }
    })
});

/// Classifies one entry from its table record and payload.
///
/// # Safety
/// `entry` must be a readable record and `payload` must point to `payload_len` readable bytes;
/// `out_format` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_format_classify(entry: *const rebof3_emi_entry, payload: *const u8, payload_len: usize, out_format: *mut rebof3_format) -> rebof3_status {
    match guarded(|| {
        let entry = unsafe { entry.as_ref() }.ok_or_else(|| invalid("the entry pointer is NULL"))?;
        let payload = unsafe { input(payload, payload_len, "payload")? };
        let out_format = unsafe { out_ref(out_format, "out_format")? };
        *out_format = format_value(rebof3_formats::classify(&entry_from_view(entry), payload));
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Reads one format's metadata. The strings are static and NUL-terminated.
///
/// # Safety
/// `out_info` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_format_info_of(format: rebof3_format, out_info: *mut rebof3_format_info) -> rebof3_status {
    match guarded(|| {
        let index = format as usize;
        let _ = format_of(format)?;
        let out_info = unsafe { out_ref(out_info, "out_info")? };
        let info = &INFO_STRINGS[index];
        *out_info = rebof3_format_info {
            name: info.name.as_ptr(),
            extension: info.extension.as_ptr(),
            state: info.state.as_ptr(),
            spec_state: info.spec_state.as_ptr(),
            codec_state: info.codec_state.as_ptr(),
            kind: info.kind.as_ptr(),
        };
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes the 16 bpp PNG view of a type-3 VRAM upload.
///
/// # Safety
/// `payload` must point to `payload_len` readable bytes and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_texture_png(load_arg: u32, payload: *const u8, payload_len: usize, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let payload = unsafe { input(payload, payload_len, "payload")? };
        let out = unsafe { out_ref(out, "out")? };
        *out = rebof3_buffer::from_vec(rebof3_formats::vram::texture_view_png(load_arg, payload)?);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes the 15 bpp TIM file of a type-3 VRAM upload.
///
/// # Safety
/// `payload` must point to `payload_len` readable bytes and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_texture_tim(load_arg: u32, payload: *const u8, payload_len: usize, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let payload = unsafe { input(payload, payload_len, "payload")? };
        let out = unsafe { out_ref(out, "out")? };
        *out = rebof3_buffer::from_vec(rebof3_formats::vram::texture_tim_direct(load_arg, payload)?);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes the indexed PNG of a texture read in one palette row.
///
/// # Safety
/// `payload` must point to `payload_len` readable bytes, `palette` must point to `palette_len`
/// readable 16-bit words, and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_texture_row_png(load_arg: u32, payload: *const u8, payload_len: usize, bits: u32, palette: *const u16, palette_len: usize, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let payload = unsafe { input(payload, payload_len, "payload")? };
        let palette = unsafe { array(palette, palette_len, "palette")? };
        let out = unsafe { out_ref(out, "out")? };
        *out = rebof3_buffer::from_vec(rebof3_formats::vram::texture_row_png(load_arg, payload, bits, palette)?);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes the JSON of a CLUT block.
///
/// # Safety
/// `payload` must point to `payload_len` readable bytes and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_palette_json(payload: *const u8, payload_len: usize, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let payload = unsafe { input(payload, payload_len, "payload")? };
        let out = unsafe { out_ref(out, "out")? };
        *out = rebof3_buffer::from_vec(rebof3_formats::clut::export(payload)?);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes the swatch-strip PNG of a CLUT block.
///
/// # Safety
/// `payload` must point to `payload_len` readable bytes and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_palette_swatch_png(payload: *const u8, payload_len: usize, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let payload = unsafe { input(payload, payload_len, "payload")? };
        let out = unsafe { out_ref(out, "out")? };
        let derive = Format::ClutBlock.describe().derive.ok_or_else(|| unsupported("the CLUT block format has no derived files"))?;
        let png = derive(0, payload)?.into_iter().find(|file| file.role == "png").ok_or_else(|| unsupported("the CLUT block format has no PNG"))?;
        *out = rebof3_buffer::from_vec(png.data);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes one VAG stream as a 44.1 kHz mono 16-bit WAVE file.
///
/// # Safety
/// `vag` must point to `vag_len` readable bytes and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_vag_wav(vag: *const u8, vag_len: usize, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let vag = unsafe { input(vag, vag_len, "vag")? };
        let out = unsafe { out_ref(out, "out")? };
        *out = rebof3_buffer::from_vec(rebof3_formats::vab::vag::vag_wav(vag)?);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes one sample of a VAB bank as a 44.1 kHz mono 16-bit WAVE file.
///
/// # Safety
/// `header` and `body` must point to their readable lengths and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_vab_vag_wav(header: *const u8, header_len: usize, body: *const u8, body_len: usize, index: u32, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let header = unsafe { input(header, header_len, "header")? };
        let body = unsafe { input(body, body_len, "body")? };
        let out = unsafe { out_ref(out, "out")? };
        *out = rebof3_buffer::from_vec(rebof3_formats::vab::vag::vab_vag_wav(header, body, index)?);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes a VAB bank as a SoundFont 2 file.
///
/// # Safety
/// `header` and `body` must point to their readable lengths, `title` must point to `title_len`
/// readable UTF-8 bytes, and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_soundfont(header: *const u8, header_len: usize, body: *const u8, body_len: usize, title: *const c_char, title_len: usize, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let header = unsafe { input(header, header_len, "header")? };
        let body = unsafe { input(body, body_len, "body")? };
        let title = unsafe { utf8(title, title_len, "title")? };
        let out = unsafe { out_ref(out, "out")? };
        *out = rebof3_buffer::from_vec(rebof3_formats::vab::soundfont::export(header, body, title)?);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes the PNG and JSON of one character sprite section.
///
/// # Safety
/// `sprites` and `executable` must point to their readable lengths, `image_name` must point to
/// `image_name_len` readable UTF-8 bytes, and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_sprite_sheet_export(
    sprites: *const u8,
    sprites_len: usize,
    executable: *const u8,
    executable_len: usize,
    section: u32,
    palette: u32,
    image_name: *const c_char,
    image_name_len: usize,
    out: *mut rebof3_sprite_sheet,
) -> rebof3_status {
    match guarded(|| {
        let sprites = unsafe { input(sprites, sprites_len, "sprites")? };
        let executable = unsafe { input(executable, executable_len, "executable")? };
        let image_name = unsafe { utf8(image_name, image_name_len, "image_name")? };
        let out = unsafe { out_ref(out, "out")? };
        let sprites = rebof3_formats::sprite::decode(sprites)?;
        let sizes = rebof3_formats::sprite::SpriteSizes::from_executable(executable)?;
        let sheet = rebof3_formats::sprite::export(&sprites, section as usize, palette as usize, &sizes, image_name)?;
        *out = rebof3_sprite_sheet { json: rebof3_buffer::from_vec(sheet.json), png: rebof3_buffer::from_vec(sheet.png) };
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Releases the two buffers of a sprite sheet.
///
/// # Safety
/// `sheet` must be NULL or a sheet this library returned, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_sprite_sheet_free(sheet: *mut rebof3_sprite_sheet) {
    if sheet.is_null() {
        return;
    }
    let sheet = unsafe { &mut *sheet };
    unsafe {
        crate::release_buffer(&mut sheet.json);
        crate::release_buffer(&mut sheet.png);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_values_follow_the_library_table() {
        assert_eq!(Format::ALL.len(), FORMAT_COUNT);
        for (index, format) in Format::ALL.iter().enumerate() {
            let value = format_value(*format);
            assert_eq!(value as usize, index);
            assert_eq!(format_of(value).expect("the value is in range"), *format);
        }
    }
}
