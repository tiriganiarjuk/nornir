//! .tomlx (Extended TOML) parser with semantic annotations.
//!
//! Parses TOML files with semantic annotations in comments, enabling
//! human-friendly configuration with machine-optimal output.
//!
//! # Design Philosophy
//!
//! ```text
//! Human wants:     session = 15  # minutes (scannable, intuitive)
//! Machine needs:   SESSION_TTL_SECONDS = 900 (efficient, typed)
//! ```
//!
//! **Solution:** Parse-time transformation with preserved metadata.
//!
//! # Supported Modes
//!
//! - **Unit conversion** (`target=seconds`, `target=bytes`, `target=meters`, and the
//!   other unit families), with angles in degrees and minutes read from quoted strings
//! - **Path expansion** (`target=path, base=~/.app/`)
//! - **Type declarations** (`type=int`, `type=list[str]`)
//! - **Combined** (`type=int, target=seconds`)
//!
//! # Usage
//!
//! ```ignore
//! use format_core::tomlx;
//!
//! let resolve_env = |name: &str| std::env::var(name).ok();
//! let output = tomlx::parse_tomlx(content, None, None, &resolve_env)?;
//! let json = output.to_json();
//! ```

pub mod angle_parse;
pub mod annotation;
pub mod diagnostics;
pub mod paths;
pub mod processor;
pub mod types;
pub mod units;
pub mod validation;

pub use processor::parse_tomlx;
pub use types::TomlxOutput;
