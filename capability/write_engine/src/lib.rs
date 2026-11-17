//! Write engine for nornir enforcement output tools.
//!
//! Provides the shared infrastructure for all writer binaries:
//! - Format enforcement (jsonl append vs json write)
//! - Path enforcement with traversal protection
//! - Atomic writes with fsync
//! - Educational error messages for LLM consumers
//! - `--help` output showing all hardcoded configuration
//! - `--dump-schema` to print embedded JSON Schema for inspection
//! - Untamperable stamps (sequence, timestamp, session) applied before validation
//! - A receipt on stdout for every write: how many records, and the last record's stamps

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use failure_mode_core::Failure;
use schema_core::EmbeddedValidator;
use schemas_embedded::WRITE_RECEIPT;
pub use stamp_core::Stamps;
use stamp_core::StampContext;

// =============================================================================
// Configuration types
// =============================================================================

/// Output format: line-delimited JSON or complete JSON document.
pub enum OutputFormat {
    /// Append one JSON line per write. File must already exist.
    Jsonl,
    /// Write a complete JSON document. File must NOT already exist.
    Json,
}

/// How many records per invocation.
pub enum WriteFrequency {
    /// Single JSON record from stdin.
    Record,
    /// Multiple JSONL lines from stdin, all-or-nothing validation.
    Batch,
}

/// Output path resolution strategy.
///
/// LLM-provided components are validated against path traversal before use.
pub enum OutputPath {
    /// Full path. No LLM input needed.
    FixedFile(PathBuf),

    /// Directory + fixed suffix. LLM provides a prefix via CLI arg.
    /// Constructed: `{dir}/{llm_prefix}{suffix}`
    DirectoryPrefix {
        dir: PathBuf,
        suffix: &'static str,
    },

    /// Directory + fixed extension. LLM provides filename stem via CLI arg.
    /// Constructed: `{dir}/{llm_name}.{ext}`
    DirectoryName {
        dir: PathBuf,
        ext: &'static str,
    },
}

/// Complete configuration for a writer binary.
pub struct WriterConfig {
    /// Binary name for --help and error messages.
    pub name: &'static str,
    /// Embedded schema validator.
    pub schema: &'static EmbeddedValidator,
    /// Path to the .schema.json source file (for --help display only).
    pub schema_source_path: String,
    /// Output format (jsonl or json).
    pub format: OutputFormat,
    /// Write frequency (record or batch).
    pub frequency: WriteFrequency,
    /// Output path strategy.
    pub output: OutputPath,
    /// Max records per batch (None = unlimited). Only used with Batch frequency.
    pub batch_size: Option<usize>,
    /// Fields the writer sets itself, before validation. A record that supplies
    /// one of them is refused. `Stamps::NONE` writes the record as given.
    pub stamps: Stamps,
}

// =============================================================================
// Error types (writer-specific, not NornirError)
// =============================================================================

/// Every refusal or failure a writer can produce. Its class, message and route come from
/// the error registry (`failure_mode_core`).
#[derive(Debug)]
enum WriteError {
    StdinEmpty,
    InvalidJson(String),
    SchemaValidation(String),
    PathTraversal(String),
    FileExists(String),
    FileNotFound(String),
    DirectoryNotFound(String),
    IoFailed(String),
    BatchTooLarge { got: usize, max: usize },
    MissingArg(String),
    /// One line of a batch was refused; the line number is added to its own failure.
    LineRefused { line: usize, inner: Box<WriteError> },
    /// A step after validation failed — a fault in the writer, not in the record.
    EngineFault(String),
    Stamp(String),
}

impl WriteError {
    /// The registry's failure mode for this error, its slots filled.
    fn failure(&self, config: &WriterConfig) -> Failure {
        let tool = config.name;
        let invocation = invocation(config);
        match self {
            WriteError::StdinEmpty => failure_mode_core::record_absent(&invocation),
            WriteError::InvalidJson(detail) => failure_mode_core::record_not_json(tool, detail, &invocation),
            WriteError::SchemaValidation(report) => failure_mode_core::data_not_matching_schema(report),
            WriteError::PathTraversal(name) => failure_mode_core::record_name_not_plain(name),
            WriteError::FileExists(path) => failure_mode_core::output_exists(path),
            WriteError::FileNotFound(path) => failure_mode_core::output_not_prepared(path),
            WriteError::DirectoryNotFound(path) => failure_mode_core::output_directory_absent(path),
            WriteError::IoFailed(reason) => failure_mode_core::writer_io_failed(reason),
            WriteError::BatchTooLarge { got, max } => failure_mode_core::batch_too_large(got, max),
            WriteError::MissingArg(argument) => failure_mode_core::argument_missing(argument, tool),
            WriteError::LineRefused { line, inner } => {
                let mut failure = inner.failure(config);
                failure.standard = format!("Line {line}: {}", failure.standard);
                failure
            }
            WriteError::EngineFault(reason) => failure_mode_core::writer_engine_fault(reason),
            WriteError::Stamp(reason) => {
                failure_mode_core::stamped_field_supplied(reason, config.stamps.field_names().join(", "))
            }
        }
    }

}

// =============================================================================
// Base path resolution
// =============================================================================

/// The ai home, `~/ai`, as an absolute path: `path_core::ai_home_under` the
/// `$HOME` of this process.
pub fn ai_home() -> PathBuf {
    path_core::ai_home_under(std::env::var("HOME").ok().as_deref())
}

// =============================================================================
// Path traversal protection
// =============================================================================

/// Validate an LLM-provided filename component.
/// Delegates to path_core::validate_path_segment (shared across workspace).
fn validate_filename_component(input: &str) -> Result<(), String> {
    path_core::validate_path_segment(input)
}

// =============================================================================
// Output path resolution
// =============================================================================

/// Resolve the output path from config + optional CLI arg.
fn resolve_output_path(
    config: &WriterConfig,
    cli_arg: Option<&str>,
) -> Result<PathBuf, WriteError> {
    match &config.output {
        OutputPath::FixedFile(path) => Ok(path.clone()),
        OutputPath::DirectoryPrefix { dir, suffix } => {
            let prefix = cli_arg.ok_or_else(|| {
                WriteError::MissingArg("filename prefix as first argument".to_string())
            })?;
            validate_filename_component(prefix)
                .map_err(|_| WriteError::PathTraversal(prefix.to_string()))?;
            Ok(dir.join(format!("{}{}", prefix, suffix)))
        }
        OutputPath::DirectoryName { dir, ext } => {
            let name = cli_arg.ok_or_else(|| {
                WriteError::MissingArg("filename stem as first argument".to_string())
            })?;
            validate_filename_component(name)
                .map_err(|_| WriteError::PathTraversal(name.to_string()))?;
            Ok(dir.join(format!("{}.{}", name, ext)))
        }
    }
}

// =============================================================================
// --help output
// =============================================================================

/// How the writer is called: its name, and `<name>` when its output path takes one.
fn invocation(config: &WriterConfig) -> String {
    let needs_arg = !matches!(config.output, OutputPath::FixedFile(_));
    format!("{}{}", config.name, if needs_arg { " <name>" } else { "" })
}

fn format_help(config: &WriterConfig) -> String {
    let invocation = invocation(config);

    let mut lines = Vec::new();
    lines.push(format!("{} — Validated enforcement output tool\n", config.name));
    format_help_config(config, &mut lines);

    lines.push(String::new());
    lines.push("USAGE:".into());
    lines.push(format!("  echo '<json>' | {invocation}"));
    lines.push(String::new());
    lines.push("  For data with quotes or apostrophes, use heredoc:".into());
    lines.push(format!("  {invocation} <<'RECORD'"));
    lines.push("  {\"uid\":\"...\",\"assessment\":\"...\"}".into());
    lines.push("  RECORD".into());

    lines.push(String::new());
    lines.push("INSPECT:".into());
    lines.push(format!("  {} --dump-schema    Print embedded JSON Schema to stdout", config.name));

    lines.push(String::new());
    format_help_output(config, &mut lines);

    lines.join("\n")
}

/// Format the HARDCODED CONFIGURATION section of --help output.
fn format_help_config(config: &WriterConfig, lines: &mut Vec<String>) {
    let format_str = match config.format {
        OutputFormat::Jsonl => "jsonl (append)",
        OutputFormat::Json => "json (write new file)",
    };
    let freq_str = match config.frequency {
        WriteFrequency::Record => "record (one record per invocation)",
        WriteFrequency::Batch => "batch (multiple JSONL lines per invocation)",
    };
    let output_str = match &config.output {
        OutputPath::FixedFile(path) => format!("{}", path.display()),
        OutputPath::DirectoryPrefix { dir, suffix } => {
            format!("{{dir}}/{{prefix}}{suffix}  (dir={dir})", suffix = suffix, dir = dir.display())
        }
        OutputPath::DirectoryName { dir, ext } => {
            format!("{{dir}}/{{name}}.{ext}  (dir={dir})", ext = ext, dir = dir.display())
        }
    };

    lines.push("HARDCODED CONFIGURATION:".into());
    lines.push(format!("  Schema:      {} (embedded)", config.schema.schema_name()));
    lines.push(format!("  Schema path: {}", config.schema_source_path));
    lines.push(format!("  Format:      {}", format_str));
    lines.push(format!("  Output:      {}", output_str));
    lines.push(format!("  Frequency:   {}", freq_str));
    if let Some(max) = config.batch_size {
        lines.push(format!("  Max batch:   {} records", max));
    }
    if !config.stamps.is_none() {
        lines.push(format!("  Stamps:      {}", format_stamps(&config.stamps)));
    }
}

/// Describe the configured stamps: field name and what it is set from.
fn format_stamps(stamps: &Stamps) -> String {
    let mut parts = Vec::new();
    if let Some(field) = stamps.sequence {
        parts.push(format!("{field} = line count + 1"));
    }
    if let Some(field) = stamps.timestamp {
        parts.push(format!("{field} = clock, ISO 8601 Zulu"));
    }
    if let Some(field) = stamps.session {
        parts.push(format!("{field} = first record's timestamp, basic ISO form"));
    }
    parts.join("; ")
}

/// Format the OUTPUT and EXIT CODES sections of --help output — the CLI contract
/// (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces).
fn format_help_output(config: &WriterConfig, lines: &mut Vec<String>) {
    let written = match config.frequency {
        WriteFrequency::Record => "the record is written",
        WriteFrequency::Batch => "every record of the batch is written",
    };
    let receipt = if config.stamps.is_none() {
        r#"{"written": N}"#.to_string()
    } else {
        format!(
            r#"{{"written": N, "last_record_stamps": {{...}}}} — the last record's {}"#,
            config.stamps.field_names().join(", ")
        )
    };
    lines.push("OUTPUT:".into());
    lines.push(format!("  success   {written}; stdout: the receipt, {receipt}"));
    lines.push("  refusal   stderr: the failure class on the first line, then the message; nothing is written".into());
    lines.push(String::new());
    lines.push("EXIT CODES:".into());
    lines.push("  0  written".into());
    lines.push("  1  refused or failed".into());
    lines.push("  2  wrong arguments".into());
}

// =============================================================================
// Stamping — gather the context from the file and the clock, apply the rules
// =============================================================================

/// Current time as Unix epoch seconds.
fn now_epoch() -> Result<u64, WriteError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|e| WriteError::IoFailed(format!("system clock: {e}")))
}

/// The non-empty lines already in a JSONL target. The file must exist.
fn read_existing_lines(path: &Path) -> Result<Vec<String>, WriteError> {
    if !path.exists() {
        return Err(WriteError::FileNotFound(path.display().to_string()));
    }
    let content = fs::read_to_string(path)
        .map_err(|e| WriteError::IoFailed(format!("read {}: {e}", path.display())))?;
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect())
}

/// What the stamps need: the target's record count, its session id, the clock.
fn stamp_context(config: &WriterConfig, output_path: &Path) -> Result<StampContext, WriteError> {
    let existing = match config.format {
        OutputFormat::Jsonl => read_existing_lines(output_path)?,
        OutputFormat::Json => Vec::new(),
    };
    let session_id = match (config.stamps.session, existing.first()) {
        (Some(field), Some(first_line)) => Some(
            stamp_core::session_id_of_first_record(first_line, field)
                .map_err(|e| WriteError::Stamp(e.to_string()))?,
        ),
        _ => None,
    };
    Ok(StampContext {
        line_count: existing.len() as u64,
        now_epoch: now_epoch()?,
        session_id,
    })
}

/// Stamp one record as the `offset`-th record beyond the context's count.
fn stamp_record(
    config: &WriterConfig,
    context: &StampContext,
    input: &str,
    offset: u64,
) -> Result<String, WriteError> {
    let record: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| WriteError::InvalidJson(e.to_string()))?;
    let context_for_record = StampContext {
        line_count: context.line_count + offset,
        now_epoch: context.now_epoch,
        session_id: context.session_id.clone(),
    };
    let stamped = stamp_core::apply(record, &config.stamps, &context_for_record)
        .map_err(|e| WriteError::Stamp(e.to_string()))?;
    serde_json::to_string(&stamped)
        .map_err(|e| WriteError::IoFailed(format!("re-serialization: {e}")))
}

/// Stamp a batch in order. When the batch opens an empty file, the first record
/// mints the session id and the rest of the batch copies it.
fn stamp_lines(
    config: &WriterConfig,
    context: &StampContext,
    lines: &[&str],
) -> Result<Vec<String>, WriteError> {
    let mut running = StampContext {
        line_count: context.line_count,
        now_epoch: context.now_epoch,
        session_id: context.session_id.clone(),
    };
    let mut stamped_lines = Vec::with_capacity(lines.len());
    for (offset, line) in lines.iter().enumerate() {
        let stamped = stamp_record(config, &running, line, offset as u64)
            .map_err(|e| WriteError::LineRefused { line: offset + 1, inner: Box::new(e) })?;
        if let (Some(field), None) = (config.stamps.session, &running.session_id) {
            running.session_id = Some(
                stamp_core::session_id_of_first_record(&stamped, field)
                    .map_err(|e| WriteError::Stamp(e.to_string()))?,
            );
        }
        stamped_lines.push(stamped);
    }
    Ok(stamped_lines)
}

// =============================================================================
// The receipt — what the writer hands back once the write is on disk
// =============================================================================

/// The receipt for `written` records, the last of which is `last_record`: the count, and
/// the last record's stamped fields under the writer's own field names. Built and checked
/// against `write-receipt` before anything is written, so a write that lands always has
/// its receipt.
fn receipt(config: &WriterConfig, written: usize, last_record: &serde_json::Value) -> Result<String, WriteError> {
    let stamps: serde_json::Map<String, serde_json::Value> = config
        .stamps
        .field_names()
        .into_iter()
        .filter_map(|field| last_record.get(field).map(|value| (field.to_string(), value.clone())))
        .collect();
    let mut body = serde_json::Map::new();
    body.insert("written".to_string(), serde_json::Value::from(written));
    if !stamps.is_empty() {
        body.insert("last_record_stamps".to_string(), serde_json::Value::Object(stamps));
    }
    let json = serde_json::Value::Object(body).to_string();
    let result = WRITE_RECEIPT
        .validate(&json)
        .map_err(|e| WriteError::EngineFault(format!("receipt: {e}")))?;
    if !result.valid {
        return Err(WriteError::EngineFault(format!("receipt: {}", result.message)));
    }
    Ok(json)
}

// =============================================================================
// Atomic write helpers
// =============================================================================

/// Append a line to an existing file with fsync.
fn append_line(path: &Path, line: &str) -> Result<(), WriteError> {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| WriteError::IoFailed(format!("{}: {}", path.display(), e)))?;
    writeln!(file, "{}", line)
        .map_err(|e| WriteError::IoFailed(format!("write to {}: {}", path.display(), e)))?;
    file.sync_all()
        .map_err(|e| WriteError::IoFailed(format!("fsync {}: {}", path.display(), e)))?;
    Ok(())
}

/// Write a complete text file atomically (write temp, fsync, rename).
fn write_atomic(path: &Path, content: &str) -> Result<(), WriteError> {
    write_atomic_bytes(path, content.as_bytes())
}

/// Write a complete file atomically (write temp, fsync, rename).
fn write_atomic_bytes(path: &Path, content: &[u8]) -> Result<(), WriteError> {
    let parent = path
        .parent()
        .ok_or_else(|| WriteError::IoFailed("cannot determine parent directory".to_string()))?;

    let tmp_path = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "output".to_string())
    ));

    // Write to temp file
    let mut file = File::create(&tmp_path)
        .map_err(|e| WriteError::IoFailed(format!("create {}: {}", tmp_path.display(), e)))?;
    file.write_all(content)
        .map_err(|e| WriteError::IoFailed(format!("write {}: {}", tmp_path.display(), e)))?;
    file.sync_all()
        .map_err(|e| WriteError::IoFailed(format!("fsync {}: {}", tmp_path.display(), e)))?;
    drop(file);

    // Atomic rename
    fs::rename(&tmp_path, path).map_err(|e| {
        // Clean up temp file on rename failure
        let _ = fs::remove_file(&tmp_path);
        WriteError::IoFailed(format!(
            "rename {} -> {}: {}",
            tmp_path.display(),
            path.display(),
            e
        ))
    })?;

    Ok(())
}

// =============================================================================
// Public utilities
// =============================================================================

/// Write a file atomically (temp file + fsync + rename).
///
/// For use by non-writer binaries that need atomic writes without the full
/// writer pipeline (schema validation, stdin parsing, path resolution).
pub fn write_file_atomic(path: &Path, content: &str) -> Result<(), String> {
    write_atomic(path, content).map_err(|e| match e {
        WriteError::IoFailed(msg) => msg,
        other => format!("{other:?}"),
    })
}

/// Write a file of bytes atomically (temp file + fsync + rename).
///
/// The byte-level form of `write_file_atomic`, for binary content.
pub fn write_bytes_atomic(path: &Path, content: &[u8]) -> Result<(), String> {
    write_atomic_bytes(path, content).map_err(|e| match e {
        WriteError::IoFailed(msg) => msg,
        other => format!("{other:?}"),
    })
}

/// Truncate a file and write new content, preserving the original inode.
///
/// Unlike `write_file_atomic` (which renames a temp file and changes the inode),
/// this opens the existing file with truncation. Consumers holding open file
/// descriptors (e.g., watchers using BufReader) retain a valid fd and can detect
/// the size change to re-seek.
pub fn write_truncate_fsync(path: &Path, content: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    file.sync_all()
        .map_err(|e| format!("fsync {}: {e}", path.display()))?;
    Ok(())
}

/// Append one line to a file with fsync. Creates the file if it doesn't exist.
///
/// A trailing newline is added automatically.
/// Used for JSONL append across the workspace.
pub fn append_line_fsync(path: &Path, line: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    writeln!(file, "{}", line)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    file.sync_all()
        .map_err(|e| format!("fsync {}: {e}", path.display()))?;
    Ok(())
}

// =============================================================================
// Core run function
// =============================================================================

/// Main entry point for all writer binaries.
///
/// Reads the arguments and stdin, validates, writes, and presents the outcome under the
/// CLI contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces): the receipt on
/// stdout on success, the failure class and message on stderr on a refusal, the usage on
/// wrong arguments. Returns the exit code; a writer's `main()` exits with it.
pub fn run_cli(config: &WriterConfig) -> i32 {
    match run_tool(config) {
        Err(WriteError::MissingArg(argument)) => {
            tool_face_io::cli_usage(&failure_mode_core::argument_missing(argument, config.name).standard_message())
        }
        outcome => tool_face_io::cli_report(outcome.map_err(|error| error.failure(config)), Failure::error_type),
    }
}

/// The writer's one behaviour: the help text for --help, the schema for --dump-schema,
/// and the receipt once the record or batch is written.
fn run_tool(config: &WriterConfig) -> Result<Option<String>, WriteError> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Some(format_help(config)));
    }
    if args.iter().any(|a| a == "--dump-schema") {
        return Ok(Some(config.schema.schema_json().to_string()));
    }

    // CLI arg (filename component) if needed; resolving it validates against traversal.
    let cli_arg = args.get(1).map(|s| s.as_str());
    let output_path = resolve_output_path(config, cli_arg)?;

    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| WriteError::IoFailed(format!("reading stdin: {}", e)))?;
    let input = input.trim();
    if input.is_empty() {
        return Err(WriteError::StdinEmpty);
    }

    let written = match config.frequency {
        WriteFrequency::Record => run_record(config, &output_path, input)?,
        WriteFrequency::Batch => run_batch(config, &output_path, input)?,
    };
    Ok(Some(written))
}

/// Handle single-record writes. Returns the receipt.
fn run_record(config: &WriterConfig, output_path: &Path, input: &str) -> Result<String, WriteError> {
    // Stamp first: the stamps are part of the record the schema sees
    let stamped;
    let input = if config.stamps.is_none() {
        input
    } else {
        let context = stamp_context(config, output_path)?;
        stamped = stamp_record(config, &context, input, 0)?;
        stamped.as_str()
    };

    // Validate JSON + schema
    let result = config
        .schema
        .validate(input)
        .map_err(|e| WriteError::InvalidJson(e.to_string()))?;

    if !result.valid {
        return Err(WriteError::SchemaValidation(result.message));
    }

    // Re-serialize to compact JSON (normalized)
    let data = result.data.ok_or(WriteError::IoFailed(
        "schema validation returned no data".to_string(),
    ))?;
    let compact = serde_json::to_string(&data)
        .map_err(|e| WriteError::IoFailed(format!("re-serialization: {e}")))?;
    let receipt = receipt(config, 1, &data)?;

    // Write based on format
    match config.format {
        OutputFormat::Jsonl => {
            // File must exist
            if !output_path.exists() {
                return Err(WriteError::FileNotFound(
                    output_path.display().to_string(),
                ));
            }
            append_line(output_path, &compact)?;
        }
        OutputFormat::Json => {
            // File must NOT exist
            if output_path.exists() {
                return Err(WriteError::FileExists(
                    output_path.display().to_string(),
                ));
            }
            // Parent directory must exist
            if let Some(parent) = output_path.parent() {
                if !parent.exists() {
                    return Err(WriteError::DirectoryNotFound(
                        parent.display().to_string(),
                    ));
                }
            }
            // Pretty-print for json files
            let pretty = serde_json::to_string_pretty(&data)
                .map_err(|e| WriteError::IoFailed(format!("re-serialization: {e}")))?;
            write_atomic(output_path, &pretty)?;
        }
    }

    Ok(receipt)
}

fn validate_batch_lines(config: &WriterConfig, lines: &[&str]) -> Result<Vec<String>, WriteError> {
    let mut validated: Vec<String> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;
        let refused = |inner: WriteError| WriteError::LineRefused { line: line_num, inner: Box::new(inner) };
        let result = config.schema.validate(line).map_err(|e| refused(WriteError::InvalidJson(e.to_string())))?;

        if !result.valid {
            return Err(refused(WriteError::SchemaValidation(result.message)));
        }

        let data = result
            .data
            .ok_or_else(|| refused(WriteError::EngineFault("no data after validation".to_string())))?;
        let compact = serde_json::to_string(&data)
            .map_err(|e| refused(WriteError::EngineFault(format!("re-serialization: {e}"))))?;
        validated.push(compact);
    }
    Ok(validated)
}

/// Split batch input into non-empty lines and enforce the batch size.
fn batch_lines<'a>(config: &WriterConfig, input: &'a str) -> Result<Vec<&'a str>, WriteError> {
    let lines: Vec<&str> = input.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return Err(WriteError::StdinEmpty);
    }
    if let Some(max) = config.batch_size {
        if lines.len() > max {
            return Err(WriteError::BatchTooLarge {
                got: lines.len(),
                max,
            });
        }
    }
    Ok(lines)
}

/// Append validated lines to an existing JSONL file, one fsync for the batch.
fn append_lines(output_path: &Path, validated: &[String]) -> Result<(), WriteError> {
    if !output_path.exists() {
        return Err(WriteError::FileNotFound(output_path.display().to_string()));
    }
    let mut file = OpenOptions::new()
        .append(true)
        .open(output_path)
        .map_err(|e| WriteError::IoFailed(format!("{}: {}", output_path.display(), e)))?;
    for line in validated {
        writeln!(file, "{}", line).map_err(|e| WriteError::IoFailed(format!("write: {}", e)))?;
    }
    file.sync_all()
        .map_err(|e| WriteError::IoFailed(format!("fsync: {}", e)))
}

/// Write validated lines as a pretty JSON array into a new file.
fn write_json_array(output_path: &Path, validated: &[String]) -> Result<(), WriteError> {
    if output_path.exists() {
        return Err(WriteError::FileExists(output_path.display().to_string()));
    }
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            return Err(WriteError::DirectoryNotFound(parent.display().to_string()));
        }
    }
    let values: Vec<serde_json::Value> = validated
        .iter()
        .map(|s| serde_json::from_str(s))
        .collect::<Result<_, _>>()
        .map_err(|e| WriteError::IoFailed(format!("batch re-parse: {e}")))?;
    let pretty = serde_json::to_string_pretty(&values)
        .map_err(|e| WriteError::IoFailed(format!("batch re-serialization: {e}")))?;
    write_atomic(output_path, &pretty)
}

/// Handle batch writes (multiple JSONL lines, all-or-nothing). Returns the receipt.
fn run_batch(config: &WriterConfig, output_path: &Path, input: &str) -> Result<String, WriteError> {
    let lines = batch_lines(config, input)?;

    let stamped_lines;
    let lines: Vec<&str> = if config.stamps.is_none() {
        lines
    } else {
        let context = stamp_context(config, output_path)?;
        stamped_lines = stamp_lines(config, &context, &lines)?;
        stamped_lines.iter().map(String::as_str).collect()
    };

    let validated = validate_batch_lines(config, &lines)?;
    let last_record: serde_json::Value = validated
        .last()
        .ok_or_else(|| WriteError::EngineFault("no record after validation".to_string()))
        .and_then(|line| {
            serde_json::from_str(line).map_err(|e| WriteError::EngineFault(format!("batch re-parse: {e}")))
        })?;
    let receipt = receipt(config, validated.len(), &last_record)?;

    match config.format {
        OutputFormat::Jsonl => append_lines(output_path, &validated)?,
        OutputFormat::Json => write_json_array(output_path, &validated)?,
    }

    Ok(receipt)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_filename_clean() {
        assert!(validate_filename_component("entry-123").is_ok());
        assert!(validate_filename_component("interview-42").is_ok());
        assert!(validate_filename_component("my_file_name").is_ok());
    }

    #[test]
    fn test_validate_filename_traversal() {
        assert!(validate_filename_component("../etc/passwd").is_err());
        assert!(validate_filename_component("foo/../bar").is_err());
        assert!(validate_filename_component("..").is_err());
    }

    #[test]
    fn test_validate_filename_separators() {
        assert!(validate_filename_component("foo/bar").is_err());
        assert!(validate_filename_component("foo\\bar").is_err());
    }

    #[test]
    fn test_validate_filename_hidden() {
        assert!(validate_filename_component(".hidden").is_err());
        assert!(validate_filename_component(".").is_err());
    }

    #[test]
    fn test_validate_filename_empty() {
        assert!(validate_filename_component("").is_err());
    }

    #[test]
    fn test_validate_filename_null() {
        assert!(validate_filename_component("foo\0bar").is_err());
    }

    #[test]
    fn test_resolve_fixed_path() {
        let config = WriterConfig {
            name: "test",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Jsonl,
            frequency: WriteFrequency::Record,
            output: OutputPath::FixedFile(PathBuf::from("/tmp/test.jsonl")),
            batch_size: None,
            stamps: Stamps::NONE,
        };
        let path = resolve_output_path(&config, None).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test.jsonl"));
    }

    #[test]
    fn test_resolve_directory_name() {
        let config = WriterConfig {
            name: "test",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Json,
            frequency: WriteFrequency::Record,
            output: OutputPath::DirectoryName {
                dir: PathBuf::from("/tmp/out"),
                ext: "json",
            },
            batch_size: None,
            stamps: Stamps::NONE,
        };
        let path = resolve_output_path(&config, Some("entry-123")).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/out/entry-123.json"));
    }

    #[test]
    fn test_resolve_directory_prefix() {
        let config = WriterConfig {
            name: "test",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Jsonl,
            frequency: WriteFrequency::Batch,
            output: OutputPath::DirectoryPrefix {
                dir: PathBuf::from("/tmp/out"),
                suffix: ".summaries.jsonl",
            },
            batch_size: None,
            stamps: Stamps::NONE,
        };
        let path = resolve_output_path(&config, Some("interview-42")).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/tmp/out/interview-42.summaries.jsonl")
        );
    }

    #[test]
    fn test_resolve_directory_name_traversal_blocked() {
        let config = WriterConfig {
            name: "test",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Json,
            frequency: WriteFrequency::Record,
            output: OutputPath::DirectoryName {
                dir: PathBuf::from("/tmp/out"),
                ext: "json",
            },
            batch_size: None,
            stamps: Stamps::NONE,
        };
        assert!(resolve_output_path(&config, Some("../../../etc/passwd")).is_err());
        assert!(resolve_output_path(&config, Some("foo/bar")).is_err());
        assert!(resolve_output_path(&config, Some(".hidden")).is_err());
    }

    #[test]
    fn test_resolve_missing_arg() {
        let config = WriterConfig {
            name: "test",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Json,
            frequency: WriteFrequency::Record,
            output: OutputPath::DirectoryName {
                dir: PathBuf::from("/tmp/out"),
                ext: "json",
            },
            batch_size: None,
            stamps: Stamps::NONE,
        };
        assert!(resolve_output_path(&config, None).is_err());
    }

    // =========================================================================
    // stamps
    // =========================================================================

    static STAMPED_SCHEMA: &str = r#"{
        "type": "object",
        "required": ["exchange", "ts", "session", "stt"],
        "properties": {
            "exchange": {"type": "integer", "minimum": 1},
            "ts": {"type": "string"},
            "session": {"type": "string"},
            "stt": {"type": "string"}
        },
        "additionalProperties": false
    }"#;
    static STAMPED_VALIDATOR: EmbeddedValidator =
        EmbeddedValidator::new(STAMPED_SCHEMA, "stamped-test");

    const EXCHANGE_STAMPS: Stamps = Stamps {
        sequence: Some("exchange"),
        timestamp: Some("ts"),
        session: Some("session"),
    };

    fn stamped_config(path: PathBuf) -> WriterConfig {
        WriterConfig {
            name: "test_stamped",
            schema: &STAMPED_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Jsonl,
            frequency: WriteFrequency::Record,
            output: OutputPath::FixedFile(path),
            batch_size: None,
            stamps: EXCHANGE_STAMPS,
        }
    }

    fn temp_jsonl(tag: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("we_test_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("OPEN.jsonl");
        std::fs::write(&path, "").unwrap();
        (dir, path)
    }

    fn records_in(path: &Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn stamped_records_count_on_and_share_the_minted_session() {
        let (dir, path) = temp_jsonl("stamp_seq");
        let config = stamped_config(path.clone());

        run_record(&config, &path, r#"{"stt": "first"}"#).unwrap();
        run_record(&config, &path, r#"{"stt": "second"}"#).unwrap();

        let records = records_in(&path);
        assert_eq!(records[0]["exchange"], 1);
        assert_eq!(records[1]["exchange"], 2);
        let session = records[0]["session"].as_str().unwrap().to_string();
        assert_eq!(records[1]["session"], session);
        assert_eq!(session.len(), "YYYYMMDDTHHMMSSZ".len());
        assert!(records[0]["ts"].as_str().unwrap().ends_with('Z'));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn supplied_stamp_field_is_refused_and_nothing_is_written() {
        let (dir, path) = temp_jsonl("stamp_refuse");
        let config = stamped_config(path.clone());

        let err = run_record(&config, &path, r#"{"exchange": 9, "stt": "x"}"#).unwrap_err();
        assert!(matches!(err, WriteError::Stamp(_)));
        assert!(err.failure(&config).to_string().contains("exchange, ts, session"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stamping_needs_the_target_file_to_exist() {
        let dir = std::env::temp_dir().join(format!("we_test_stamp_missing_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("OPEN.jsonl");
        let config = stamped_config(path.clone());

        let err = run_record(&config, &path, r#"{"stt": "x"}"#).unwrap_err();
        assert!(matches!(err, WriteError::FileNotFound(_)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stamped_batch_mints_once_and_numbers_in_order() {
        let (dir, path) = temp_jsonl("stamp_batch");
        let mut config = stamped_config(path.clone());
        config.frequency = WriteFrequency::Batch;

        let input = "{\"stt\": \"a\"}\n{\"stt\": \"b\"}\n{\"stt\": \"c\"}";
        run_batch(&config, &path, input).unwrap();

        let records = records_in(&path);
        let numbers: Vec<_> = records.iter().map(|r| r["exchange"].as_u64().unwrap()).collect();
        assert_eq!(numbers, vec![1, 2, 3]);
        assert!(records.iter().all(|r| r["session"] == records[0]["session"]));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // =========================================================================
    // the receipt
    // =========================================================================

    fn receipt_of(json: &str) -> serde_json::Value {
        assert!(WRITE_RECEIPT.is_valid(json).unwrap(), "not a write-receipt: {json}");
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn an_unstamped_record_is_receipted_with_its_count_alone() {
        let (dir, path) = temp_jsonl("receipt_plain");
        let config = WriterConfig {
            name: "test_plain",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Jsonl,
            frequency: WriteFrequency::Record,
            output: OutputPath::FixedFile(path.clone()),
            batch_size: None,
            stamps: Stamps::NONE,
        };

        let receipt = run_record(&config, &path, r#"{"a": 1}"#).unwrap();

        assert_eq!(receipt, r#"{"written":1}"#);
        assert_eq!(records_in(&path).len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stamped_record_is_receipted_with_the_stamps_it_was_given() {
        let (dir, path) = temp_jsonl("receipt_stamped");
        let config = stamped_config(path.clone());

        run_record(&config, &path, r#"{"stt": "first"}"#).unwrap();
        let receipt = receipt_of(&run_record(&config, &path, r#"{"stt": "second"}"#).unwrap());

        let written = &records_in(&path)[1];
        assert_eq!(receipt["written"], 1);
        assert_eq!(receipt["last_record_stamps"]["exchange"], 2);
        assert_eq!(receipt["last_record_stamps"]["ts"], written["ts"]);
        assert_eq!(receipt["last_record_stamps"]["session"], written["session"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_batch_is_receipted_with_its_count_and_the_last_records_stamps() {
        let (dir, path) = temp_jsonl("receipt_batch");
        let mut config = stamped_config(path.clone());
        config.frequency = WriteFrequency::Batch;

        let input = "{\"stt\": \"a\"}\n{\"stt\": \"b\"}\n{\"stt\": \"c\"}";
        let receipt = receipt_of(&run_batch(&config, &path, input).unwrap());

        assert_eq!(receipt["written"], 3);
        assert_eq!(receipt["last_record_stamps"]["exchange"], 3);
        assert_eq!(receipt["last_record_stamps"]["session"], records_in(&path)[2]["session"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_receipt_names_no_path() {
        let (dir, path) = temp_jsonl("receipt_no_path");
        let config = stamped_config(path.clone());

        let receipt = run_record(&config, &path, r#"{"stt": "x"}"#).unwrap();

        assert!(!receipt.contains(dir.to_str().unwrap()));
        assert!(!receipt.contains("OPEN.jsonl"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // =========================================================================
    // append_line_fsync
    // =========================================================================

    #[test]
    fn append_line_fsync_creates_and_appends() {
        let dir = std::env::temp_dir().join(format!("we_test_append_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.jsonl");

        append_line_fsync(&path, r#"{"a":1}"#).unwrap();
        append_line_fsync(&path, r#"{"b":2}"#).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], r#"{"a":1}"#);
        assert_eq!(lines[1], r#"{"b":2}"#);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // =========================================================================
    // write_truncate_fsync
    // =========================================================================

    #[test]
    fn write_truncate_fsync_replaces_content() {
        let dir = std::env::temp_dir().join(format!("we_test_trunc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.jsonl");

        // Write initial content
        append_line_fsync(&path, "line1").unwrap();
        append_line_fsync(&path, "line2").unwrap();
        append_line_fsync(&path, "line3").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 3);

        // Truncate to single line
        write_truncate_fsync(&path, "only_this\n").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "only_this\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_truncate_fsync_preserves_inode() {
        use std::os::unix::fs::MetadataExt;

        let dir = std::env::temp_dir().join(format!("we_test_inode_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.jsonl");

        append_line_fsync(&path, "original").unwrap();
        let ino_before = std::fs::metadata(&path).unwrap().ino();

        write_truncate_fsync(&path, "replaced\n").unwrap();
        let ino_after = std::fs::metadata(&path).unwrap().ino();

        assert_eq!(ino_before, ino_after);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Dummy validator for path resolution tests (schema content doesn't matter)
    static DUMMY_SCHEMA: &str = r#"{"type": "object"}"#;
    static DUMMY_VALIDATOR: EmbeddedValidator =
        EmbeddedValidator::new(DUMMY_SCHEMA, "test-schema");
}
