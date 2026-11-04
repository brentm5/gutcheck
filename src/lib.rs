//! Core library for `gutcheck`.
//!
//! The CLI (`src/main.rs`) and the MCP server are thin front ends over this
//! crate. See `docs/language-and-distribution.md`.

pub mod client;
pub mod config;
pub mod error;

pub use client::{Client, api_key_from_env};
pub use config::{Overrides, Settings, parse_duration};
pub use error::Error;
