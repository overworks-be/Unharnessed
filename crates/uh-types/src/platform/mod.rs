//! Cross-crate platform contracts: process identity and home-dir resolution.
//!
//! uh-types is the only crate every platform consumer can depend on
//! (uh-daemon, uh-cli, uh-core all sit above it), so the shared platform traits live here. Every implementation
//! is cfg-gated per platform: `/proc` and libc today, native process queries
//! on Windows later. Adding a platform means adding an
//! implementation - call sites never branch on `cfg` themselves.

pub mod dirs;
pub mod process;

pub use dirs::{agent_dir, executable_dir, home_dir};
pub use process::{is_process_alive, process_start_id};
