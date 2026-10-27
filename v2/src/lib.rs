//! Standalone source-compatible palette engine. The host compiles tezbar_adapter.rs
//! beside its real v2_sdk module; headless tests use byte-exact beta SDK/common files.
mod engine;
pub use engine::*;
#[cfg(test)]
#[path = "../tests/host.rs"]
pub mod native_core;
#[cfg(test)]
extern crate self as tezbar_color_shades;
#[cfg(test)]
#[path = "../tests/parity.rs"]
mod parity;

#[cfg(test)]
extern crate self as tauri;
#[cfg(test)]
#[path = "../fixtures/tauri-stub/src/lib.rs"]
mod tauri_stub;
#[cfg(test)]
pub use tauri_stub::{AppHandle, Emitter};
