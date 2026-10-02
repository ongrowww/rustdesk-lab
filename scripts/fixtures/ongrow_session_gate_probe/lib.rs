//! Only compiles the real gate, never the remote-access application.
#![allow(dead_code)] // Runtime entry points are deliberately not activated.
extern crate self as hbb_common;
#[cfg(target_os = "macos")]
pub extern crate libc;

#[path = "../../../src/ongrow_update/session_gate.rs"]
mod session_gate;
