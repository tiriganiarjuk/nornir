//! Zone writing — the one way a generated zone is written: replaced whole.
//!
//! A generator hands every file its zone is to hold. Each text entry is validated
//! against its named schema and rendered in its format through `text_writer_io`; each
//! copy entry is read as bytes. Every entry is checked before anything is written,
//! and every offender is named at once. The new zone is built in a staging folder
//! beside the old one and swapped in by rename; the previous zone is removed only
//! after the swap. A file whose bytes are already in the zone is kept — the same file,
//! its modification time intact — so a time on disk means "last really changed".
//!
//! A zone carries its marker, `.zone.json`. An existing folder without it is not a
//! zone and is refused: the tool replaces only what it wrote. Request paths cannot
//! name the marker, a hidden file, `..` or a separator-bearing segment.
//!
//! Flow: validate the request → check every path → render or read every file →
//! list the old zone → build staging (marker, kept files, fresh files) → swap → report.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use directory_read_io::{list_directory, EntryKind, ListOptions};
use path_core::validate_path_segment;
use schemas_embedded::{ZONE_REPORT, ZONE_REQUEST};
use text_writer_io::{validate_render, TextFormat};

/// The marker that makes a folder a zone this tool may replace.
pub const ZONE_MARKER: &str = ".zone.json";
const MARKER_CONTENT: &str = "{\"written_by\": \"write_json_validate_zone\"}\n";

/// One file the zone is to hold.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ZoneEntry {
    Text { path: String, format: String, schema_name: String, data: Value },
    Copy { path: String, from: String },
}

#[derive(Debug, Deserialize)]
struct ZoneRequest {
    files: Vec<ZoneEntry>,
}

/// What a write did: files written fresh, kept in place, and removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ZoneReport {
    pub written: usize,
    pub unchanged: usize,
    pub removed: usize,
}

/// Every failure a zone write can produce. `error_type()` is the `error.type` the
/// writer returns to Python; the Display string is the `error.message`.
#[derive(Debug, thiserror::Error)]
pub enum ZoneWriteError {
    #[error("Refused to write zone '{zone}': the request is not valid JSON: {message}")]
    RequestNotJson { zone: String, message: String },

    #[error("Refused to write zone '{zone}': {message}")]
    RequestInvalid { zone: String, message: String },

    /// Every path the request may not name, at once.
    #[error("Refused to write zone '{zone}' — {count} path(s) refused:\n  {}", .problems.join("\n  "), count = .problems.len())]
    PathsRefused { zone: String, problems: Vec<String> },

    #[error("Refused to write zone '{zone}': the folder exists and holds no {ZONE_MARKER}, so it is not a zone")]
    NotAZone { zone: String },

    /// Every file that could not be rendered or read, at once; `error_type` is the first one's class.
    #[error("Refused to write zone '{zone}' — {count} file(s) refused:\n  {}", .failures.join("\n  "), count = .failures.len())]
    FilesRefused { zone: String, first_type: &'static str, failures: Vec<String> },

    #[error("Cannot list the zone '{zone}': {message}")]
    ZoneUnreadable { zone: String, message: String },

    #[error("Cannot write zone '{zone}': {message}")]
    WriteFailed { zone: String, message: String },

    #[error("Zone '{zone}' report does not match zone-report: {message}")]
    ReportInvalid { zone: String, message: String },
}

impl ZoneWriteError {
    /// The failure class a caller branches on — the `error.type` of the writer result.
    pub fn error_type(&self) -> &'static str {
        match self {
            ZoneWriteError::RequestNotJson { .. } => "parse_error",
            ZoneWriteError::RequestInvalid { .. }
            | ZoneWriteError::PathsRefused { .. }
            | ZoneWriteError::ReportInvalid { .. } => "validation_error",
            ZoneWriteError::FilesRefused { first_type, .. } => first_type,
            ZoneWriteError::ZoneUnreadable { .. } => "read_error",
            ZoneWriteError::NotAZone { .. } | ZoneWriteError::WriteFailed { .. } => "write_error",
        }
    }
}

/// Replace the zone at `zone_root` with exactly the files `request_json` lists.
pub fn write_zone(zone_root: &str, request_json: &str, schema_dir: &str) -> Result<ZoneReport, ZoneWriteError> {
    let zone = Path::new(zone_root);
    let request = parsed_request(zone_root, request_json)?;
    check_paths(zone_root, &request)?;
    check_is_zone(zone_root, zone)?;
    let contents = rendered_contents(zone_root, &request, schema_dir)?;
    let previous = previous_files(zone_root, zone)?;
    let staging = sibling(zone, "zone-staging");
    let report = build_staging(zone_root, zone, &staging, &contents, &previous)?;
    swap_in(zone_root, zone, &staging)?;
    Ok(report)
}

/// Replace the zone and return its report as JSON validated against `zone-report`.
pub fn write_zone_json(zone_root: &str, request_json: &str, schema_dir: &str) -> Result<String, ZoneWriteError> {
    let report = write_zone(zone_root, request_json, schema_dir)?;
    let invalid = |message: String| ZoneWriteError::ReportInvalid { zone: zone_root.to_string(), message };
    let json = serde_json::to_string(&report).map_err(|json_err| invalid(json_err.to_string()))?;
    let result = ZONE_REPORT.validate(&json).map_err(|schema_err| invalid(schema_err.to_string()))?;
    if !result.valid {
        return Err(invalid(result.message));
    }
    Ok(json)
}

fn parsed_request(zone_root: &str, request_json: &str) -> Result<ZoneRequest, ZoneWriteError> {
    let invalid = |message: String| ZoneWriteError::RequestInvalid { zone: zone_root.to_string(), message };
    let result = ZONE_REQUEST.validate(request_json).map_err(|schema_err| ZoneWriteError::RequestNotJson {
        zone: zone_root.to_string(),
        message: schema_err.to_string(),
    })?;
    if !result.valid {
        return Err(invalid(result.message));
    }
    serde_json::from_str(request_json).map_err(|json_err| invalid(json_err.to_string()))
}

fn entry_path(entry: &ZoneEntry) -> &str {
    match entry {
        ZoneEntry::Text { path, .. } | ZoneEntry::Copy { path, .. } => path,
    }
}

/// Every segment of every path is a plain name, and no path is named twice.
fn check_paths(zone_root: &str, request: &ZoneRequest) -> Result<(), ZoneWriteError> {
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
    Err(ZoneWriteError::PathsRefused { zone: zone_root.to_string(), problems })
}

/// A zone is a named folder; an existing one must carry the marker; an absent one is a new zone.
fn check_is_zone(zone_root: &str, zone: &Path) -> Result<(), ZoneWriteError> {
    if zone.file_name().is_none() {
        return Err(ZoneWriteError::RequestInvalid {
            zone: zone_root.to_string(),
            message: "the zone root names no folder".to_string(),
        });
    }
    if zone.exists() && !zone.join(ZONE_MARKER).is_file() {
        return Err(ZoneWriteError::NotAZone { zone: zone_root.to_string() });
    }
    Ok(())
}

/// The bytes every entry is to hold, by path; every entry that fails is named.
fn rendered_contents(
    zone_root: &str,
    request: &ZoneRequest,
    schema_dir: &str,
) -> Result<BTreeMap<String, Vec<u8>>, ZoneWriteError> {
    let mut contents = BTreeMap::new();
    let mut failures: Vec<(&'static str, String)> = Vec::new();
    for entry in &request.files {
        match entry_bytes(zone_root, entry, schema_dir) {
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
    Err(ZoneWriteError::FilesRefused { zone: zone_root.to_string(), first_type, failures })
}

/// One entry's bytes: its text rendered, or its source read; a failure carries its class.
fn entry_bytes(zone_root: &str, entry: &ZoneEntry, schema_dir: &str) -> Result<Vec<u8>, (&'static str, String)> {
    match entry {
        ZoneEntry::Text { path, format, schema_name, data } => {
            let target = Path::new(zone_root).join(path);
            let target_name = target.to_string_lossy();
            let Some(text_format) = TextFormat::from_name(format) else {
                return Err(("validation_error", format!("{path}: unknown format '{format}'")));
            };
            validate_render(text_format, &data.to_string(), &target_name, schema_name, schema_dir)
                .map(String::into_bytes)
                .map_err(|text_err| (text_err.error_type(), text_err.to_string()))
        }
        ZoneEntry::Copy { path, from } => {
            fs::read(from).map_err(|io_err| ("read_error", format!("{path}: cannot read '{from}': {io_err}")))
        }
    }
}

/// The files the previous zone holds, by path, marker excluded; an absent zone holds none.
fn previous_files(zone_root: &str, zone: &Path) -> Result<BTreeSet<String>, ZoneWriteError> {
    if !zone.exists() {
        return Ok(BTreeSet::new());
    }
    let listing = list_directory(zone_root, ListOptions::default())
        .map_err(|list_err| ZoneWriteError::ZoneUnreadable { zone: zone_root.to_string(), message: list_err.to_string() })?;
    Ok(listing
        .entries
        .into_iter()
        .filter(|entry| entry.kind != EntryKind::Directory)
        .map(|entry| entry.path)
        .collect())
}

/// A folder beside the zone, named for it: `.{name}.{role}`.
fn sibling(zone: &Path, role: &str) -> PathBuf {
    let name = zone.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    zone.with_file_name(format!(".{name}.{role}"))
}

/// Remove a leftover sibling folder — only one that carries the marker, so only one this tool made.
fn clear_leftover(zone_root: &str, folder: &Path) -> Result<(), ZoneWriteError> {
    if !folder.exists() {
        return Ok(());
    }
    if !folder.join(ZONE_MARKER).is_file() {
        return Err(ZoneWriteError::WriteFailed {
            zone: zone_root.to_string(),
            message: format!("'{}' exists and holds no {ZONE_MARKER}; it was not made by this tool", folder.display()),
        });
    }
    fs::remove_dir_all(folder).map_err(|io_err| write_failed(zone_root, folder, &io_err))
}

fn write_failed(zone_root: &str, path: &Path, io_err: &std::io::Error) -> ZoneWriteError {
    ZoneWriteError::WriteFailed { zone: zone_root.to_string(), message: format!("'{}': {io_err}", path.display()) }
}

/// Build the new zone in staging: the marker, each byte-identical file kept as itself, every other written fresh.
fn build_staging(
    zone_root: &str,
    zone: &Path,
    staging: &Path,
    contents: &BTreeMap<String, Vec<u8>>,
    previous: &BTreeSet<String>,
) -> Result<ZoneReport, ZoneWriteError> {
    clear_leftover(zone_root, staging)?;
    fs::create_dir_all(staging).map_err(|io_err| write_failed(zone_root, staging, &io_err))?;
    fs::write(staging.join(ZONE_MARKER), MARKER_CONTENT).map_err(|io_err| write_failed(zone_root, staging, &io_err))?;
    let mut report = ZoneReport { written: 0, unchanged: 0, removed: 0 };
    for (path, bytes) in contents {
        let kept = stage_file(zone_root, &zone.join(path), &staging.join(path), bytes, previous.contains(path))?;
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
fn stage_file(zone_root: &str, current: &Path, staged: &Path, bytes: &[u8], existed: bool) -> Result<bool, ZoneWriteError> {
    if let Some(parent) = staged.parent() {
        fs::create_dir_all(parent).map_err(|io_err| write_failed(zone_root, parent, &io_err))?;
    }
    if existed && fs::read(current).is_ok_and(|existing| existing == bytes) && fs::hard_link(current, staged).is_ok() {
        return Ok(true);
    }
    fs::write(staged, bytes).map_err(|io_err| write_failed(zone_root, staged, &io_err))?;
    Ok(false)
}

/// Swap staging in for the zone by rename; the previous zone is removed only after the swap holds.
fn swap_in(zone_root: &str, zone: &Path, staging: &Path) -> Result<(), ZoneWriteError> {
    let previous = sibling(zone, "zone-previous");
    clear_leftover(zone_root, &previous)?;
    let had_zone = zone.exists();
    if had_zone {
        fs::rename(zone, &previous).map_err(|io_err| write_failed(zone_root, zone, &io_err))?;
    }
    if let Err(io_err) = fs::rename(staging, zone) {
        let restore = if had_zone { fs::rename(&previous, zone).err() } else { None };
        let message = match restore {
            None => format!("'{}': {io_err}", staging.display()),
            Some(restore_err) => format!(
                "'{}': {io_err}; restoring the previous zone from '{}' also failed: {restore_err}",
                staging.display(),
                previous.display()
            ),
        };
        return Err(ZoneWriteError::WriteFailed { zone: zone_root.to_string(), message });
    }
    if had_zone {
        fs::remove_dir_all(&previous).map_err(|io_err| write_failed(zone_root, &previous, &io_err))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    const PAGE_SCHEMA: &str = r#"{"type": "object", "required": ["frontmatter", "body"]}"#;

    fn workspace(test_name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("nornir_zone_write_io_{test_name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("schemas")).unwrap();
        fs::write(dir.join("schemas/page.schema.json"), PAGE_SCHEMA).unwrap();
        (dir.join("zone"), dir)
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
    fn a_new_zone_is_written_with_its_marker() {
        let (zone, dir) = workspace("new");
        let report = write_zone(zone.to_str().unwrap(), &request(&[page("a/one.md", "1")]), &schema_dir(&dir)).unwrap();
        assert_eq!(report, ZoneReport { written: 1, unchanged: 0, removed: 0 });
        assert_eq!(fs::read_to_string(zone.join("a/one.md")).unwrap(), "---\ntitle: T\n---\n1\n");
        assert!(zone.join(ZONE_MARKER).is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_rewrite_keeps_identical_files_with_their_time_and_removes_what_is_gone() {
        let (zone, dir) = workspace("rewrite");
        let schemas = schema_dir(&dir);
        write_zone(zone.to_str().unwrap(), &request(&[page("keep.md", "k"), page("change.md", "1"), page("gone.md", "g")]), &schemas).unwrap();
        let old_time = SystemTime::now() - Duration::from_secs(3600);
        fs::File::options().write(true).open(zone.join("keep.md")).unwrap().set_modified(old_time).unwrap();
        let report = write_zone(zone.to_str().unwrap(), &request(&[page("keep.md", "k"), page("change.md", "2")]), &schemas).unwrap();
        assert_eq!(report, ZoneReport { written: 1, unchanged: 1, removed: 1 });
        assert_eq!(fs::metadata(zone.join("keep.md")).unwrap().modified().unwrap(), old_time);
        assert!(!zone.join("gone.md").exists());
        assert_eq!(fs::read_to_string(zone.join("change.md")).unwrap(), "---\ntitle: T\n---\n2\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_folder_without_the_marker_is_refused_and_untouched() {
        let (zone, dir) = workspace("not_a_zone");
        fs::create_dir_all(&zone).unwrap();
        fs::write(zone.join("authored.md"), "mine").unwrap();
        let error = write_zone(zone.to_str().unwrap(), &request(&[page("a.md", "x")]), &schema_dir(&dir)).unwrap_err();
        assert_eq!(error.error_type(), "write_error");
        assert_eq!(fs::read_to_string(zone.join("authored.md")).unwrap(), "mine");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_bad_path_is_named_and_nothing_is_written() {
        let (zone, dir) = workspace("paths");
        let entries = [page("../escape.md", "x"), page(".zone.json", "x"), page("dup.md", "x"), page("dup.md", "y")];
        let error = write_zone(zone.to_str().unwrap(), &request(&entries), &schema_dir(&dir)).unwrap_err();
        assert_eq!(error.error_type(), "validation_error");
        let message = error.to_string();
        assert!(message.contains("../escape.md") && message.contains(".zone.json") && message.contains("dup.md"), "{message}");
        assert!(!zone.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_refused_file_is_named_and_the_old_zone_stays() {
        let (zone, dir) = workspace("refused");
        let schemas = schema_dir(&dir);
        write_zone(zone.to_str().unwrap(), &request(&[page("kept.md", "k")]), &schemas).unwrap();
        let bad = [
            r#"{"path": "a.md", "format": "markdown", "schema_name": "page", "data": {"body": "no frontmatter"}}"#.to_string(),
            r#"{"path": "b.jpg", "from": "/absolutely/absent.jpg"}"#.to_string(),
        ];
        let error = write_zone(zone.to_str().unwrap(), &request(&bad), &schemas).unwrap_err();
        assert_eq!(error.error_type(), "validation_error");
        assert!(error.to_string().contains("a.md") && error.to_string().contains("absent.jpg"), "{error}");
        assert!(zone.join("kept.md").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_copy_entry_holds_the_source_bytes() {
        let (zone, dir) = workspace("copy");
        let source = dir.join("photo.bin");
        fs::write(&source, [0u8, 1, 2, 255]).unwrap();
        let entry = format!(r#"{{"path": "img/photo.bin", "from": "{}"}}"#, source.display());
        write_zone(zone.to_str().unwrap(), &request(&[entry]), &schema_dir(&dir)).unwrap();
        assert_eq!(fs::read(zone.join("img/photo.bin")).unwrap(), vec![0u8, 1, 2, 255]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_report_json_matches_its_schema() {
        let (zone, dir) = workspace("report");
        let json = write_zone_json(zone.to_str().unwrap(), &request(&[page("a.md", "x")]), &schema_dir(&dir)).unwrap();
        assert!(ZONE_REPORT.is_valid(&json).unwrap());
        fs::remove_dir_all(&dir).ok();
    }
}
