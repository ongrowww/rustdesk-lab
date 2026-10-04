//! Isolated native-apply tests import the original gate, never a substitute.
#![allow(dead_code)]
#[cfg(not(ongrow_native_apply_probe))]
compile_error!("native apply probe requires its compile-time marker");
extern crate self as hbb_common;
#[cfg(target_os = "macos")]
pub extern crate libc;

#[path = "../../../src/ongrow_update/session_gate.rs"]
mod session_gate;
