//! Imports the real gate. No desktop application or replacement gate is built.
#![allow(dead_code)]
#[cfg(not(ongrow_pending_recovery_probe))]
compile_error!("isolated pending recovery probe requires its compile-time marker");
extern crate self as hbb_common;
#[cfg(target_os = "macos")]
pub extern crate libc;

#[path = "../../../src/ongrow_update/session_gate.rs"]
mod session_gate;
