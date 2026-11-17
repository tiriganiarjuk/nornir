//! The IO of a session event hook: read the Claude Code event from stdin as the
//! payload its schema describes, typed, and hand it to the hook.
//!
//!   run_event_hook  — the entry point: read the event, act on it, or refuse
//!   parse_event     — a payload as one typed event
//!   SessionStart, SessionEnd, Stop, Notification, PostCompact — the events
//!   SessionId       — a session id that is one plain path segment
//!   project_dir     — the directory the session belongs to
//!
//! Each event's shape on the wire is its schema in `schemas/tools/`. A hook that
//! cannot read its event does nothing else: the failure goes to stderr under the
//! CLI contract — its class, then its message — the hook exits 1, and a payload
//! that arrived and was refused is raised to Hlidskjalf as an alert.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use error_core::SchemaError;
use failure_mode_core::Failure;
use schema_core::EmbeddedValidator;
use serde::de::DeserializeOwned;
use serde::Deserialize;

/// A session id that is one plain path segment, so it can name a directory.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct SessionId(String);

impl SessionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SessionId {
    type Error = String;

    fn try_from(session_id: String) -> Result<Self, Self::Error> {
        path_core::validate_path_segment(&session_id)?;
        Ok(Self(session_id))
    }
}

impl AsRef<Path> for SessionId {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One Claude Code session event: its name and the schema of its payload.
pub trait SessionEvent: DeserializeOwned {
    /// The event's name, as Claude Code names it.
    const NAME: &'static str;

    /// The schema of the event's payload.
    fn schema() -> &'static EmbeddedValidator;
}

/// A session started: `schemas/tools/hook-session-start.schema.json`.
#[derive(Debug, Deserialize)]
pub struct SessionStart {
    pub session_id: SessionId,
    /// The directory the session is working in.
    pub cwd: String,
    /// What started it: startup, resume, clear or compact.
    pub source: String,
}

/// A session ended: `schemas/tools/hook-session-end.schema.json`.
#[derive(Debug, Deserialize)]
pub struct SessionEnd {
    pub session_id: SessionId,
    pub cwd: String,
    /// Why it ended: clear, logout, and the others Claude Code names.
    pub reason: String,
}

/// The assistant finished a turn: `schemas/tools/hook-stop.schema.json`.
#[derive(Debug, Deserialize)]
pub struct Stop {
    pub cwd: String,
    pub last_assistant_message: String,
}

/// Claude Code notified the user: `schemas/tools/hook-notification.schema.json`.
#[derive(Debug, Deserialize)]
pub struct Notification {
    pub cwd: String,
    pub notification_type: String,
}

/// A session was compacted: `schemas/tools/hook-post-compact.schema.json`.
#[derive(Debug, Deserialize)]
pub struct PostCompact {
    pub session_id: SessionId,
    pub cwd: String,
    pub compact_summary: String,
}

impl SessionEvent for SessionStart {
    const NAME: &'static str = "SessionStart";
    fn schema() -> &'static EmbeddedValidator {
        &schemas_embedded::HOOK_SESSION_START
    }
}

impl SessionEvent for SessionEnd {
    const NAME: &'static str = "SessionEnd";
    fn schema() -> &'static EmbeddedValidator {
        &schemas_embedded::HOOK_SESSION_END
    }
}

impl SessionEvent for Stop {
    const NAME: &'static str = "Stop";
    fn schema() -> &'static EmbeddedValidator {
        &schemas_embedded::HOOK_STOP
    }
}

impl SessionEvent for Notification {
    const NAME: &'static str = "Notification";
    fn schema() -> &'static EmbeddedValidator {
        &schemas_embedded::HOOK_NOTIFICATION
    }
}

impl SessionEvent for PostCompact {
    const NAME: &'static str = "PostCompact";
    fn schema() -> &'static EmbeddedValidator {
        &schemas_embedded::HOOK_POST_COMPACT
    }
}

/// A payload as the event `E`: valid against the event's schema, its session id
/// one plain path segment, then typed.
pub fn parse_event<E: SessionEvent>(payload: &str) -> Result<E, Failure> {
    let schema = E::schema();
    let checked = schema.validate(payload).map_err(|schema_err| match schema_err {
        SchemaError::InvalidInput(reason) => failure_mode_core::session_event_not_json(E::NAME, &reason),
        other => failure_mode_core::embedded_schema_unusable(schema.schema_name(), &other.to_string()),
    })?;
    let Some(event) = checked.data.filter(|_| checked.valid) else {
        return Err(failure_mode_core::session_event_not_matching_schema(E::NAME, &checked.message));
    };
    if let Some(session_id) = event.get("session_id").and_then(|id| id.as_str()) {
        path_core::validate_path_segment(session_id)
            .map_err(|reason| failure_mode_core::session_id_not_plain(session_id, &reason))?;
    }
    serde_json::from_value(event)
        .map_err(|typing_err| failure_mode_core::session_event_not_matching_schema(E::NAME, &typing_err.to_string()))
}

/// The directory the session belongs to: the one named on the command line,
/// else the directory the event says the session is working in.
pub fn project_dir(named: Option<&Path>, event_cwd: &str) -> PathBuf {
    named.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(event_cwd))
}

/// Run a session event hook: read the event `E` from stdin and hand it to
/// `act_on_event`.
///
/// An event that cannot be read is not acted on: the failure is printed on
/// stderr, a payload that arrived is raised as an alert, and the hook exits 1.
pub fn run_event_hook<E, F>(act_on_event: F) -> ExitCode
where
    E: SessionEvent,
    F: FnOnce(E),
{
    let mut payload = String::new();
    let event = match std::io::stdin().read_to_string(&mut payload) {
        Ok(_) => parse_event::<E>(&payload),
        Err(io_err) => Err(failure_mode_core::session_event_unreadable(E::NAME, &io_err.to_string())),
    };
    match event {
        Ok(event) => {
            act_on_event(event);
            ExitCode::SUCCESS
        }
        Err(failure) => {
            eprintln!("{}\n{failure}", failure.error_type());
            if !payload.trim().is_empty() {
                alert_event_refused(&failure);
            }
            ExitCode::from(1)
        }
    }
}

fn hook_name() -> String {
    std::env::args()
        .next()
        .and_then(|invoked| Path::new(&invoked).file_name().map(|name| name.to_string_lossy().to_string()))
        .unwrap_or_else(|| "hook".to_string())
}

fn alert_event_refused(failure: &Failure) {
    let datagram = datagram_io::Datagram {
        timestamp: datagram_io::now(),
        source: hook_name(),
        kind: datagram_io::DatagramKind::Alert,
        classifier: None,
        priority: failure.alert.unwrap_or(datagram_io::Priority::High),
        workspace: datagram_io::workspace_name(),
        detail: Some(failure.standard_message()),
        speech: None,
        payload: Some(serde_json::json!({ "failure_mode": failure.mode, "class": failure.class })),
    };
    datagram_io::emit(&datagram);
}

#[cfg(test)]
mod tests {
    use super::*;

    const STARTED: &str =
        r#"{"session_id":"1213cd9f-1525","cwd":"/w/nornir","source":"compact","hook_event_name":"SessionStart"}"#;

    // ── A payload its schema describes is the event ──────────────

    #[test]
    fn a_session_start_payload_is_the_event() {
        let started: SessionStart = parse_event(STARTED).unwrap();
        assert_eq!(started.session_id.as_str(), "1213cd9f-1525");
        assert_eq!(started.cwd, "/w/nornir");
        assert_eq!(started.source, "compact");
    }

    #[test]
    fn a_session_end_payload_is_the_event() {
        let ended: SessionEnd = parse_event(r#"{"session_id":"s-1","cwd":"/w","reason":"clear"}"#).unwrap();
        assert_eq!(ended.reason, "clear");
    }

    #[test]
    fn a_stop_payload_is_the_event_and_its_message_may_be_empty() {
        let stopped: Stop = parse_event(r#"{"cwd":"/w","last_assistant_message":""}"#).unwrap();
        assert_eq!(stopped.last_assistant_message, "");
    }

    #[test]
    fn a_notification_payload_is_the_event() {
        let notified: Notification = parse_event(r#"{"cwd":"/w","notification_type":"idle_prompt"}"#).unwrap();
        assert_eq!(notified.notification_type, "idle_prompt");
    }

    #[test]
    fn a_post_compact_payload_is_the_event() {
        let compacted: PostCompact =
            parse_event(r#"{"session_id":"s-1","cwd":"/w","compact_summary":"what happened"}"#).unwrap();
        assert_eq!(compacted.compact_summary, "what happened");
    }

    // ── A payload its schema does not describe is refused ────────

    #[test]
    fn a_payload_with_a_renamed_field_is_refused() {
        let renamed = r#"{"sessionId":"s-1","cwd":"/w","source":"startup"}"#;
        let failure = parse_event::<SessionStart>(renamed).unwrap_err();
        assert_eq!(failure.mode, "session_event_not_matching_schema");
        assert_eq!(failure.class, "validation_error");
        assert!(failure.standard.contains("SessionStart"), "{}", failure.standard);
    }

    #[test]
    fn every_event_refuses_a_payload_without_its_fields() {
        let other_event = r#"{"tool_name":"Test","tool_input":{}}"#;
        assert!(parse_event::<SessionStart>(other_event).is_err());
        assert!(parse_event::<SessionEnd>(other_event).is_err());
        assert!(parse_event::<Stop>(other_event).is_err());
        assert!(parse_event::<Notification>(other_event).is_err());
        assert!(parse_event::<PostCompact>(other_event).is_err());
    }

    #[test]
    fn an_empty_field_that_must_name_something_is_refused() {
        let no_directory = r#"{"session_id":"s-1","cwd":"","source":"startup"}"#;
        assert!(parse_event::<SessionStart>(no_directory).is_err());
    }

    #[test]
    fn text_that_is_not_json_is_refused_as_a_parse_error() {
        for payload in ["", "not json"] {
            let failure = parse_event::<Stop>(payload).unwrap_err();
            assert_eq!(failure.mode, "session_event_not_json", "{payload:?}");
            assert_eq!(failure.class, "parse_error");
        }
    }

    // ── A session id is one plain path segment ───────────────────

    #[test]
    fn a_session_id_that_leaves_its_directory_is_refused() {
        for session_id in ["../../.ssh", "a/b", "..", ".hidden", "-flag"] {
            let payload = serde_json::json!({ "session_id": session_id, "cwd": "/w", "source": "startup" }).to_string();
            let failure = parse_event::<SessionStart>(&payload).unwrap_err();
            assert_eq!(failure.mode, "session_id_not_plain", "{session_id}");
            let compacted = serde_json::json!({ "session_id": session_id, "cwd": "/w", "compact_summary": "s" }).to_string();
            assert!(parse_event::<PostCompact>(&compacted).is_err(), "{session_id}");
            let ended = serde_json::json!({ "session_id": session_id, "cwd": "/w", "reason": "clear" }).to_string();
            assert!(parse_event::<SessionEnd>(&ended).is_err(), "{session_id}");
        }
    }

    #[test]
    fn a_session_id_cannot_be_made_from_a_path() {
        assert!(SessionId::try_from("a/b".to_string()).is_err());
        assert!(SessionId::try_from("1213cd9f-1525-48bd".to_string()).is_ok());
    }

    #[test]
    fn a_session_id_joins_as_one_segment() {
        let session_id = SessionId::try_from("s-1".to_string()).unwrap();
        assert_eq!(Path::new("/control/nornir").join(&session_id), Path::new("/control/nornir/s-1"));
    }

    // ── project_dir ──────────────────────────────────────────────

    #[test]
    fn the_directory_named_on_the_command_line_is_the_project_dir() {
        assert_eq!(project_dir(Some(Path::new("/named")), "/cwd"), Path::new("/named"));
    }

    #[test]
    fn without_one_the_event_directory_is_the_project_dir() {
        assert_eq!(project_dir(None, "/cwd"), Path::new("/cwd"));
    }
}
