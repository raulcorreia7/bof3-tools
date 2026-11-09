// SPDX-License-Identifier: MIT
//! The conversion commands' output and messages for small synthetic inputs. The expected text is `tests/convert.snap`; `UPDATE_SNAPSHOT=1` rewrites it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One 16-color row: a red entry and fifteen white ones.
fn palette() -> Vec<u8> {
    let mut words = vec![0x1f, 0x00];
    words.extend([0xff, 0x7f].repeat(15));
    words
}

/// A one-section script table: a stream of two frames that loops on its second step, and two bytes of padding.
fn script_table() -> Vec<u8> {
    let stream = [
        2, 3, // frames, steps
        4, 0, 6, 1, 0x80, 1, // show 0 for 4, show 1 for 6, jump to step 1
        4, 0, 10, 0, // the frame table
        1, 5, 0xF8, 0xF0, 0, 0, // frame 0: one cell
        1, 0x85, 0xF8, 0xF0, 16, 0, // frame 1: one flipped cell
        0, 0, // padding
    ];
    [vec![4, 0, 0, 0, 2, 0], stream.to_vec()].concat()
}

fn run(dir: &Path, args: &[&str], out: &mut String) {
    let run = Command::new(env!("CARGO_BIN_EXE_bof3")).args(args).current_dir(dir).output().expect("bof3 runs");
    writeln!(out, "$ bof3 {}", args.join(" ")).unwrap();
    writeln!(out, "[exit {}]", run.status.code().unwrap_or(-1)).unwrap();
    out.push_str(&String::from_utf8_lossy(&run.stdout));
    out.push_str(&String::from_utf8_lossy(&run.stderr));
    out.push('\n');
}

fn render() -> String {
    let dir = std::env::temp_dir().join(format!("bof3-convert-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("block.clut"), palette()).unwrap();
    std::fs::write(dir.join("stub.bin"), [0u8; 8]).unwrap();
    let mut out = String::new();
    run(&dir, &["assets", "export", "palette", "block.clut"], &mut out);
    out.push_str(&std::fs::read_to_string(dir.join("block.json")).unwrap());
    out.push_str("\n\n");
    run(&dir, &["assets", "import", "palette", "block.json", "-o", "sub/back.clut"], &mut out);
    assert_eq!(std::fs::read(dir.join("sub/back.clut")).unwrap(), palette());
    run(&dir, &["assets", "export", "palette", "block.clut", "--out", "other.json"], &mut out);
    run(&dir, &["assets", "export", "palette"], &mut out);
    run(&dir, &["assets", "export", "palette", "a", "b"], &mut out);
    run(&dir, &["assets", "export", "bogus", "a"], &mut out);
    run(&dir, &["assets", "export"], &mut out);
    std::fs::write(dir.join("table.emi0"), script_table()).unwrap();
    run(&dir, &["assets", "export", "script", "table.emi0"], &mut out);
    out.push_str(&std::fs::read_to_string(dir.join("table.txt")).unwrap());
    out.push('\n');
    run(&dir, &["assets", "import", "script", "table.txt", "--payload", "table.emi0", "-o", "back.emi0"], &mut out);
    assert_eq!(std::fs::read(dir.join("back.emi0")).unwrap(), script_table());
    run(&dir, &["assets", "import", "script", "table.txt"], &mut out);
    run(&dir, &["assets", "import", "strings", "stub.bin"], &mut out);
    run(&dir, &["assets", "import", "encounters", "stub.bin"], &mut out);
    run(&dir, &["assets", "export", "vag", "stub.bin", "stub.bin"], &mut out);
    run(&dir, &["assets", "export", "vag", "stub.bin"], &mut out);
    run(&dir, &["assets", "export", "texture", "stub.bin"], &mut out);
    run(&dir, &["assets", "export", "texture", "stub.bin", "--load-arg", "zz"], &mut out);
    run(&dir, &["assets", "import", "emi", "stub.bin", "--entry", "oops"], &mut out);
    run(&dir, &["assets", "import", "sequence", "stub.bin"], &mut out);
    run(&dir, &["assets", "import", "disc", "game.cue"], &mut out);
    run(&dir, &["assets", "tables"], &mut out);
    run(&dir, &["assets", "bogus"], &mut out);
    let _ = std::fs::remove_dir_all(&dir);
    out.replace('\\', "/")
}

#[test]
fn conversions_keep_their_output_and_messages() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/convert.snap");
    let actual = render();
    if std::env::var_os("UPDATE_SNAPSHOT").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .expect("tests/convert.snap exists; run with UPDATE_SNAPSHOT=1")
        .replace("\r\n", "\n");
    assert!(actual == expected, "conversion output changed; rerun with UPDATE_SNAPSHOT=1 and read the diff");
}
