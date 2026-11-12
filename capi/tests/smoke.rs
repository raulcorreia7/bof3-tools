//! Runs the C smoke test built by `build.rs`.

#![cfg(feature = "smoke")]

unsafe extern "C" {
    fn rebof3_smoke_run() -> std::ffi::c_int;
}

#[test]
fn the_c_smoke_test_passes() {
    assert_eq!(rebof3_capi::smoke_anchor(), 1);
    let status = unsafe { rebof3_smoke_run() };
    assert_eq!(status, 0, "the C smoke test failed");
}
