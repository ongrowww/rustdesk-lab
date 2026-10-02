//! Only compiles the real gate, never the remote-access application.
#![allow(dead_code)] // Runtime entry points are deliberately not activated.
#[cfg(not(ongrow_session_gate_probe))]
compile_error!("isolated native probe requires its compile-time marker");
extern crate self as hbb_common;
#[cfg(target_os = "macos")]
pub extern crate libc;

#[path = "../../../src/ongrow_update/session_gate.rs"]
mod session_gate;
