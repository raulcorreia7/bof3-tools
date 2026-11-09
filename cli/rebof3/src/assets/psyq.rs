//! The verbs for the files a 1990s PlayStation studio exchanged: TIM, VAB (VH and VB) and SEQ. The
//! conversions are in `rebof3_formats`; this file reads the arguments and writes the files.

use std::fs;
use std::path::PathBuf;

use clap::Parser;
use rebof3_assets::{Error, Result};
use rebof3_formats::vram::{self, Placement};

use super::write_output;
use super::{EXIT_OK, color_words, create_parent, number, optional_number, parse, read_file, single, with_extension_of};

pub(super) const EXPORT_TIM_HELP: &str = "\
bof3 assets export tim: write a texture as a PlayStation TIM file.

Usage: bof3 assets export tim PAYLOAD --load-arg HEX [-o OUT]
       bof3 assets export tim PAYLOAD --load-arg HEX --depth 4|8 --palette CLUT_BLOCK --clut X,Y
                                       [--palette-offset WORDS] [-o OUT]

  PAYLOAD                   stored texture payload
      --load-arg HEX        the entry's load address (required); it gives the image's VRAM place
      --depth 4|8           write an indexed TIM with a CLUT block (default: a 15-bit TIM, no CLUT)
      --palette CLUT_BLOCK  color-word file for --depth (required with it)
      --palette-offset N    first word of the palette row (default: 0)
      --clut X,Y            the CLUT row's VRAM place, in words and rows (required with --depth)
  -o, --output OUT          TIM to write (default: PAYLOAD.tim)

The TIM header carries the image's and the CLUT's VRAM coordinates, so a tool that lays textures out
in VRAM reads them from the file. A texture no draw site joins to a palette row has no CLUT of its own:
give one with --depth, --palette and --clut, or write the 15-bit TIM.

Examples:
  bof3 assets export tim payload.img --load-arg 0x16080100 -o 7.tim
  bof3 assets export tim payload.img --load-arg 0x16080100 --depth 4 --palette block.clut --clut 160,483 -o 7.tim
";

pub(super) const IMPORT_TIM_HELP: &str = "\
bof3 assets import tim: read a TIM file back into a texture payload and its palette.

Usage: bof3 assets import tim FILE.tim [-o OUT] [--load-arg HEX]
                                       [--palette OUT [--clut-block CLUT_BLOCK [--palette-offset WORDS]]]

  FILE.tim                  a TIM whose image is whole 32-by-32-word rectangles on a 32-word grid
  -o, --output OUT          payload to write (default: FILE.img)
      --load-arg HEX        the entry's stored load address; its other bits are kept in the one printed
      --palette OUT         write the CLUT block's color words (needs a CLUT block)
      --clut-block CLUT_BLOCK
                            with --palette: write CLUT_BLOCK with the CLUT block's words spliced in instead
      --palette-offset N    first word of the row in CLUT_BLOCK (default: 0)

Prints the image's place and the load argument it implies. Write that load argument into the
entry's table row to move the texture; the payload alone keeps the entry where it is.

Examples:
  bof3 assets import tim 7.tim -o payload.img --load-arg 0x16080100
  bof3 assets import tim 7.tim -o payload.img --palette block.new --clut-block block.clut --palette-offset 16
";

pub(super) const IMPORT_VAB_HELP: &str = "\
bof3 assets import vab: split a VAB file into its header (VH) and body (VB).

Usage: bof3 assets import vab FILE.vab [-o OUT.vh] [--body OUT.vb]

  FILE.vab            a VAB file: the header followed by the body
  -o, --output OUT    header to write (default: FILE.vh)
      --body OUT      body to write (default: FILE.vb)

The halves are the stored header and body entries. Both must parse as a bank and the header must
encode to its own bytes. Write the pair back with `export vab`.

Example:
  bof3 assets import vab bank.vab -o bank.vh --body bank.vb
";

pub(super) const EXPORT_VAB_HELP: &str = "\
bof3 assets export vab: join a bank's header and body into one VAB file.

Usage: bof3 assets export vab HEADER BODY [-o OUT]

  HEADER              the header (.vh)
  BODY                the body (.vb)
  -o, --output OUT    VAB to write (default: HEADER.vab)

Example:
  bof3 assets export vab bank.vh bank.vb -o bank.vab
";

pub(super) const EXPORT_SEQ_HELP: &str = "\
bof3 assets export seq: write one sequence of a package as a SEQ file.

Usage: bof3 assets export seq PACKAGE [--index N] [-o OUT]

  PACKAGE             a stored sequence package (.seq), which is a SEP file
      --index N       the sequence's position in the package (default: 0)
  -o, --output OUT    SEQ to write (default: PACKAGE.N.seq)

A SEQ file has no sequence id; give the id back with `import seq`.

Example:
  bof3 assets export seq package.seq --index 2 -o song.seq
";

pub(super) const IMPORT_SEQ_HELP: &str = "\
bof3 assets import seq: write a sequence package from SEQ files.

Usage: bof3 assets import seq FILE.seq:ID... [-o OUT]

  FILE.seq:ID         one file per sequence, in order; ID is its package id
  -o, --output OUT    package to write (default: first file with .bin)

Example:
  bof3 assets import seq song.seq:1 other.seq:2 -o package.bin
";

pub(super) const EXPORT_SEP_HELP: &str = "\
bof3 assets export sep: write a sequence package as a SEP file.

Usage: bof3 assets export sep PACKAGE [-o OUT]

  PACKAGE             a stored sequence package (.seq)
  -o, --output OUT    SEP to write (default: PACKAGE.sep)

The stored package already is the SEP layout; the file is checked and written unchanged.
";

pub(super) const IMPORT_SEP_HELP: &str = "\
bof3 assets import sep: read a SEP file back into a sequence package.

Usage: bof3 assets import sep FILE.sep [-o OUT]

  FILE.sep            a SEP file
  -o, --output OUT    package to write (default: FILE.bin)
";

/// `bof3 assets export tim PAYLOAD --load-arg HEX [--depth 4|8 --palette CLUT_BLOCK --clut X,Y ...]`.
#[derive(Parser)]
struct ExportTim {
    files: Vec<PathBuf>,
    #[arg(long = "load-arg")]
    load_arg: Option<String>,
    #[arg(long)]
    depth: Option<String>,
    #[arg(long)]
    palette: Option<PathBuf>,
    #[arg(long = "palette-offset")]
    palette_offset: Option<String>,
    #[arg(long)]
    clut: Option<String>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
}

pub(super) fn export_tim(args: &[String]) -> Result<i32> {
    let args: ExportTim = parse(args)?;
    let payload_path = single(args.files, "export tim needs a payload file")?;
    let load_arg = optional_number(args.load_arg.as_deref(), "--load-arg")?.ok_or_else(|| Error::Usage("export tim needs --load-arg, the entry's load address".into()))?;
    let payload = read_file(&payload_path)?;
    let (tim, what) = match optional_number(args.depth.as_deref(), "--depth")? {
        None => {
            if args.palette.is_some() || args.clut.is_some() {
                return Err(Error::Usage("--palette and --clut need --depth, the CLUT row's depth".into()));
            }
            (vram::texture_tim_direct(load_arg, &payload)?, "15-bit, no CLUT".to_string())
        }
        Some(bits @ (4 | 8)) => {
            let bank = args.palette.ok_or_else(|| Error::Usage("--depth needs --palette, the CLUT block that holds the row".into()))?;
            let clut = args.clut.ok_or_else(|| Error::Usage("--depth needs --clut X,Y, the CLUT row's VRAM place; none is guessed".into()))?;
            let (x, y) = clut.split_once(',').ok_or_else(|| Error::Usage(format!("--clut needs X,Y, not {clut:?}")))?;
            let (x, y) = (clut_part(x, "--clut")?, clut_part(y, "--clut")?);
            let count = if bits == 4 { 16 } else { 256 };
            let offset = optional_number(args.palette_offset.as_deref(), "--palette-offset")?.unwrap_or(0) as usize;
            let words = color_words(&read_file(&bank)?)?;
            let row = words.get(offset..offset + count).ok_or_else(|| Error::Usage(format!("{} has no {count}-color row at word {offset}", bank.display())))?;
            (vram::texture_tim_of(load_arg, &payload, bits, row, x, y)?, format!("{bits}-bit, CLUT at ({x}, {y})"))
        }
        Some(other) => return Err(Error::Usage(format!("--depth is 4 or 8, not {other}"))),
    };
    let out = with_extension_of(args.output, &payload_path, "tim");
    write_output(&out, &tim)?;
    println!("  {what}");
    Ok(EXIT_OK)
}

fn clut_part(text: &str, flag: &str) -> Result<u16> {
    u16::try_from(number(text.trim(), flag)?).map_err(|_| Error::Usage(format!("{flag} needs 16-bit numbers")))
}

/// `bof3 assets import tim FILE.tim [-o OUT] [--load-arg HEX] [--palette OUT [--clut-block CLUT_BLOCK ...]]`.
#[derive(Parser)]
struct ImportTim {
    files: Vec<PathBuf>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
    #[arg(long = "load-arg")]
    load_arg: Option<String>,
    #[arg(long)]
    palette: Option<PathBuf>,
    #[arg(long = "clut-block", alias = "bank")]
    clut_block: Option<PathBuf>,
    #[arg(long = "palette-offset")]
    palette_offset: Option<String>,
}

pub(super) fn import_tim(args: &[String]) -> Result<i32> {
    let args: ImportTim = parse(args)?;
    let input = single(args.files, "import tim needs a TIM file")?;
    let base = optional_number(args.load_arg.as_deref(), "--load-arg")?;
    if args.clut_block.is_some() && args.palette.is_none() {
        return Err(Error::Usage("--clut-block needs --palette, the file to write the spliced CLUT block to".into()));
    }
    let texture = vram::import_tim(&read_file(&input)?)?;
    let out = with_extension_of(args.output, &input, "img");
    create_parent(&out)?;
    fs::write(&out, &texture.payload)?;
    let Placement { column, row, chunks_per_row } = texture.placement;
    let depth = texture.depth.map_or("15-bit".to_string(), |depth| format!("{}-bit", depth.bits()));
    println!("wrote {} ({depth} image)", out.display());
    println!("  image at ({}, {}), {chunks_per_row} rectangles per row: load argument {:#010x}", texture.placement.x(), texture.placement.y(), texture.placement.load_argument(base.unwrap_or(0)));
    println!("  column {column}, row {row}");
    if let Some(path) = args.palette {
        let clut = texture.clut.ok_or_else(|| Error::Usage("--palette needs a TIM with a CLUT block; this one has none".into()))?;
        println!("  CLUT at ({}, {}), {} rows of {} words", clut.x, clut.y, clut.height, clut.width);
        let bytes = match args.clut_block {
            Some(block) => {
                let offset = optional_number(args.palette_offset.as_deref(), "--palette-offset")?.unwrap_or(0) as usize;
                rebof3_formats::clut::splice(&read_file(&block)?, offset, &clut.words)?
            }
            None => clut.words.iter().flat_map(|word| word.to_le_bytes()).collect(),
        };
        create_parent(&path)?;
        fs::write(&path, bytes)?;
        println!("wrote {} ({} color words)", path.display(), clut.words.len());
    }
    Ok(EXIT_OK)
}

/// `bof3 assets import vab FILE.vab [-o OUT.vh] [--body OUT.vb]`.
#[derive(Parser)]
struct ImportVab {
    files: Vec<PathBuf>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
    #[arg(long)]
    body: Option<PathBuf>,
}

pub(super) fn import_vab(args: &[String]) -> Result<i32> {
    let args: ImportVab = parse(args)?;
    let input = single(args.files, "import vab needs a VAB file")?;
    let (header, body) = rebof3_formats::vab::split_file(&read_file(&input)?)?;
    write_output(&with_extension_of(args.output, &input, "vh"), &header)?;
    write_output(&args.body.unwrap_or_else(|| input.with_extension("vb")), &body)?;
    Ok(EXIT_OK)
}

/// `bof3 assets import seq FILE.seq:ID... [-o OUT]`.
#[derive(Parser)]
struct ImportSeq {
    parts: Vec<String>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
}

pub(super) fn import_seq(args: &[String]) -> Result<i32> {
    let args: ImportSeq = parse(args)?;
    let parts = args
        .parts
        .iter()
        .map(|part| {
            let (path, id) = part.rsplit_once(':').ok_or_else(|| Error::Usage(format!("{part:?} needs FILE:ID")))?;
            let id: u16 = id.parse().map_err(|_| Error::Usage(format!("{part:?} needs a numeric ID")))?;
            Ok((PathBuf::from(path), id))
        })
        .collect::<Result<Vec<_>>>()?;
    let Some((first, _)) = parts.first() else {
        return Err(Error::Usage("import seq needs at least one FILE:ID".into()));
    };
    let default = first.with_extension("bin");
    let files = parts.iter().map(|(path, id)| -> Result<(Vec<u8>, u16)> { Ok((read_file(path)?, *id)) }).collect::<Result<Vec<_>>>()?;
    let borrowed: Vec<(&[u8], u16)> = files.iter().map(|(bytes, id)| (bytes.as_slice(), *id)).collect();
    write_output(&args.output.unwrap_or(default), &rebof3_formats::sep::psyq::import_seq(&borrowed)?)?;
    Ok(EXIT_OK)
}
