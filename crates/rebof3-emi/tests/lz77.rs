//! Type-1 codec scoreboard: decode, re-encode, and compare all 65 payloads.
//!
//! It checks semantic round-trip and classifies the first differing frame
//! against each retail stream.

mod support;

use rebof3_emi::lz77::{Frame, HEADER, decode, encode, frames};

use support::{first_mismatch, retail_root, type_1_payloads};

/// Earliest measured mismatch byte offset across the 65 retail payloads.
const EARLIEST_REENCODE_MISMATCH: usize = 0x25;
const EXPECTED_PAYLOADS: usize = 65;

#[test]
fn type_1_payloads_decode_and_reencode() {
    let Some(root) = retail_root() else {
        return;
    };
    let payloads = type_1_payloads(&root);
    assert_eq!(
        payloads.len(),
        EXPECTED_PAYLOADS,
        "expected {EXPECTED_PAYLOADS} type-1 payloads under {}",
        root.display()
    );

    let report = Scoreboard::measure(&payloads);
    report.print(payloads.len());
    assert!(
        report.decode_failures.is_empty(),
        "{} type-1 payloads failed to decode:\n{}",
        report.decode_failures.len(),
        report.decode_failures.join("\n")
    );
    assert!(
        report.roundtrip_failures.is_empty(),
        "{} encoded payloads failed semantic round-trip:\n{}",
        report.roundtrip_failures.len(),
        report.roundtrip_failures.join("\n")
    );
    assert!(
        report.earliest >= EARLIEST_REENCODE_MISMATCH,
        "re-encode diverges at 0x{:X}, before the measured bound \
         0x{EARLIEST_REENCODE_MISMATCH:X}; {} streams differ:\n{}",
        report.earliest,
        report.divergences.len(),
        report.divergences.join("\n")
    );

    let out = support::out_dir("type_1_payloads_decode_and_reencode");
    let (_, _, retail) = &payloads[0];
    let decoded = decode(retail).expect("the first payload decodes");
    support::write(&out, "decoded.bin", &decoded);
    support::write(&out, "re-encoded.lz", encode(&decoded));
    support::write(
        &out,
        "scoreboard.txt",
        format!(
            "{} payloads; {} byte-exact re-encodes; earliest first difference 0x{:X}\n\n{}\\n",
            payloads.len(),
            report.exact,
            report.earliest,
            report.divergences.join("\n")
        ),
    );
}
#[derive(Default)]
struct Scoreboard {
    decode_failures: Vec<String>,
    roundtrip_failures: Vec<String>,
    divergences: Vec<String>,
    earliest: usize,
    exact: usize,
    classes: FrameCounts,
}

#[derive(Default)]
struct FrameCounts {
    literal_run_length: usize,
    match_length: usize,
    match_distance: usize,
    literal_vs_match: usize,
    frame_or_end: usize,
}

#[derive(Clone, Copy)]
enum FrameClass {
    LiteralRunLength,
    MatchLength,
    MatchDistance,
    LiteralVsMatch,
    FrameOrEnd,
}

impl FrameClass {
    fn name(self) -> &'static str {
        match self {
            Self::LiteralRunLength => "literal-run-length",
            Self::MatchLength => "match-length",
            Self::MatchDistance => "match-distance",
            Self::LiteralVsMatch => "literal-vs-match",
            Self::FrameOrEnd => "frame-or-end",
        }
    }
}

impl FrameCounts {
    fn record(&mut self, class: FrameClass) {
        match class {
            FrameClass::LiteralRunLength => self.literal_run_length += 1,
            FrameClass::MatchLength => self.match_length += 1,
            FrameClass::MatchDistance => self.match_distance += 1,
            FrameClass::LiteralVsMatch => self.literal_vs_match += 1,
            FrameClass::FrameOrEnd => self.frame_or_end += 1,
        }
    }
}

impl Scoreboard {
    fn measure(payloads: &[support::Payload]) -> Self {
        let mut report = Self { earliest: usize::MAX, ..Self::default() };
        for (path, index, payload) in payloads {
            report.check(path, *index, payload);
        }
        report
    }

    fn check(&mut self, path: &std::path::Path, index: usize, payload: &[u8]) {
        let decoded = match decode(payload) {
            Ok(decoded) => decoded,
            Err(error) => {
                self.decode_failures.push(format!("{}#{index}: {error}", path.display()));
                return;
            }
        };
        let encoded = encode(&decoded);
        match decode(&encoded) {
            Ok(roundtrip) if roundtrip == decoded => {}
            Ok(_) => self
                .roundtrip_failures
                .push(format!("{}#{index}: encoded payload decodes to different bytes", path.display())),
            Err(error) => self.roundtrip_failures.push(format!("{}#{index}: encoded payload: {error}", path.display())),
        }
        if encoded.as_slice() == payload {
            self.exact += 1;
            return;
        }

        let at = first_mismatch(payload, &encoded).unwrap_or(payload.len().min(encoded.len()));
        self.earliest = self.earliest.min(at);
        let (class, detail) = classify_frames(payload, &encoded);
        self.classes.record(class);
        self.divergences.push(format!(
            "{}#{index}: byte 0x{at:X} of {} (encoded {}):{detail}",
            path.display(),
            payload.len(),
            encoded.len()
        ));
    }

    fn print(&self, total: usize) {
        let decoded = total - self.decode_failures.len();
        let roundtrip = decoded - self.roundtrip_failures.len();
        eprintln!(
            "type-1 codec gate: decode {decoded}/{total}; semantic round-trip \
             {roundtrip}/{total}; byte-exact re-encode {}/{}; first-frame classes: \
             literal-run-length {}, match-length {}, match-distance {}, \
             literal-vs-match {}, frame/end {}; earliest first difference 0x{:X}",
            self.exact,
            total,
            self.classes.literal_run_length,
            self.classes.match_length,
            self.classes.match_distance,
            self.classes.literal_vs_match,
            self.classes.frame_or_end,
            self.earliest
        );
        for divergence in &self.divergences {
            eprintln!("  {divergence}");
        }
    }
}

fn classify_frames(retail: &[u8], encoded: &[u8]) -> (FrameClass, String) {
    match frame_mismatch(retail, encoded) {
        Some((position, retail_frame, encoded_frame)) => {
            let class = match (retail_frame.distance, encoded_frame.distance) {
                (None, None) if retail_frame.length != encoded_frame.length => FrameClass::LiteralRunLength,
                (Some(_), Some(_)) if retail_frame.length != encoded_frame.length => FrameClass::MatchLength,
                (Some(retail), Some(encoded)) if retail != encoded => FrameClass::MatchDistance,
                (None, Some(_)) | (Some(_), None) => FrameClass::LiteralVsMatch,
                _ => FrameClass::FrameOrEnd,
            };
            let detail = format!(
                " class={} output {position}: retail {retail_frame:?}, encoded {encoded_frame:?}",
                class.name()
            );
            (class, detail)
        }
        None => (FrameClass::FrameOrEnd, " class=frame-or-end".to_owned()),
    }
}

/// First output position where the two control streams choose different frames.
fn frame_mismatch(retail: &[u8], mine: &[u8]) -> Option<(usize, Frame, Frame)> {
    let retail_frames = frames(&retail[HEADER..]);
    let my_frames = frames(&mine[HEADER..]);
    let mut position = 0;
    for (retail_frame, my_frame) in retail_frames.iter().zip(my_frames.iter()) {
        if retail_frame != my_frame {
            return Some((position, retail_frame.clone(), my_frame.clone()));
        }
        position += retail_frame.length as usize;
    }
    None
}
