//! The `rebof3_` C functions over the EMI, format and asset libraries; the shared layer is `psx-capi`.

#![allow(non_camel_case_types)]

mod assets;
mod emi;
mod error;
mod formats;

pub use psx_capi::*;

pub const REBOF3_ABI_VERSION: u32 = 2;

#[unsafe(no_mangle)]
pub extern "C" fn rebof3_abi_version() -> u32 {
    REBOF3_ABI_VERSION
}

#[doc(hidden)]
pub fn smoke_anchor() -> u32 {
    rebof3_abi_version()
}
