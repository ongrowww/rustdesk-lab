//! Original source modules, no desktop app or replacement transport.
#![allow(dead_code)]
#[cfg(not(ongrow_update_runtime_probe))]
compile_error!("native TLS probe requires its dedicated compile-time marker");
extern crate self as hbb_common;
pub extern crate sodiumoxide;
pub extern crate tokio;
pub extern crate libc;
pub fn get_app_name() -> String { "RustDesk".to_owned() }
#[path = "../../../src/ongrow_update/mod.rs"]
mod ongrow_update;
