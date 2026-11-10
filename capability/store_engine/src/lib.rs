//! Store engine: close an open append file as one stored session.
//!
//! The counterpart of `write_engine`. A writer appends records to one open
//! file; the store tool ends that file's life as a session:
//!
//! 1. refuse an empty open file
//! 2. derive the session id from the first record's stamped session field
//! 3. copy the open file to `sessions/<id>.jsonl` (temp + fsync + rename)
//! 4. verify the copy byte-for-byte (SHA-256) — nothing is destroyed before this
//! 5. append one validated session record to the index (fsync)
//! 6. truncate the open file in place (inode kept), ready for the next session
//!
//! The session record carries only what the file and the clock say (id,
//! opened, closed) plus the closing note given on the command line (about,
//! tags, prior). It never carries counts of the file's contents.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use schema_core::EmbeddedValidator;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

// =============================================================================
// Configuration
// =============================================================================

/// Complete configuration for a store binary.
pub struct StoreConfig {
    /// Binary name for --help and error messages.
    pub name: &'static str,
    /// The open append file a writer fills.
    pub open_file: PathBuf,
    /// Where stored sessions go, as `<id>.jsonl`.
    pub sessions_dir: PathBuf,
    /// The append-only session index.
    pub index_file: PathBuf,
    /// Embedded schema for one index record.
    pub index_schema: &'static EmbeddedValidator,
    /// Path to the index schema source (for --help display only).
    pub index_schema_source_path: String,
    /// Record field carrying the stamped session id.
    pub session_field: &'static str,
    /// Record field carrying the stamped timestamp.
    pub timestamp_field: &'static str,
}

/// The closing note, as given on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosingNote {
    pub about: String,
    pub tags: Vec<String>,
    pub prior: Vec<String>,
}

// =============================================================================
// Errors
// =============================================================================

#[derive(Debug)]
enum StoreError {
    Usage(String),
    OpenFileMissing(PathBuf),
    OpenFileEmpty(PathBuf),
    FirstRecord(String),
    SessionsDirMissing(PathBuf),
    AlreadyStored(PathBuf),
    CopyMismatch { source: PathBuf, copy: PathBuf },
    IndexRecordInvalid(String),
    IoFailed(String),
}

impl StoreError {
    fn format(&self, config: &StoreConfig) -> String {
        match self {
            StoreError::Usage(msg) => format!("FAIL:usage — {msg}\n\nRun: {} --help", config.name),
            StoreError::OpenFileMissing(path) => format!(
                "FAIL:open file does not exist — nothing to store.\n\nExpected: {}",
                path.display()
            ),
            StoreError::OpenFileEmpty(path) => format!(
                "FAIL:open file is empty — nothing to store.\n\nFile: {}\n\
                 Write at least one record before storing a session.",
                path.display()
            ),
            StoreError::FirstRecord(msg) => format!(
                "FAIL:first record — {msg}\n\n\
                 The open file must begin with a record stamped by its writer."
            ),
            StoreError::SessionsDirMissing(path) => format!(
                "FAIL:sessions directory does not exist.\n\nExpected: {}\nRun: mkdir -p {}",
                path.display(),
                path.display()
            ),
            StoreError::AlreadyStored(path) => format!(
                "FAIL:a session with this id is already stored — refusing to overwrite.\n\n\
                 Path: {}\nThe open file was left untouched.",
                path.display()
            ),
            StoreError::CopyMismatch { source, copy } => format!(
                "FAIL:copy verification failed — the stored copy does not match the open file.\n\n\
                 Source: {}\nCopy:   {} (removed)\nThe open file was left untouched.",
                source.display(),
                copy.display()
            ),
            StoreError::IndexRecordInvalid(msg) => format!(
                "FAIL:session record — {msg}\n\nThe session copy exists; the index was not written and the open file was left untouched."
            ),
            StoreError::IoFailed(msg) => format!("FAIL:IO error — {msg}"),
        }
    }
}

// =============================================================================
// Argument parsing (pure over the argument list)
// =============================================================================

/// Parse `--about <text>`, repeatable `--tag <slug>` and `--prior <id>`.
fn parse_note(args: &[String]) -> Result<ClosingNote, StoreError> {
    let mut about = None;
    let mut tags = Vec::new();
    let mut prior = Vec::new();
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| StoreError::Usage(format!("{flag} needs a value")))?;
        match flag.as_str() {
            "--about" => about = Some(value.clone()),
            "--tag" => tags.push(value.clone()),
            "--prior" => prior.push(value.clone()),
            other => return Err(StoreError::Usage(format!("unknown argument {other:?}"))),
        }
    }
    let about = about.ok_or_else(|| StoreError::Usage("--about is required".to_string()))?;
    if about.trim().is_empty() {
        return Err(StoreError::Usage("--about must not be empty".to_string()));
    }
    Ok(ClosingNote { about, tags, prior })
}

// =============================================================================
// The session record (pure)
// =============================================================================

/// What the open file says about itself: its id and when it opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdentity {
    pub id: String,
    pub opened: String,
}

/// Read id and opening time from the first record of the open file.
pub fn session_identity(first_line_json: &str, config: &StoreConfig) -> Result<SessionIdentity, String> {
    let id = stamp_core::session_id_of_first_record(first_line_json, config.session_field)
        .map_err(|e| e.to_string())?;
    let record: Value = serde_json::from_str(first_line_json).map_err(|e| e.to_string())?;
    let opened = record
        .get(config.timestamp_field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("first record carries no string {:?}", config.timestamp_field))?
        .to_string();
    let expected_id = time_core::iso_zulu_to_basic(&opened)?;
    if expected_id != id {
        return Err(format!(
            "session id {id:?} does not match the first record's {:?} {opened:?} (expected {expected_id:?})",
            config.timestamp_field
        ));
    }
    Ok(SessionIdentity { id, opened })
}

/// Build the index record: identity from the file, closing time from the
/// clock, the note from the command line. Optional arrays are omitted when empty.
pub fn session_record(identity: &SessionIdentity, closed: &str, note: &ClosingNote) -> Value {
    let mut record = json!({
        "id": identity.id,
        "opened": identity.opened,
        "closed": closed,
        "about": note.about,
    });
    if !note.tags.is_empty() {
        record["tags"] = json!(note.tags);
    }
    if !note.prior.is_empty() {
        record["prior"] = json!(note.prior);
    }
    record
}

// =============================================================================
// IO steps
// =============================================================================

fn now_iso_zulu() -> Result<String, StoreError> {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| StoreError::IoFailed(format!("system clock: {e}")))?
        .as_secs();
    Ok(time_core::iso_zulu(epoch))
}

fn read_open_file(config: &StoreConfig) -> Result<String, StoreError> {
    if !config.open_file.exists() {
        return Err(StoreError::OpenFileMissing(config.open_file.clone()));
    }
    let content = fs::read_to_string(&config.open_file)
        .map_err(|e| StoreError::IoFailed(format!("read {}: {e}", config.open_file.display())))?;
    if content.trim().is_empty() {
        return Err(StoreError::OpenFileEmpty(config.open_file.clone()));
    }
    Ok(content)
}

fn first_line(content: &str) -> Result<&str, StoreError> {
    content
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| StoreError::FirstRecord("no record found".to_string()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Copy the open file to its stored path and prove the copy identical.
fn copy_verified(config: &StoreConfig, content: &str, stored_path: &Path) -> Result<(), StoreError> {
    if !config.sessions_dir.exists() {
        return Err(StoreError::SessionsDirMissing(config.sessions_dir.clone()));
    }
    if stored_path.exists() {
        return Err(StoreError::AlreadyStored(stored_path.to_path_buf()));
    }
    write_engine::write_file_atomic(stored_path, content).map_err(StoreError::IoFailed)?;
    let copied = fs::read(stored_path)
        .map_err(|e| StoreError::IoFailed(format!("read back {}: {e}", stored_path.display())))?;
    if sha256_hex(&copied) != sha256_hex(content.as_bytes()) {
        let _ = fs::remove_file(stored_path);
        return Err(StoreError::CopyMismatch {
            source: config.open_file.clone(),
            copy: stored_path.to_path_buf(),
        });
    }
    Ok(())
}

fn append_index_record(config: &StoreConfig, record: &Value) -> Result<(), StoreError> {
    let line = serde_json::to_string(record)
        .map_err(|e| StoreError::IoFailed(format!("serialize session record: {e}")))?;
    let result = config
        .index_schema
        .validate(&line)
        .map_err(|e| StoreError::IndexRecordInvalid(e.to_string()))?;
    if !result.valid {
        return Err(StoreError::IndexRecordInvalid(result.message));
    }
    write_engine::append_line_fsync(&config.index_file, &line).map_err(StoreError::IoFailed)
}

// =============================================================================
// --help
// =============================================================================

fn format_help(config: &StoreConfig) -> String {
    format!(
        "{name} — Store the open interview session\n\
         \n\
         HARDCODED CONFIGURATION:\n\
         \x20 Open file:    {open}\n\
         \x20 Sessions dir: {sessions}\n\
         \x20 Index file:   {index}\n\
         \x20 Index schema: {schema} (embedded)\n\
         \x20 Schema path:  {schema_path}\n\
         \x20 Session id:   first record's {session_field:?}; file name {{id}}.jsonl\n\
         \n\
         USAGE:\n\
         \x20 {name} --about \"<what it turned out to be about>\" [--tag <slug>]... [--prior <session-id>]...\n\
         \n\
         STEPS:\n\
         \x20 copy open file → sessions/<id>.jsonl, verify SHA-256, append index record, truncate open file\n\
         \x20 Nothing is removed before the copy is proven identical.\n\
         \n\
         OUTPUT:\n\
         \x20 OK:<id>         Session stored\n\
         \x20 FAIL:<reason>   Nothing stored — see reason\n\
         \n\
         EXIT CODES:\n\
         \x20 0  success\n\
         \x20 1  failure\n\
         \x20 2  usage error",
        name = config.name,
        open = config.open_file.display(),
        sessions = config.sessions_dir.display(),
        index = config.index_file.display(),
        schema = config.index_schema.schema_name(),
        schema_path = config.index_schema_source_path,
        session_field = config.session_field,
    )
}

// =============================================================================
// Run
// =============================================================================

/// Resolve ~/ai as an absolute path from $HOME. Same rule as write_engine's.
pub fn ai_home() -> PathBuf {
    write_engine::ai_home()
}

/// Outcome of a run, so the binary can pick the exit code.
pub enum StoreOutcome {
    Stored(String),
    Help(String),
}

/// Main entry point for store binaries. Parses args, stores the session.
///
/// Returns `Err((exit_code, message))` on failure: 2 for usage, 1 otherwise.
pub fn run(config: &StoreConfig) -> Result<StoreOutcome, (i32, String)> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(StoreOutcome::Help(format_help(config)));
    }
    let note = parse_note(&args).map_err(|e| (2, e.format(config)))?;
    store(config, &note)
        .map(StoreOutcome::Stored)
        .map_err(|e| (1, e.format(config)))
}

/// Store the open file as a session. Returns the session id.
fn store(config: &StoreConfig, note: &ClosingNote) -> Result<String, StoreError> {
    let content = read_open_file(config)?;
    let identity = session_identity(first_line(&content)?, config).map_err(StoreError::FirstRecord)?;
    let stored_path = config.sessions_dir.join(format!("{}.jsonl", identity.id));

    copy_verified(config, &content, &stored_path)?;

    let record = session_record(&identity, &now_iso_zulu()?, note);
    append_index_record(config, &record)?;

    write_engine::write_truncate_fsync(&config.open_file, "").map_err(StoreError::IoFailed)?;
    Ok(format!("OK:{}", identity.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    static INDEX_SCHEMA: &str = r#"{
        "type": "object",
        "required": ["id", "opened", "closed", "about"],
        "properties": {
            "id": {"type": "string", "pattern": "^[0-9]{8}T[0-9]{6}Z$"},
            "opened": {"type": "string"},
            "closed": {"type": "string"},
            "about": {"type": "string", "minLength": 1},
            "tags": {"type": "array", "items": {"type": "string"}},
            "prior": {"type": "array", "items": {"type": "string"}}
        },
        "additionalProperties": false
    }"#;
    static INDEX_VALIDATOR: EmbeddedValidator = EmbeddedValidator::new(INDEX_SCHEMA, "index-test");

    fn config_in(dir: &Path) -> StoreConfig {
        StoreConfig {
            name: "store_test",
            open_file: dir.join("OPEN.jsonl"),
            sessions_dir: dir.join("sessions"),
            index_file: dir.join("index.jsonl"),
            index_schema: &INDEX_VALIDATOR,
            index_schema_source_path: "test".into(),
            session_field: "session",
            timestamp_field: "ts",
        }
    }

    fn fresh_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("store_test_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sessions")).unwrap();
        dir
    }

    const RECORDS: &str = "{\"exchange\":1,\"session\":\"20261110T045515Z\",\"ts\":\"2026-11-10T04:55:15Z\",\"stt\":\"a\"}\n\
                           {\"exchange\":2,\"session\":\"20261110T045515Z\",\"ts\":\"2026-11-10T05:03:27Z\",\"stt\":\"b\"}\n";

    fn note() -> ClosingNote {
        ClosingNote {
            about: "the setup".into(),
            tags: vec!["forseti".into()],
            prior: vec![],
        }
    }

    // --- parse_note ---

    #[test]
    fn parse_note_reads_about_tags_and_prior() {
        let args: Vec<String> = ["--about", "x", "--tag", "a", "--tag", "b", "--prior", "20261110T045515Z"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let parsed = parse_note(&args).unwrap();
        assert_eq!(parsed.about, "x");
        assert_eq!(parsed.tags, vec!["a", "b"]);
        assert_eq!(parsed.prior, vec!["20261110T045515Z"]);
    }

    #[test]
    fn parse_note_requires_about_and_rejects_unknown_flags() {
        assert!(matches!(parse_note(&[]).unwrap_err(), StoreError::Usage(_)));
        let dangling: Vec<String> = vec!["--about".into()];
        assert!(matches!(parse_note(&dangling).unwrap_err(), StoreError::Usage(_)));
        let unknown: Vec<String> = vec!["--about".into(), "x".into(), "--nope".into(), "y".into()];
        assert!(matches!(parse_note(&unknown).unwrap_err(), StoreError::Usage(_)));
    }

    // --- session_identity / session_record ---

    #[test]
    fn session_identity_reads_id_and_opened_and_checks_them_against_each_other() {
        let dir = fresh_dir("identity");
        let config = config_in(&dir);
        let first = RECORDS.lines().next().unwrap();
        let identity = session_identity(first, &config).unwrap();
        assert_eq!(identity.id, "20261110T045515Z");
        assert_eq!(identity.opened, "2026-11-10T04:55:15Z");

        let mismatched = r#"{"session":"20261110T045515Z","ts":"2026-11-10T04:55:16Z"}"#;
        assert!(session_identity(mismatched, &config).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_record_omits_empty_arrays() {
        let identity = SessionIdentity {
            id: "20261110T045515Z".into(),
            opened: "2026-11-10T04:55:15Z".into(),
        };
        let bare = session_record(&identity, "2026-11-10T11:25:09Z", &ClosingNote {
            about: "x".into(),
            tags: vec![],
            prior: vec![],
        });
        assert!(bare.get("tags").is_none());
        assert!(bare.get("prior").is_none());
        assert_eq!(bare["closed"], "2026-11-10T11:25:09Z");

        let tagged = session_record(&identity, "2026-11-10T11:25:09Z", &note());
        assert_eq!(tagged["tags"], json!(["forseti"]));
    }

    // --- store ---

    #[test]
    fn store_copies_verifies_indexes_and_truncates() {
        let dir = fresh_dir("store");
        let config = config_in(&dir);
        fs::write(&config.open_file, RECORDS).unwrap();

        assert_eq!(store(&config, &note()).unwrap(), "OK:20261110T045515Z");

        let stored = dir.join("sessions/20261110T045515Z.jsonl");
        assert_eq!(fs::read_to_string(&stored).unwrap(), RECORDS);
        assert_eq!(fs::read_to_string(&config.open_file).unwrap(), "");
        let index = fs::read_to_string(&config.index_file).unwrap();
        let record: Value = serde_json::from_str(index.trim()).unwrap();
        assert_eq!(record["id"], "20261110T045515Z");
        assert_eq!(record["about"], "the setup");
        assert_eq!(record["tags"], json!(["forseti"]));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_refuses_empty_open_file_and_leaves_everything_alone() {
        let dir = fresh_dir("empty");
        let config = config_in(&dir);
        fs::write(&config.open_file, "").unwrap();
        assert!(matches!(store(&config, &note()).unwrap_err(), StoreError::OpenFileEmpty(_)));
        assert!(!config.index_file.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_refuses_to_overwrite_an_existing_session() {
        let dir = fresh_dir("exists");
        let config = config_in(&dir);
        fs::write(&config.open_file, RECORDS).unwrap();
        fs::write(dir.join("sessions/20261110T045515Z.jsonl"), "older").unwrap();

        assert!(matches!(store(&config, &note()).unwrap_err(), StoreError::AlreadyStored(_)));
        assert_eq!(fs::read_to_string(&config.open_file).unwrap(), RECORDS);
        assert_eq!(fs::read_to_string(dir.join("sessions/20261110T045515Z.jsonl")).unwrap(), "older");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_refuses_a_file_whose_first_record_is_unstamped() {
        let dir = fresh_dir("unstamped");
        let config = config_in(&dir);
        fs::write(&config.open_file, "{\"stt\":\"a\"}\n").unwrap();
        assert!(matches!(store(&config, &note()).unwrap_err(), StoreError::FirstRecord(_)));
        assert!(!dir.join("sessions").read_dir().unwrap().any(|_| true));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_requires_the_sessions_dir() {
        let dir = fresh_dir("nodir");
        let config = config_in(&dir);
        fs::remove_dir_all(&config.sessions_dir).unwrap();
        fs::write(&config.open_file, RECORDS).unwrap();
        assert!(matches!(store(&config, &note()).unwrap_err(), StoreError::SessionsDirMissing(_)));
        assert_eq!(fs::read_to_string(&config.open_file).unwrap(), RECORDS);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_hex_is_the_standard_digest() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
