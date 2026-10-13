//! Independent, console-free Desk installer guardian. No Flutter or mutable config.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![allow(dead_code)]
pub extern crate libc;
extern crate self as hbb_common;
pub extern crate sodiumoxide;
pub extern crate tokio;
pub fn get_app_name() -> String {
    "OnGROW Support Desk".to_owned()
}
#[path = "../../../src/ongrow_update/mod.rs"]
mod ongrow_update;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    #[cfg(windows)]
    {
        std::hint::black_box(b"ONGROW_WINDOWS_UPDATE_GUARDIAN_V1");
        let arguments = std::env::args().skip(1).collect::<Vec<_>>();
        let result = match arguments.as_slice() {
            [mode] if mode == "--check" => ongrow_update::windows_integration::check_once().await,
            [mode] if mode == "--wait-restart" => {
                ongrow_update::windows_integration::wait_restart(false).await
            }
            [mode] if mode == "--wait-restart-tray" => {
                ongrow_update::windows_integration::wait_restart(true).await
            }
            _ => {
                std::process::exit(1);
            }
        };
        let code = result.unwrap_or(1);
        std::process::exit(code);
    }
    #[cfg(not(windows))]
    std::process::exit(1);
}
