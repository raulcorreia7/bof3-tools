//! Checks that the committed header is what cbindgen generates.

use std::path::PathBuf;

#[test]
fn the_committed_header_is_current() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config = cbindgen::Config::from_file(manifest.join("cbindgen.toml")).expect("cbindgen.toml parses");
    let bindings = cbindgen::Builder::new().with_crate(&manifest).with_config(config).generate().expect("cbindgen generates the header");
    let mut generated = Vec::new();
    bindings.write(&mut generated);
    let header = manifest.join("include/rebof3_capi.h");
    if std::env::var_os("UPDATE_HEADER").is_some() {
        std::fs::write(&header, &generated).expect("header written");
        return;
    }
    let committed = std::fs::read(&header).expect("the committed header is readable");
    if generated != committed {
        let generated = String::from_utf8_lossy(&generated);
        let committed = String::from_utf8_lossy(&committed);
        let line = generated.lines().zip(committed.lines()).position(|(left, right)| left != right).map(|index| index + 1);
        panic!("capi/include/rebof3_capi.h is stale{}; run `UPDATE_HEADER=1 cargo test -p rebof3-capi --test header`", line.map(|line| format!(" at line {line}")).unwrap_or_default());
    }
}
