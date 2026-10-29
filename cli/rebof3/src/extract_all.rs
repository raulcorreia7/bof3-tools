// SPDX-License-Identifier: MIT
//! `assets extract-all`: every asset of the disc, one folder per step, plus the `index.json`
//! manifest that `rebof3 player extracted DIR` browses.
//!
//! Steps (each writes `DIR/<step>/`): `disc` the disc's files, `emi` every archive of `disc`,
//! `assets` the classified payloads (`assets.jsonl`), `assets-export` terrain glTF, message JSON,
//! SoundFont and MIDI from those payloads, `audio` rendered sample WAVs.

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rebof3_assets::decode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::assets::{IMAGE_DIR, extract_command, find_disc_image};
use crate::{disc, player};

const INDEX_JSON: &str = "index.json";
const INDEX_MD: &str = "index.md";
/// Default output folder, relative to the repository root (or the working directory).
const DEFAULT_OUT: &str = "build/extract";
/// The steps, in run order.
const STEPS: [&str; 5] = ["disc", "emi", "assets", "assets-export", "audio"];
/// Steps whose first folder level is the asset kind.
const KIND_FOLDER_STEPS: [&str; 2] = ["assets", "assets-export"];
/// Items the `audio` step renders (sample ids).
const AUDIO_QUERIES: [&str; 2] = ["sample:BATL_END", "sample:A085SE2"];
/// The browser's manifest keeps a compare column; no step has a peer to compare against.
const NO_PEER: &str = "none";
/// Most errors printed per step.
const SHOWN_ERRORS: usize = 5;
const HASH_BLOCK: usize = 1 << 20;

pub(crate) const USAGE: &str = "\
rebof3 assets extract-all: every asset of the disc, one folder per step, plus index.json.

Usage: rebof3 assets extract-all [-i IMAGE] [-o DIR] [--jobs N] [-q] [STEP...]

Steps (default: all, in this order):
  disc           the disc's files, as they are on the disc
  emi            every .EMI archive of disc, each entry as a file
  assets         the classified payloads and the text, with assets.jsonl and summary.txt
                 (needs the disc folder the disc step writes)
  assets-export  terrain glTF, message JSON, SoundFont and MIDI made from assets
  audio          the sample WAVs rendered by the audio library

  -i, --input IMAGE  CUE sheet or image (default: the one in inputs/external, then ./disk)
  -o, --output DIR   output folder (default: build/extract)
      --jobs N       worker threads (default: all cores)
  -q, --quiet        print only errors
  -h, --help         this text

DIR/index.json and DIR/index.md list every file with its step, kind, size and sha256.
Naming STEPs reruns only those; the other steps keep their earlier files and index rows.
Exit code: 0 success, 1 some step had failures, 2 an error.

Examples:
  rebof3 assets extract-all -i \"Breath of Fire III (USA).cue\" -o build/extract
  rebof3 assets extract-all emi assets-export -o build/extract
  rebof3 player extracted build/extract";

/// What a step reports before its folder is scanned.
#[derive(Default)]
struct Outcome {
    errors: Vec<String>,
    note: String,
    skipped: bool,
}

struct Context {
    image: PathBuf,
    out: PathBuf,
    jobs: usize,
}

impl Context {
    fn folder(&self, step: &str) -> PathBuf {
        self.out.join(step)
    }
}

struct Arguments {
    image: Option<PathBuf>,
    out: Option<PathBuf>,
    jobs: usize,
    quiet: bool,
    steps: Vec<String>,
}

fn parse(args: &[String]) -> Result<Option<Arguments>, String> {
    let mut parsed = Arguments {
        image: None,
        out: None,
        jobs: std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get),
        quiet: false,
        steps: Vec::new(),
    };
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let mut value = |name: &str| rest.next().cloned().ok_or_else(|| format!("option {name} needs a value."));
        match flag.as_str() {
            "-h" | "--help" | "help" => return Ok(None),
            "-i" | "--input" | "--image" => parsed.image = Some(PathBuf::from(value(flag)?)),
            "-o" | "--output" | "--out" => parsed.out = Some(PathBuf::from(value(flag)?)),
            "--jobs" => {
                parsed.jobs = value(flag)?
                    .parse::<usize>()
                    .ok()
                    .filter(|jobs| *jobs > 0)
                    .ok_or("--jobs needs a positive number, for example --jobs 4.")?;
            }
            "-q" | "--quiet" => parsed.quiet = true,
            step if STEPS.contains(&step) => parsed.steps.push(step.to_string()),
            other => {
                return Err(format!(
                    "unknown step or option {other:?}: choose from {}; run `rebof3 assets extract-all --help`.",
                    STEPS.join(", ")
                ));
            }
        }
    }
    Ok(Some(parsed))
}

pub fn run(args: &[String]) -> Result<i32, String> {
    let Some(arguments) = parse(args)? else {
        println!("{USAGE}");
        return Ok(0);
    };
    let current = env::current_dir().map_err(|error| error.to_string())?;
    let image = match arguments.image {
        Some(image) => image,
        None => find_disc_image(&current)
            .map_err(|error| format!("{error}. Put the CUE/BIN set in {IMAGE_DIR}/ or pass -i IMAGE."))?,
    };
    let out = arguments.out.unwrap_or_else(|| current.join(DEFAULT_OUT));
    fs::create_dir_all(&out).map_err(|error| format!("{}: {error}. Pass -o with a writable folder.", out.display()))?;
    let context = Context { image, out, jobs: arguments.jobs };
    let chosen: Vec<&str> = STEPS
        .into_iter()
        .filter(|step| arguments.steps.is_empty() || arguments.steps.iter().any(|s| s == step))
        .collect();

    let (mut records, previous) = load_previous(&context.out);
    let mut files: Vec<Value> =
        previous.into_iter().filter(|f| !chosen.contains(&f["consumer"].as_str().unwrap_or(""))).collect();
    let mut failed = false;
    for step in &chosen {
        let (record, found) = run_step(&context, step, arguments.quiet);
        failed |= record["status"] == "failed";
        records.insert((*step).to_string(), record);
        files.extend(found);
    }
    let ordered: Vec<Value> = STEPS.iter().filter_map(|step| records.remove(*step)).collect();
    write_index(&context, &ordered, &files)?;
    if !arguments.quiet {
        println!("index: {}", context.out.join(INDEX_JSON).display());
    }
    Ok(i32::from(failed))
}

fn run_step(context: &Context, step: &str, quiet: bool) -> (Value, Vec<Value>) {
    let started = Instant::now();
    let outcome = match step {
        "disc" => disc_step(context),
        "emi" => emi_step(context),
        "assets" => assets_step(context),
        "assets-export" => export_step(context),
        _ => audio_step(context),
    };
    let seconds = started.elapsed().as_secs_f64();
    let files = if outcome.skipped { Vec::new() } else { scan(context, step) };
    let bytes: u64 = files.iter().map(|f| f["size"].as_u64().unwrap_or(0)).sum();
    let status = if outcome.skipped {
        "skipped"
    } else if outcome.errors.is_empty() {
        "ok"
    } else {
        "failed"
    };
    if !quiet || status == "failed" {
        println!(
            "{step:<14} {status:<8} files={} bytes={bytes} seconds={seconds:.1} failures={}{}",
            files.len(),
            outcome.errors.len(),
            if outcome.note.is_empty() { String::new() } else { format!("  {}", outcome.note) }
        );
        for error in outcome.errors.iter().take(SHOWN_ERRORS) {
            println!("    {error}");
        }
        if outcome.errors.len() > SHOWN_ERRORS {
            println!("    ... {} more", outcome.errors.len() - SHOWN_ERRORS);
        }
    }
    let record = json!({
        "name": step, "status": status, "files": files.len(), "bytes": bytes,
        "seconds": seconds, "failures": outcome.errors.len(), "note": outcome.note,
    });
    (record, files)
}

fn disc_step(context: &Context) -> Outcome {
    let mut outcome = Outcome::default();
    if let Err(error) = disc::extract_image(&context.image, &context.folder("disc")) {
        outcome.errors.push(format!("{error}. Check the disc image or pass -i IMAGE."));
    }
    outcome
}

/// Every `.EMI` file below `folder`, sorted.
fn archives_in(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.is_dir() {
            archives_in(&path, found);
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("emi")) {
            found.push(path);
        }
    }
    found.sort();
}

fn emi_step(context: &Context) -> Outcome {
    let source = context.folder("disc");
    let mut archives = Vec::new();
    archives_in(&source, &mut archives);
    if archives.is_empty() {
        return Outcome {
            skipped: true,
            note: format!("needs {}: run the disc step first", source.display()),
            ..Outcome::default()
        };
    }
    let results = parallel(&archives, context.jobs, |archive| {
        let relative = archive.strip_prefix(&source).unwrap_or(archive);
        rebof3_emi::Archive::open(archive)
            .and_then(|opened| opened.extract_all(context.folder("emi").join(relative), true))
            .map_err(|error| format!("{}: {error}", relative.display()))
    });
    Outcome {
        errors: results.into_iter().filter_map(Result::err).collect(),
        note: format!("{} archives", archives.len()),
        skipped: false,
    }
}

fn assets_step(context: &Context) -> Outcome {
    let disc = context.folder("disc");
    if !disc.is_dir() {
        return Outcome {
            errors: vec![format!(
                "text: no extracted disc at {}; run the disc step first: rebof3 assets extract-all disc -o {}",
                disc.display(),
                context.out.display()
            )],
            ..Outcome::default()
        };
    }
    let args = [
        "-i".into(),
        context.image.display().to_string(),
        "-o".into(),
        context.folder("assets").display().to_string(),
        "--text-disc".into(),
        disc.display().to_string(),
        "-q".into(),
    ];
    let mut outcome = Outcome::default();
    match extract_command(&args) {
        Ok(0) => {}
        Ok(_) => outcome.errors.push("some payloads failed to write; see assets/summary.txt".into()),
        Err(error) => outcome.errors.push(format!("extract: {error}")),
    }
    outcome
}

/// One file `assets-export` makes from a payload `assets` wrote.
enum Export {
    Terrain,
    Message,
    Soundfont(PathBuf),
    Sequence(usize),
}

struct ExportJob {
    label: String,
    source: PathBuf,
    target: PathBuf,
    what: Export,
}

fn export_jobs(context: &Context, rows: &[Value]) -> Vec<ExportJob> {
    let source = context.folder("assets");
    let out = context.folder("assets-export");
    let mut jobs = Vec::new();
    let mut banks: BTreeMap<(String, String), BTreeMap<String, PathBuf>> = BTreeMap::new();
    for row in rows {
        let (Some(output), Some(kind)) = (row["output"].as_str(), row["kind"].as_str()) else { continue };
        if output == "-" {
            continue; // a payload the extractor reserves and does not write
        }
        let path = source.join(output);
        let stem = Path::new(output).strip_prefix(kind).unwrap_or(Path::new(output)).with_extension("");
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
        match (kind, extension.as_str()) {
            ("area-map", "bin") => jobs.push(ExportJob {
                label: format!("terrain {output}"),
                target: out.join("terrain").join(&stem).with_extension("glb"),
                source: path,
                what: Export::Terrain,
            }),
            ("area-message", "bin") => jobs.push(ExportJob {
                label: format!("message {output}"),
                target: out.join("message").join(&stem).with_extension("json"),
                source: path,
                what: Export::Message,
            }),
            ("audio-bank", "vh" | "vb") => {
                let key = (
                    row["source"].as_str().unwrap_or("").to_string(),
                    row["load_arg"].as_str().unwrap_or("").to_string(),
                );
                banks.entry(key).or_default().insert(extension, path);
            }
            ("audio-sequence", "seq") => {
                let count = fs::read(&path)
                    .ok()
                    .and_then(|bytes| psx_formats::sep::Package::parse(&bytes).ok())
                    .map_or(0, |p| p.sequences.len());
                let folder = out.join("sequence").join(stem.parent().unwrap_or(Path::new("")));
                let name = stem.file_name().and_then(|n| n.to_str()).unwrap_or("sequence");
                for index in 0..count {
                    jobs.push(ExportJob {
                        label: format!("sequence {output}#{index}"),
                        target: folder.join(format!("{name}.{index}.mid")),
                        source: path.clone(),
                        what: Export::Sequence(index),
                    });
                }
            }
            _ => {}
        }
    }
    for ((from, load_arg), halves) in banks {
        if let (Some(header), Some(body)) = (halves.get("vh"), halves.get("vb")) {
            let stem = header.strip_prefix(source.join("audio-bank")).unwrap_or(header).with_extension("");
            jobs.push(ExportJob {
                label: format!("soundfont {from} {load_arg}"),
                target: out.join("soundfont").join(stem).with_extension("sf2"),
                source: header.clone(),
                what: Export::Soundfont(body.clone()),
            });
        }
    }
    jobs
}

fn run_export(job: &ExportJob) -> Result<(), String> {
    let bytes = fs::read(&job.source).map_err(|error| error.to_string())?;
    let data = match &job.what {
        Export::Terrain => {
            let mesh = rebof3_assets::terrain::mesh_from_payload(&bytes).map_err(|e| e.to_string())?;
            let name = job.target.file_stem().unwrap_or_default().to_string_lossy();
            rebof3_assets::terrain::glb_of_mesh(&mesh, &name).ok_or("no cell to draw")?
        }
        Export::Message => rebof3_assets::text::bank::export(&bytes).map_err(|e| e.to_string())?,
        Export::Soundfont(body) => {
            let body = fs::read(body).map_err(|error| error.to_string())?;
            let title = job.target.file_stem().unwrap_or_default().to_string_lossy();
            decode::soundfont(&bytes, &body, &title).map_err(|e| e.to_string())?
        }
        Export::Sequence(index) => decode::sequence_midi(&bytes, *index).map_err(|e| e.to_string())?,
    };
    if let Some(parent) = job.target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&job.target, data).map_err(|error| error.to_string())
}

fn export_step(context: &Context) -> Outcome {
    let manifest = context.folder("assets").join("assets.jsonl");
    let Ok(text) = fs::read_to_string(&manifest) else {
        return Outcome {
            skipped: true,
            note: format!("needs {}: run the assets step first", manifest.display()),
            ..Outcome::default()
        };
    };
    let rows: Vec<Value> =
        text.lines().filter(|line| !line.is_empty()).filter_map(|line| serde_json::from_str(line).ok()).collect();
    let jobs = export_jobs(context, &rows);
    let results =
        parallel(&jobs, context.jobs, |job| run_export(job).map_err(|error| format!("{}: {error}", job.label)));
    Outcome { errors: results.into_iter().filter_map(Result::err).collect(), ..Outcome::default() }
}

fn audio_step(context: &Context) -> Outcome {
    let mut outcome = Outcome::default();
    let out = context.folder("audio");
    for query in AUDIO_QUERIES {
        match player::render_matching(&context.image, query, &out) {
            Ok(0) => {}
            Ok(count) => outcome.errors.push(format!("{query}: {count} items failed to render")),
            Err(error) => outcome.errors.push(format!("{query}: {error}")),
        }
    }
    outcome.note = format!("items {}", AUDIO_QUERIES.join(", "));
    outcome
}

/// Runs `work` over `items` on up to `jobs` threads; results keep the order of `items`.
fn parallel<T: Sync, R: Send>(items: &[T], jobs: usize, work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new(items.iter().map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(items.len()).max(1) {
            scope.spawn(|| {
                loop {
                    let slot = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(slot) else { break };
                    let result = work(item);
                    results.lock().expect("a worker panicked")[slot] = Some(result);
                }
            });
        }
    });
    results.into_inner().expect("a worker panicked").into_iter().flatten().collect()
}

fn sha256(path: &Path) -> String {
    let Ok(mut file) = fs::File::open(path) else { return String::new() };
    let mut digest = Sha256::new();
    let mut block = vec![0; HASH_BLOCK];
    while let Ok(read) = file.read(&mut block) {
        if read == 0 {
            break;
        }
        digest.update(&block[..read]);
    }
    digest.finalize().iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

fn kind_of(step: &str, relative: &Path) -> String {
    let name = relative.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if step == "audio" {
        return name.split('_').next().unwrap_or(name).to_string(); // `<kind>_<item>.wav`
    }
    let mut parts = relative.components();
    let first = parts.next().and_then(|c| c.as_os_str().to_str());
    if let (true, Some(first), Some(_)) = (KIND_FOLDER_STEPS.contains(&step), first, parts.next()) {
        return first.to_string();
    }
    relative.extension().map_or_else(|| "no extension".into(), |ext| ext.to_string_lossy().to_lowercase())
}

fn files_under(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.is_dir() {
            files_under(&path, found);
        } else {
            found.push(path);
        }
    }
}

/// Every file under `DIR/STEP` with its kind, size and sha256, sorted by path.
fn scan(context: &Context, step: &str) -> Vec<Value> {
    let root = context.folder(step);
    let mut paths = Vec::new();
    files_under(&root, &mut paths);
    paths.sort();
    let digests = parallel(&paths, context.jobs, |path| sha256(path));
    paths
        .iter()
        .zip(digests)
        .map(|(path, digest)| {
            let relative = path.strip_prefix(&root).unwrap_or(path);
            let posix = relative.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            json!({
                "consumer": step, "kind": kind_of(step, relative), "path": posix,
                "size": fs::metadata(path).map_or(0, |m| m.len()), "sha256": digest,
                "compare": NO_PEER, "peer": "",
            })
        })
        .collect()
}

/// Steps of an earlier run that this run does not repeat keep their rows.
fn load_previous(out: &Path) -> (BTreeMap<String, Value>, Vec<Value>) {
    let Some(data) = fs::read_to_string(out.join(INDEX_JSON)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return (BTreeMap::new(), Vec::new());
    };
    let records = data["consumers"]
        .as_array()
        .map(|list| list.iter().filter_map(|c| Some((c["name"].as_str()?.to_string(), c.clone()))).collect())
        .unwrap_or_default();
    (records, data["files"].as_array().cloned().unwrap_or_default())
}

fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut lines = vec![format!("| {} |", header.join(" | ")), format!("|{}|", vec!["---"; header.len()].join("|"))];
    lines.extend(rows.iter().map(|row| format!("| {} |", row.join(" | "))));
    lines.join("\n")
}

fn write_index(context: &Context, records: &[Value], files: &[Value]) -> Result<(), String> {
    let index = json!({
        "root": context.out.display().to_string(),
        "disc": context.image.display().to_string(),
        "consumers": records,
        "files": files,
    });
    let write = |name: &str, text: String| {
        let path = context.out.join(name);
        fs::write(&path, text).map_err(|error| format!("{}: {error}. Pass -o with a writable folder.", path.display()))
    };
    write(INDEX_JSON, index.to_string())?;
    let cell = |record: &Value, key: &str| match &record[key] {
        Value::String(text) => text.clone(),
        Value::Number(number) if key == "seconds" => format!("{:.1}", number.as_f64().unwrap_or(0.0)),
        other => other.to_string(),
    };
    let mut md = format!("# Extract index\n\nDisc: `{}`\n\n## Consumers\n\n", context.image.display());
    let header = ["consumer", "status", "files", "bytes", "seconds", "failures", "note"];
    let rows: Vec<Vec<String>> = records
        .iter()
        .map(|r| {
            ["name", "status", "files", "bytes", "seconds", "failures", "note"].iter().map(|k| cell(r, k)).collect()
        })
        .collect();
    md.push_str(&table(&header, &rows));
    for record in records {
        let name = record["name"].as_str().unwrap_or("");
        let mut kinds: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
        for file in files.iter().filter(|f| f["consumer"] == name) {
            let entry = kinds.entry(file["kind"].as_str().unwrap_or("")).or_default();
            entry.0 += 1;
            entry.1 += file["size"].as_u64().unwrap_or(0);
        }
        if kinds.is_empty() {
            continue;
        }
        let rows: Vec<Vec<String>> =
            kinds.iter().map(|(k, (n, b))| vec![(*k).into(), n.to_string(), b.to_string()]).collect();
        let _ = write!(md, "\n\n## {name}\n\n{}", table(&["kind", "files", "bytes"], &rows));
    }
    md.push('\n');
    write(INDEX_MD, md)
}
