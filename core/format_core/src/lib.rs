//! Format conversion infrastructure for Nornir.
//!
//! Provides parse, serialize, and convert functions for all supported formats:
//! JSON, YAML, TOML, TOON, XML, and Markdown-with-frontmatter (parse only).
//!
//! # Modules
//!
//! - [`parse`] - Format string → serde_json::Value
//! - [`serialize`] - serde_json::Value → format string
//! - [`convert`] - Full conversions with educational error diagnostics
//! - [`yaml_emit`] - The YAML document nornir writes: one fixed style every reader types alike
//!
//! # Legacy interface
//!
//! `toml_to_json` and `json_to_toml` remain at crate root for backward compatibility
//! with existing gate checkers. New code should use `convert::` functions.

pub mod convert;
pub mod parse;
pub mod serialize;
pub mod tomlx;
pub mod write_verify;
pub mod yaml_emit;
mod yaml_scalar;

// Re-export legacy interface for backward compatibility
pub use convert::{json_to_toml, toml_to_json};
