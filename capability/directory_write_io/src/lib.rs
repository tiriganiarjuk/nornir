//! Directory writing — the one way a generated directory is written: replaced whole.
//!
//! A generator hands every file its directory is to hold. Each text entry is validated
//! against its named schema and rendered in its format through `text_writer_io`; each
//! copy entry is read as bytes. Every entry is checked before anything is written,
//! and every offender is named at once. The new directory is built in a staging folder
//! beside the old one and swapped in by rename; the previous directory is removed only
//! after the swap. A file whose bytes are already in the directory is kept — the same
//! file, its modification time intact — so a time on disk means "last really changed".
//!
//! A generated directory carries its marker, `.generated_directory.json`, holding
//! `{"written_by": ...}`: the writer's proof of ownership. An existing folder without it
//! was not written by this tool and is refused: the tool replaces only what it wrote.
//! Request paths cannot name the marker, a hidden file, `..` or a separator-bearing segment.
//!
//! Flow: validate the request → check every path → render or read every file →
//! list the old directory → build staging (marker, kept files, fresh files) → swap → report.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use directory_read_io::{list_directory, EntryKind, ListOptions};
use path_core::validate_path_segment;
use schemas_embedded::{DIRECTORY_WRITE_REPORT, DIRECTORY_WRITE_REQUEST};
use text_writer_io::{validate_render, TextFormat};

/// The marker that makes a folder a generated directory this tool may replace.
pub const DIRECTORY_MARKER: &str = ".generated_directory.json";
const MARKER_CONTENT: &str = "{\"written_by\": \"write_json_validate_directory\"}\n";

/// One file the directory is to hold.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DirectoryWriteEntry {
    Text { path: String, format: String, schema_name: String, data: Value },
    Copy { path: String, from: String },
}

#[derive(Debug, Deserialize)]
struct DirectoryWriteRequest {
    files: Vec<DirectoryWriteEntry>,
}

/// What a write did: files written fresh, kept in place, and removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DirectoryWriteReport {
    pub written: usize,
    pub unchanged: usize,
    pub removed: usize,
}

/// Every failure a directory write can produce. `error_type()` is the `error.type` the
/// writer returns to Python; the Display string is the `error.message`.
#[derive(Debug, thiserror::Error)]
pub enum DirectoryWriteError {
    #[error("Refused to write directory '{directory}': the request is not valid JSON: {message}")]
    RequestNotJson { directory: String, message: String },

    #[error("Refused to write directory '{directory}': {message}")]
    RequestInvalid { directory: String, message: String },

    /// Every path the request may not name, at once.
    #[error("Refused to write directory '{directory}' — {count} path(s) refused:\n  {}", .problems.join("\n  "), count = .problems.len())]
    PathsRefused { directory: String, problems: Vec<String> },

    #[error("Refused to write directory '{directory}': the folder exists and holds no {DIRECTORY_MARKER}, so it is not a generated directory")]
    NotGenerated { directory: String },

    /// Every file that could not be rendered or read, at once; `error_type` is the first one's class.
    #[error("Refused to write directory '{directory}' — {count} file(s) refused:\n  {}", .failures.join("\n  "), count = .failures.len())]
    FilesRefused { directory: String, first_type: &'static str, failures: Vec<String> },

    #[error("Cannot list the directory '{directory}': {message}")]
    DirectoryUnreadable { directory: String, message: String },

    #[error("Cannot write directory '{directory}': {message}")]
    WriteFailed { directory: String, message: String },

    #[error("Directory '{directory}' report does not match directory-write-report: {message}")]
    ReportInvalid { directory: String, message: String },
}

impl DirectoryWriteError {
    /// The failure class a caller branches on — the `error.type` of the writer result.
    pub fn error_type(&self) -> &'static str {
        match self {
            DirectoryWriteError::RequestNotJson { .. } => "parse_error",
            DirectoryWriteError::RequestInvalid { .. }
            | DirectoryWriteError::PathsRefused { .. }
            | DirectoryWriteError::ReportInvalid { .. } => "validation_error",
            DirectoryWriteError::FilesRefused { first_type, .. } => first_type,
            DirectoryWriteError::DirectoryUnreadable { .. } => "read_error",
            DirectoryWriteError::NotGenerated { .. } | DirectoryWriteError::WriteFailed { .. } => "write_error",
        }
    }
}

/// Replace the directory at `directory_root` with exactly the files `request_json` lists.
pub fn write_directory(
    directory_root: &str,
    request_json: &str,
    schema_dir: &str,
) -> Result<DirectoryWriteReport, DirectoryWriteError> {
    let directory = Path::new(directory_root);
    let request = parsed_request(directory_root, request_json)?;
    check_paths(directory_root, &request)?;
    check_is_generated(directory_root, directory)?;
    let contents = rendered_contents(directory_root, &request, schema_dir)?;
    let previous = previous_files(directory_root, directory)?;
    let staging = sibling(directory, "staging");
    let report = build_staging(directory_root, directory, &staging, &contents, &previous)?;
    swap_in(directory_root, directory, &staging)?;
    Ok(report)
}

/// Replace the directory and return its report as JSON validated against `directory-write-report`.
pub fn write_directory_json(
    directory_root: &str,
    request_json: &str,
    schema_dir: &str,
) -> Result<String, DirectoryWriteError> {
    let report = write_directory(directory_root, request_json, schema_dir)?;
    let invalid =
        |message: String| DirectoryWriteError::ReportInvalid { directory: directory_root.to_string(), message };
    let json = serde_json::to_string(&report).map_err(|json_err| invalid(json_err.to_string()))?;
    let result = DIRECTORY_WRITE_REPORT.validate(&json).map_err(|schema_err| invalid(schema_err.to_string()))?;
    if !result.valid {
        return Err(invalid(result.message));
    }
    Ok(json)
}

fn parsed_request(directory_root: &str, request_json: &str) -> Result<DirectoryWriteRequest, DirectoryWriteError> {
    let invalid =
        |message: String| DirectoryWriteError::RequestInvalid { directory: directory_root.to_string(), message };
    let result =
        DIRECTORY_WRITE_REQUEST.validate(request_json).map_err(|schema_err| DirectoryWriteError::RequestNotJson {
            directory: directory_root.to_string(),
            message: schema_err.to_string(),
        })?;
    if !result.valid {
        return Err(invalid(result.message));
    }
    serde_json::from_str(request_json).map_err(|json_err| invalid(json_err.to_string()))
}

fn entry_path(entry: &DirectoryWriteEntry) -> &str {
    match entry {
        DirectoryWriteEntry::Text { path, .. } | DirectoryWriteEntry::Copy { path, .. } => path,
    }
}

/// Every segment of every path is a plain name, and no path is named twice.
fn check_paths(directory_root: &str, request: &DirectoryWriteRequest) -> Result<(), DirectoryWriteError> {
    let mut seen = BTreeSet::new();
    let mut problems = Vec::new();
    for path in request.files.iter().map(entry_path) {
        let segment_problems = path.split('/').filter_map(|segment| validate_path_segment(segment).err());
        problems.extend(segment_problems.map(|problem| format!("{path}: {problem}")));
        if !seen.insert(path) {
            problems.push(format!("{path}: named more than once"));
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(DirectoryWriteError::PathsRefused { directory: directory_root.to_string(), problems })
}

/// A generated directory is a named folder; an existing one must carry the marker; an absent one is new.
fn check_is_generated(directory_root: &str, directory: &Path) -> Result<(), DirectoryWriteError> {
    if directory.file_name().is_none() {
        return Err(DirectoryWriteError::RequestInvalid {
            directory: directory_root.to_string(),
            message: "the directory root names no folder".to_string(),
        });
    }
    if directory.exists() && !directory.join(DIRECTORY_MARKER).is_file() {
        return Err(DirectoryWriteError::NotGenerated { directory: directory_root.to_string() });
    }
    Ok(())
}

/// The bytes every entry is to hold, by path; every entry that fails is named.
fn rendered_contents(
    directory_root: &str,
    request: &DirectoryWriteRequest,
    schema_dir: &str,
) -> Result<BTreeMap<String, Vec<u8>>, DirectoryWriteError> {
    let mut contents = BTreeMap::new();
    let mut failures: Vec<(&'static str, String)> = Vec::new();
    for entry in &request.files {
        match entry_bytes(directory_root, entry, schema_dir) {
            Ok(bytes) => {
                contents.insert(entry_path(entry).to_string(), bytes);
            }
            Err(failure) => failures.push(failure),
        }
    }
    let Some(&(first_type, _)) = failures.first() else {
        return Ok(contents);
    };
    let failures = failures.into_iter().map(|(error_type, message)| format!("[{error_type}] {message}")).collect();
    Err(DirectoryWriteError::FilesRefused { directory: directory_root.to_string(), first_type, failures })
}

/// One entry's bytes: its text rendered, or its source read; a failure carries its class.
fn entry_bytes(
    directory_root: &str,
    entry: &DirectoryWriteEntry,
    schema_dir: &str,
) -> Result<Vec<u8>, (&'static str, String)> {
    match entry {
        DirectoryWriteEntry::Text { path, format, schema_name, data } => {
            let target = Path::new(directory_root).join(path);
            let target_name = target.to_string_lossy();
            let Some(text_format) = TextFormat::from_name(format) else {
                return Err(("validation_error", format!("{path}: unknown format '{format}'")));
            };
            validate_render(text_format, &data.to_string(), &target_name, schema_name, schema_dir)
                .map(String::into_bytes)
                .map_err(|text_err| (text_err.error_type(), text_err.to_string()))
        }
        DirectoryWriteEntry::Copy { path, from } => {
            fs::read(from).map_err(|io_err| ("read_error", format!("{path}: cannot read '{from}': {io_err}")))
        }
    }
}

/// The files the previous directory holds, by path, marker excluded; an absent directory holds none.
fn previous_files(directory_root: &str, directory: &Path) -> Result<BTreeSet<String>, DirectoryWriteError> {
    if !directory.exists() {
        return Ok(BTreeSet::new());
    }
    let listing = list_directory(directory_root, ListOptions::default()).map_err(|list_err| {
        DirectoryWriteError::DirectoryUnreadable { directory: directory_root.to_string(), message: list_err.to_string() }
    })?;
    Ok(listing
        .entries
        .into_iter()
        .filter(|entry| entry.kind != EntryKind::Directory)
        .map(|entry| entry.path)
        .collect())
}

/// A folder beside the directory, named for it: `.{name}.{role}`.
fn sibling(directory: &Path, role: &str) -> PathBuf {
    let name = directory.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    directory.with_file_name(format!(".{name}.{role}"))
}

/// Remove a leftover sibling folder — only one that carries the marker, so only one this tool made.
fn clear_leftover(directory_root: &str, folder: &Path) -> Result<(), DirectoryWriteError> {
    if !folder.exists() {
        return Ok(());
    }
    if !folder.join(DIRECTORY_MARKER).is_file() {
        return Err(DirectoryWriteError::WriteFailed {
            directory: directory_root.to_string(),
            message: format!("'{}' exists and holds no {DIRECTORY_MARKER}; it was not made by this tool", folder.display()),
        });
    }
    fs::remove_dir_all(folder).map_err(|io_err| write_failed(directory_root, folder, &io_err))
}

fn write_failed(directory_root: &str, path: &Path, io_err: &std::io::Error) -> DirectoryWriteError {
    DirectoryWriteError::WriteFailed {
        directory: directory_root.to_string(),
        message: format!("'{}': {io_err}", path.display()),
    }
}

/// Build the new directory in staging: the marker, each byte-identical file kept as itself, every other written fresh.
fn build_staging(
    directory_root: &str,
    directory: &Path,
    staging: &Path,
    contents: &BTreeMap<String, Vec<u8>>,
    previous: &BTreeSet<String>,
) -> Result<DirectoryWriteReport, DirectoryWriteError> {
    clear_leftover(directory_root, staging)?;
    fs::create_dir_all(staging).map_err(|io_err| write_failed(directory_root, staging, &io_err))?;
    fs::write(staging.join(DIRECTORY_MARKER), MARKER_CONTENT)
        .map_err(|io_err| write_failed(directory_root, staging, &io_err))?;
    let mut report = DirectoryWriteReport { written: 0, unchanged: 0, removed: 0 };
    for (path, bytes) in contents {
        let kept =
            stage_file(directory_root, &directory.join(path), &staging.join(path), bytes, previous.contains(path))?;
        if kept {
            report.unchanged += 1;
        } else {
            report.written += 1;
        }
    }
    report.removed = previous.iter().filter(|path| !contents.contains_key(*path)).count();
    Ok(report)
}

/// One file into staging: the existing file, linked as itself when its bytes match; else the new bytes. True when kept.
fn stage_file(
    directory_root: &str,
    current: &Path,
    staged: &Path,
    bytes: &[u8],
    existed: bool,
) -> Result<bool, DirectoryWriteError> {
    if let Some(parent) = staged.parent() {
        fs::create_dir_all(parent).map_err(|io_err| write_failed(directory_root, parent, &io_err))?;
    }
    if existed && fs::read(current).is_ok_and(|existing| existing == bytes) && fs::hard_link(current, staged).is_ok() {
        return Ok(true);
    }
    fs::write(staged, bytes).map_err(|io_err| write_failed(directory_root, staged, &io_err))?;
    Ok(false)
}

/// Swap staging in for the directory by rename; the previous directory is removed only after the swap holds.
fn swap_in(directory_root: &str, directory: &Path, staging: &Path) -> Result<(), DirectoryWriteError> {
    let previous = sibling(directory, "previous");
    clear_leftover(directory_root, &previous)?;
    let had_directory = directory.exists();
    if had_directory {
        fs::rename(directory, &previous).map_err(|io_err| write_failed(directory_root, directory, &io_err))?;
    }
    if let Err(io_err) = fs::rename(staging, directory) {
        let restore = if had_directory { fs::rename(&previous, directory).err() } else { None };
        let message = match restore {
            None => format!("'{}': {io_err}", staging.display()),
            Some(restore_err) => format!(
                "'{}': {io_err}; restoring the previous directory from '{}' also failed: {restore_err}",
                staging.display(),
                previous.display()
            ),
        };
        return Err(DirectoryWriteError::WriteFailed { directory: directory_root.to_string(), message });
    }
    if had_directory {
        fs::remove_dir_all(&previous).map_err(|io_err| write_failed(directory_root, &previous, &io_err))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    const PAGE_SCHEMA: &str = r#"{"type": "object", "required": ["frontmatter", "body"]}"#;

    fn workspace(test_name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("nornir_directory_write_io_{test_name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("schemas")).unwrap();
        fs::write(dir.join("schemas/page.schema.json"), PAGE_SCHEMA).unwrap();
        (dir.join("generated"), dir)
    }

    fn page(path: &str, body: &str) -> String {
        format!(r#"{{"path": "{path}", "format": "markdown", "schema_name": "page", "data": {{"frontmatter": {{"title": "T"}}, "body": "{body}"}}}}"#)
    }

    fn request(entries: &[String]) -> String {
        format!(r#"{{"files": [{}]}}"#, entries.join(", "))
    }

    fn schema_dir(dir: &Path) -> String {
        dir.join("schemas").to_string_lossy().to_string()
    }

    #[test]
    fn a_new_directory_is_written_with_its_marker() {
        let (generated, dir) = workspace("new");
        let report =
            write_directory(generated.to_str().unwrap(), &request(&[page("a/one.md", "1")]), &schema_dir(&dir)).unwrap();
        assert_eq!(report, DirectoryWriteReport { written: 1, unchanged: 0, removed: 0 });
        assert_eq!(fs::read_to_string(generated.join("a/one.md")).unwrap(), "---\ntitle: T\n---\n1\n");
        assert!(generated.join(DIRECTORY_MARKER).is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_rewrite_keeps_identical_files_with_their_time_and_removes_what_is_gone() {
        let (generated, dir) = workspace("rewrite");
        let schemas = schema_dir(&dir);
        write_directory(
            generated.to_str().unwrap(),
            &request(&[page("keep.md", "k"), page("change.md", "1"), page("gone.md", "g")]),
            &schemas,
        )
        .unwrap();
        let old_time = SystemTime::now() - Duration::from_secs(3600);
        fs::File::options().write(true).open(generated.join("keep.md")).unwrap().set_modified(old_time).unwrap();
        let report =
            write_directory(generated.to_str().unwrap(), &request(&[page("keep.md", "k"), page("change.md", "2")]), &schemas)
                .unwrap();
        assert_eq!(report, DirectoryWriteReport { written: 1, unchanged: 1, removed: 1 });
        assert_eq!(fs::metadata(generated.join("keep.md")).unwrap().modified().unwrap(), old_time);
        assert!(!generated.join("gone.md").exists());
        assert_eq!(fs::read_to_string(generated.join("change.md")).unwrap(), "---\ntitle: T\n---\n2\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_folder_without_the_marker_is_refused_and_untouched() {
        let (generated, dir) = workspace("not_generated");
        fs::create_dir_all(&generated).unwrap();
        fs::write(generated.join("authored.md"), "mine").unwrap();
        let error =
            write_directory(generated.to_str().unwrap(), &request(&[page("a.md", "x")]), &schema_dir(&dir)).unwrap_err();
        assert_eq!(error.error_type(), "write_error");
        assert_eq!(fs::read_to_string(generated.join("authored.md")).unwrap(), "mine");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_bad_path_is_named_and_nothing_is_written() {
        let (generated, dir) = workspace("paths");
        let entries =
            [page("../escape.md", "x"), page(DIRECTORY_MARKER, "x"), page("dup.md", "x"), page("dup.md", "y")];
        let error = write_directory(generated.to_str().unwrap(), &request(&entries), &schema_dir(&dir)).unwrap_err();
        assert_eq!(error.error_type(), "validation_error");
        let message = error.to_string();
        assert!(
            message.contains("../escape.md") && message.contains(DIRECTORY_MARKER) && message.contains("dup.md"),
            "{message}"
        );
        assert!(!generated.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_refused_file_is_named_and_the_old_directory_stays() {
        let (generated, dir) = workspace("refused");
        let schemas = schema_dir(&dir);
        write_directory(generated.to_str().unwrap(), &request(&[page("kept.md", "k")]), &schemas).unwrap();
        let bad = [
            r#"{"path": "a.md", "format": "markdown", "schema_name": "page", "data": {"body": "no frontmatter"}}"#.to_string(),
            r#"{"path": "b.jpg", "from": "/absolutely/absent.jpg"}"#.to_string(),
        ];
        let error = write_directory(generated.to_str().unwrap(), &request(&bad), &schemas).unwrap_err();
        assert_eq!(error.error_type(), "validation_error");
        assert!(error.to_string().contains("a.md") && error.to_string().contains("absent.jpg"), "{error}");
        assert!(generated.join("kept.md").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_copy_entry_holds_the_source_bytes() {
        let (generated, dir) = workspace("copy");
        let source = dir.join("photo.bin");
        fs::write(&source, [0u8, 1, 2, 255]).unwrap();
        let entry = format!(r#"{{"path": "img/photo.bin", "from": "{}"}}"#, source.display());
        write_directory(generated.to_str().unwrap(), &request(&[entry]), &schema_dir(&dir)).unwrap();
        assert_eq!(fs::read(generated.join("img/photo.bin")).unwrap(), vec![0u8, 1, 2, 255]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_report_json_matches_its_schema() {
        let (generated, dir) = workspace("report");
        let json =
            write_directory_json(generated.to_str().unwrap(), &request(&[page("a.md", "x")]), &schema_dir(&dir)).unwrap();
        assert!(DIRECTORY_WRITE_REPORT.is_valid(&json).unwrap());
        fs::remove_dir_all(&dir).ok();
    }
}
