//! The error registry: every failure mode a nornir tool can refuse or fail with, written once
//! in `failure_modes.toml` and compiled into one function per mode
//! (`HOW_A_NORNIR_TOOL_IS_MADE.md` § The error registry).
//!
//! An engine never writes failure text of its own. It calls the mode's function with the
//! mode's slots and carries the `Failure` it gets back:
//!
//! ```
//! let failure = failure_mode_core::file_unreadable("/data/config.toml", "No such file or directory");
//! assert_eq!(failure.class, "read_error");
//! ```
//!
//! A mode with no entry, or a slot left unfilled, does not compile.

use std::fmt;

use datagram_core::Priority;

/// What a tool built in adversarial mode tells its user, whatever the failure.
pub const STOP_ORDER: &str = "This tool refused the input. Do not retry with variations. \
                              Stop, and report to your caller what you were trying to do.";

/// One failure, made from its registry entry with its slots filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The registry entry's name.
    pub mode: &'static str,
    /// The `error.type` callers branch on.
    pub class: &'static str,
    /// What was refused or failed, and where.
    pub standard: String,
    /// What to do next.
    pub route: String,
    /// The Hlidskjalf Alert priority in adversarial mode; `None` sends no alert.
    pub alert: Option<Priority>,
}

impl Failure {
    /// The full message of standard mode: what, where, and the route.
    pub fn standard_message(&self) -> String {
        format!("{} {}", self.standard, self.route)
    }

    /// The message of adversarial mode: the shared stop order, and nothing a user could probe with.
    pub fn adversarial_message(&self) -> &'static str {
        STOP_ORDER
    }

    /// The failure class — the signature `tool_face_io` takes as an engine's `error_type`.
    pub fn error_type(&self) -> &'static str {
        self.class
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.standard_message())
    }
}

include!(concat!(env!("OUT_DIR"), "/failure_modes.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_fills_its_slots_into_its_message() {
        let failure = file_unreadable("/data/config.toml", "No such file or directory");
        assert_eq!(failure.mode, "file_unreadable");
        assert_eq!(failure.class, "read_error");
        assert_eq!(failure.standard, "Cannot read '/data/config.toml': No such file or directory.");
    }

    #[test]
    fn the_standard_message_ends_with_the_route() {
        let failure = file_unwritable("/out/report.json", "Permission denied");
        assert!(failure.to_string().starts_with("Cannot write '/out/report.json': Permission denied."));
        assert!(failure.to_string().ends_with(&failure.route));
    }

    #[test]
    fn the_adversarial_message_withholds_everything_but_the_stop_order() {
        let failure = file_unreadable("/etc/secret", "Permission denied");
        assert_eq!(failure.adversarial_message(), STOP_ORDER);
        assert!(!failure.adversarial_message().contains("/etc/secret"));
    }

    #[test]
    fn the_alert_is_the_entry_s_priority_or_none() {
        assert_eq!(file_unreadable("p", "r").alert, Some(Priority::High));
        assert_eq!(text_not_json("p", "r").alert, None);
    }

    #[test]
    fn every_schema_failure_is_a_schema_error() {
        for failure in [schema_unreadable("s", "r"), schema_not_json("s", "r"), schema_not_compilable("s", "r")] {
            assert_eq!(failure.error_type(), "schema_error");
        }
    }
}
