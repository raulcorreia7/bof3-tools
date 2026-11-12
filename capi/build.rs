//! Builds the C smoke test for the test build only.

#[cfg(feature = "smoke")]
fn main() {
    println!("cargo:rerun-if-changed=tests/smoke.c");
    cc::Build::new().file("tests/smoke.c").include("include").compile("rebof3_capi_smoke");
}

#[cfg(not(feature = "smoke"))]
fn main() {}
