//! `geekcli` — a command-line client for the Real Geeks content & blog API.
//!
//! The binary lives in `main.rs`; everything else is here so integration
//! tests and other tools can reuse it.

pub mod auth;
pub mod cli;
pub mod client;
pub mod commands;
pub mod config;
pub mod error;
pub mod html;
pub mod output;

pub const GUIDE: &str = include_str!("../docs/GUIDE.md");
