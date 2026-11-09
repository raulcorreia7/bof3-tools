//! The table of asset codecs: for each kind, how its stored bytes become an open form (export) and
//! come back (import).
//!
//! A [`Codec`] row names the kind and holds an [`Op`] per direction. An op says which files it
//! reads, which options it takes, the messages for what is missing, the default output extension and
//! the function that does the work, so a command line can be built from the table: parse, check the
//! op's needs, read the files, call [`Op::run`], write the bytes.

use crate::error::Result;
use rebof3_formats::message::table;
use rebof3_formats::text::strings;

use crate::{import, sprites, tables};

/// Which way a conversion goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// Stored bytes to the open form.
    Export,
    /// The open form back to stored bytes.
    Import,
}

/// The inputs of one conversion, already read.
#[derive(Clone, Copy, Debug, Default)]
pub struct Job<'a> {
    /// A name the op takes before its file (the game table's name).
    pub name: Option<&'a str>,
    /// The main file.
    pub input: &'a [u8],
    /// The second positional file (the VAB body), or the file the `--payload` or `--base` flag names.
    pub companion: Option<&'a [u8]>,
    /// `--index`.
    pub index: Option<u32>,
    /// `--load-arg`.
    pub load_arg: Option<u32>,
}

/// How an op uses `--index`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Index {
    /// The op takes no index.
    No,
    /// The op takes an index and has a default.
    Optional,
    /// The op needs an index; the text is the error when it is missing.
    Required(&'static str),
}

/// A flag that names a second file the op reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompanionFlag {
    /// The flag, for example `--payload`.
    pub flag: &'static str,
    /// The error when the flag is missing.
    pub missing: &'static str,
}

/// One direction of a codec.
pub struct Op {
    /// The error when the main file is missing.
    pub needs_input: &'static str,
    /// Positional files: 1, or 2 when the second is the companion (a VAB body).
    pub files: usize,
    /// The op takes the file's name before the file.
    pub named: bool,
    /// The flag that names the companion file, if the op takes one.
    pub companion_flag: Option<CompanionFlag>,
    /// How the op uses `--index`.
    pub index: Index,
    /// Whether the op takes `--load-arg`.
    pub load_arg: bool,
    /// The default output extension; `{index}` is replaced by the index.
    pub extension: &'static str,
    /// The conversion.
    pub run: fn(&Job<'_>) -> Result<Vec<u8>>,
}

impl Op {
    const fn new(needs_input: &'static str, extension: &'static str, run: fn(&Job<'_>) -> Result<Vec<u8>>) -> Self {
        Self { needs_input, files: 1, named: false, companion_flag: None, index: Index::No, load_arg: false, extension, run }
    }

    const fn with_companion(mut self, flag: &'static str, missing: &'static str) -> Self {
        self.companion_flag = Some(CompanionFlag { flag, missing });
        self
    }

    /// The default extension of the output file for `index`.
    pub fn extension_for(&self, index: Option<u32>) -> String {
        self.extension.replace("{index}", &index.unwrap_or(0).to_string())
    }
}

/// One asset kind with an open form.
pub struct Codec {
    /// The kind's name on the command line.
    pub kind: &'static str,
    /// Stored bytes to the open form.
    pub export: Option<Op>,
    /// The open form back to stored bytes.
    pub import: Option<Op>,
}

impl Codec {
    /// The op for `direction`.
    pub fn op(&self, direction: Direction) -> Option<&Op> {
        match direction {
            Direction::Export => self.export.as_ref(),
            Direction::Import => self.import.as_ref(),
        }
    }
}

/// The codec of `kind`, if the kind has one.
pub fn find(kind: &str) -> Option<&'static Codec> {
    CODECS.iter().find(|codec| codec.kind == kind)
}

fn companion<'a>(job: &Job<'a>) -> &'a [u8] {
    job.companion.unwrap_or_default()
}

/// Every codec, in the order the help lists them.
pub static CODECS: &[Codec] = &[
    Codec {
        kind: "palette",
        export: Some(Op::new("export palette needs a CLUT block file", "json", |job| Ok(rebof3_formats::clut::export(job.input)?))),
        import: Some(Op::new("import palette needs a palette JSON file", "bin", |job| Ok(rebof3_formats::clut::import(job.input)?))),
    },
    Codec {
        kind: "vag",
        export: Some(Op {
            files: 2,
            index: Index::Required("export vag needs --index, the VAG's number in the bank"),
            ..Op::new("export vag needs a header (.vh) and a body (.vb) file", "{index}.wav", |job| Ok(rebof3_formats::vab::vag::vab_vag_wav(job.input, companion(job), job.index.unwrap_or(0))?))
        }),
        import: Some(Op::new("import vag needs a WAVE file", "vag", |job| Ok(rebof3_formats::vab::vag::import(job.input)?))),
    },
    Codec {
        kind: "sequence",
        export: Some(Op {
            index: Index::Optional,
            ..Op::new("export sequence needs a sequence package (.seq) file", "mid", |job| Ok(rebof3_formats::sep::midi::export(job.input, job.index.unwrap_or(0) as usize)?))
        }),
        import: None,
    },
    Codec {
        kind: "vab",
        export: Some(Op {
            files: 2,
            ..Op::new("export vab needs a header (.vh) and a body (.vb) file", "vab", |job| {
                rebof3_formats::vab::check_pair(job.input, companion(job))?;
                Ok(rebof3_formats::vab::standard_file(job.input, companion(job)))
            })
        }),
        import: None,
    },
    Codec {
        kind: "sep",
        export: Some(Op::new("export sep needs a sequence package (.seq) file", "sep", |job| Ok(rebof3_formats::sep::psyq::export_sep(job.input)?))),
        import: Some(Op::new("import sep needs a SEP file", "bin", |job| Ok(rebof3_formats::sep::psyq::import_sep(job.input)?))),
    },
    Codec {
        kind: "seq",
        export: Some(Op {
            index: Index::Optional,
            ..Op::new("export seq needs a sequence package (.seq) file", "{index}.seq", |job| Ok(rebof3_formats::sep::psyq::export_seq(job.input, job.index.unwrap_or(0) as usize)?))
        }),
        import: None,
    },
    Codec {
        kind: "message",
        export: Some(Op::new("export message needs a message table payload", "json", |job| Ok(table::export(job.input)?))),
        import: Some(Op::new("import message needs a JSON file from export message", "bin", |job| Ok(table::import(job.input)?))),
    },
    Codec {
        kind: "message-po",
        export: Some(Op::new("export message-po needs a message table payload", "po", |job| Ok(rebof3_formats::message::po::export(job.input)?))),
        import: Some(
            Op::new("import message-po needs a PO file from export message-po", "bin", |job| Ok(rebof3_formats::message::po::import(job.input, companion(job))?))
                .with_companion("--payload", "import message-po needs --payload, the message table it was exported from"),
        ),
    },
    Codec {
        kind: "script",
        export: Some(Op::new("export script needs a script table payload", "txt", |job| Ok(rebof3_formats::script::text::export(job.input)?))),
        import: Some(
            Op::new("import script needs a text file from export script", "bin", |job| Ok(rebof3_formats::script::text::import(job.input, companion(job))?))
                .with_companion("--payload", "import script needs --payload, the bank it was exported from"),
        ),
    },
    Codec {
        kind: "strings",
        export: Some(Op { load_arg: true, ..Op::new("export strings needs a payload file", "json", |job| Ok(strings::export(job.input, job.load_arg)?)) }),
        import: Some(
            Op::new("import strings needs a JSON file from export strings", "bin", |job| Ok(strings::import(job.input, companion(job))?))
                .with_companion("--payload", "import strings needs --payload, the file it was exported from"),
        ),
    },
    Codec {
        kind: "characters",
        export: Some(Op::new("export characters needs the payload of ETC/START.EMI entry 8", "json", |job| tables::characters(job.input))),
        import: Some(
            Op::new("import characters needs a JSON file from export characters", "bin", |job| import::characters(job.input, companion(job)))
                .with_companion("--payload", "import characters needs --payload, the entry it was exported from"),
        ),
    },
    Codec {
        kind: "character-sprites",
        export: Some(Op::new("export character-sprites needs the decoded character sprites", "json", |job| sprites::character_sprites(job.input))),
        import: Some(
            Op::new("import character-sprites needs a JSON file from export character-sprites", "bin", |job| import::character_sprites(job.input, companion(job)))
                .with_companion("--payload", "import character-sprites needs --payload, the decoded sprites it was exported from"),
        ),
    },
    Codec {
        kind: "terrain",
        export: None,
        import: Some(
            Op::new("import terrain needs a mesh file", "bin", |job| Ok(rebof3_formats::map::terrain::import(job.input, companion(job))?))
                .with_companion("--payload", "import terrain needs --payload, the stored bundle"),
        ),
    },
    Codec {
        kind: "table",
        export: None,
        import: Some(Op {
            named: true,
            ..Op::new("import table needs a table name and its JSON file", "bin", |job| {
                Ok(rebof3_formats::game_tables::open::import_in_payload(job.name.unwrap_or_default(), job.input, companion(job))?)
            })
            .with_companion("--payload", "import table needs --payload, the stored entry 0 of ETC/GAME.EMI")
        }),
    },
    Codec {
        kind: "encounters",
        export: None,
        import: Some(
            Op::new("import encounters needs the block's JSON file", "bin", |job| Ok(rebof3_formats::encounter::open::import(job.input, companion(job))?))
                .with_companion("--base", "import encounters needs --base, the stored block"),
        ),
    },
    Codec {
        kind: "layout",
        export: None,
        import: Some(
            Op::new("import layout needs the layout's JSON file", "bin", |job| Ok(rebof3_formats::map::layout::import(job.input, companion(job))?))
                .with_companion("--payload", "import layout needs --payload, the stored bundle"),
        ),
    },
];
