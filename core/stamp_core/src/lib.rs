//! Untamperable record stamps.
//!
//! A stamp is a field the writer sets and the author may not: a sequence number
//! derived from the target file's length, a timestamp from the clock, a session
//! id derived from the first record of the file. The author's record arrives
//! without them; a record that carries one is refused, never overwritten.
//!
//! Pure: every value comes in through [`StampContext`]. The engine that owns the
//! file and the clock gathers the context; this crate only applies the rules.

use serde_json::{Map, Value};
use thiserror::Error;

/// Which fields a writer stamps, by name. `None` means that stamp is not applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamps {
    /// Set to the target file's line count + 1.
    pub sequence: Option<&'static str>,
    /// Set to the current time as an ISO 8601 Zulu timestamp.
    pub timestamp: Option<&'static str>,
    /// Set to the file's session id: minted from the first record's timestamp,
    /// copied from the first record thereafter.
    pub session: Option<&'static str>,
}

impl Stamps {
    /// No stamps at all — the author's record is validated and written as given.
    pub const NONE: Stamps = Stamps {
        sequence: None,
        timestamp: None,
        session: None,
    };

    /// The stamp field names, in a fixed order, for messages and checks.
    pub fn field_names(&self) -> Vec<&'static str> {
        [self.sequence, self.timestamp, self.session]
            .into_iter()
            .flatten()
            .collect()
    }

    /// Whether any stamp is configured.
    pub fn is_none(&self) -> bool {
        self.field_names().is_empty()
    }
}

/// What the engine measured before stamping one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StampContext {
    /// Records already in the target file.
    pub line_count: u64,
    /// Current time, Unix epoch seconds.
    pub now_epoch: u64,
    /// The session id carried by the file's first record, when the file has one.
    pub session_id: Option<String>,
}

/// Why a record could not be stamped.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StampError {
    #[error("record must be a JSON object, got {0}")]
    NotAnObject(&'static str),
    #[error("field {0:?} is stamped by the writer and must not be supplied")]
    FieldSupplied(String),
    #[error("file already holds {line_count} records but its first record carries no {field:?} — not a session file this writer can continue")]
    SessionMissingFromFile { field: String, line_count: u64 },
    #[error("first record's {field:?} is not a string: {found}")]
    SessionNotAString { field: String, found: String },
}

/// Apply the configured stamps to one record.
///
/// Order: the author's fields are checked for collisions first, so a refusal
/// never depends on the context; then sequence, timestamp and session are set.
/// For the first record of a file (`line_count == 0`) the session id is minted
/// from the stamped timestamp in basic ISO form; for every later record it is
/// the id the context read from the file's first record.
pub fn apply(record: Value, stamps: &Stamps, context: &StampContext) -> Result<Value, StampError> {
    let mut object = into_object(record)?;
    refuse_supplied_fields(&object, stamps)?;

    if let Some(field) = stamps.sequence {
        object.insert(field.to_string(), Value::from(context.line_count + 1));
    }
    if let Some(field) = stamps.timestamp {
        object.insert(field.to_string(), Value::from(time_core::iso_zulu(context.now_epoch)));
    }
    if let Some(field) = stamps.session {
        let session_id = session_id_for(field, context)?;
        object.insert(field.to_string(), Value::from(session_id));
    }
    Ok(Value::Object(object))
}

/// Read the session id a file's first record carries in `field`.
///
/// The engine hands over the first line of the file; this is the only parsing
/// the session stamp needs, so it lives beside the rule that uses it.
pub fn session_id_of_first_record(first_line_json: &str, field: &str) -> Result<String, StampError> {
    let record: Value = serde_json::from_str(first_line_json).map_err(|_| StampError::NotAnObject("unparseable first line"))?;
    let object = match record {
        Value::Object(object) => object,
        other => return Err(StampError::NotAnObject(value_kind(&other))),
    };
    match object.get(field) {
        Some(Value::String(id)) => Ok(id.clone()),
        Some(other) => Err(StampError::SessionNotAString {
            field: field.to_string(),
            found: other.to_string(),
        }),
        None => Err(StampError::SessionMissingFromFile {
            field: field.to_string(),
            line_count: 1,
        }),
    }
}

fn into_object(record: Value) -> Result<Map<String, Value>, StampError> {
    match record {
        Value::Object(object) => Ok(object),
        other => Err(StampError::NotAnObject(value_kind(&other))),
    }
}

fn refuse_supplied_fields(object: &Map<String, Value>, stamps: &Stamps) -> Result<(), StampError> {
    match stamps.field_names().into_iter().find(|field| object.contains_key(*field)) {
        Some(field) => Err(StampError::FieldSupplied(field.to_string())),
        None => Ok(()),
    }
}

fn session_id_for(field: &str, context: &StampContext) -> Result<String, StampError> {
    match (&context.session_id, context.line_count) {
        (Some(id), _) => Ok(id.clone()),
        (None, 0) => Ok(time_core::iso_basic(context.now_epoch)),
        (None, line_count) => Err(StampError::SessionMissingFromFile {
            field: field.to_string(),
            line_count,
        }),
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ALL: Stamps = Stamps {
        sequence: Some("exchange"),
        timestamp: Some("ts"),
        session: Some("session"),
    };

    // 2024-01-01T12:30:45Z
    const NOW: u64 = 1704112245;

    fn first_record_context() -> StampContext {
        StampContext {
            line_count: 0,
            now_epoch: NOW,
            session_id: None,
        }
    }

    fn later_record_context() -> StampContext {
        StampContext {
            line_count: 12,
            now_epoch: NOW + 60,
            session_id: Some("20240101T123045Z".to_string()),
        }
    }

    #[test]
    fn first_record_gets_sequence_one_timestamp_and_minted_session() {
        let stamped = apply(json!({"agent": "q", "stt": "a"}), &ALL, &first_record_context()).unwrap();
        assert_eq!(stamped["exchange"], json!(1));
        assert_eq!(stamped["ts"], json!("2024-01-01T12:30:45Z"));
        assert_eq!(stamped["session"], json!("20240101T123045Z"));
        assert_eq!(stamped["agent"], json!("q"));
        assert_eq!(stamped["stt"], json!("a"));
    }

    #[test]
    fn later_record_copies_the_file_session_and_counts_on() {
        let stamped = apply(json!({"agent": "q", "stt": "a"}), &ALL, &later_record_context()).unwrap();
        assert_eq!(stamped["exchange"], json!(13));
        assert_eq!(stamped["ts"], json!("2024-01-01T12:31:45Z"));
        assert_eq!(stamped["session"], json!("20240101T123045Z"));
    }

    #[test]
    fn supplied_stamp_field_is_refused_not_overwritten() {
        let err = apply(json!({"exchange": 99, "stt": "a"}), &ALL, &first_record_context()).unwrap_err();
        assert_eq!(err, StampError::FieldSupplied("exchange".to_string()));

        let err = apply(json!({"session": "mine", "stt": "a"}), &ALL, &first_record_context()).unwrap_err();
        assert_eq!(err, StampError::FieldSupplied("session".to_string()));
    }

    #[test]
    fn refusal_does_not_depend_on_context() {
        let bad_context = StampContext {
            line_count: 5,
            now_epoch: NOW,
            session_id: None,
        };
        let err = apply(json!({"ts": "x"}), &ALL, &bad_context).unwrap_err();
        assert_eq!(err, StampError::FieldSupplied("ts".to_string()));
    }

    #[test]
    fn non_empty_file_without_session_is_refused() {
        let context = StampContext {
            line_count: 5,
            now_epoch: NOW,
            session_id: None,
        };
        let err = apply(json!({"stt": "a"}), &ALL, &context).unwrap_err();
        assert_eq!(
            err,
            StampError::SessionMissingFromFile {
                field: "session".to_string(),
                line_count: 5
            }
        );
    }

    #[test]
    fn no_stamps_leaves_record_untouched() {
        let record = json!({"exchange": 7, "stt": "a"});
        let stamped = apply(record.clone(), &Stamps::NONE, &later_record_context()).unwrap();
        assert_eq!(stamped, record);
    }

    #[test]
    fn only_configured_stamps_are_applied() {
        let only_sequence = Stamps {
            sequence: Some("n"),
            timestamp: None,
            session: None,
        };
        let stamped = apply(json!({"ts": "author's own"}), &only_sequence, &later_record_context()).unwrap();
        assert_eq!(stamped["n"], json!(13));
        assert_eq!(stamped["ts"], json!("author's own"));
        assert!(stamped.get("session").is_none());
    }

    #[test]
    fn non_object_record_is_refused() {
        let err = apply(json!([1, 2]), &ALL, &first_record_context()).unwrap_err();
        assert_eq!(err, StampError::NotAnObject("an array"));
    }

    #[test]
    fn field_names_follow_configuration() {
        assert_eq!(ALL.field_names(), vec!["exchange", "ts", "session"]);
        assert!(Stamps::NONE.field_names().is_empty());
        assert!(Stamps::NONE.is_none());
        assert!(!ALL.is_none());
    }

    #[test]
    fn session_id_of_first_record_reads_the_field() {
        let line = r#"{"exchange":1,"session":"20240101T123045Z","stt":"a"}"#;
        assert_eq!(session_id_of_first_record(line, "session").unwrap(), "20240101T123045Z");
    }

    #[test]
    fn session_id_of_first_record_refuses_missing_or_wrong_type() {
        let missing = r#"{"exchange":1,"stt":"a"}"#;
        assert!(matches!(
            session_id_of_first_record(missing, "session").unwrap_err(),
            StampError::SessionMissingFromFile { .. }
        ));
        let wrong = r#"{"session":42}"#;
        assert!(matches!(
            session_id_of_first_record(wrong, "session").unwrap_err(),
            StampError::SessionNotAString { .. }
        ));
        assert!(session_id_of_first_record("not json", "session").is_err());
    }
}
