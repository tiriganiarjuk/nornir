//! Saga runner: quality tool execution and sidecar I/O.
//!
//! Capability crate — performs I/O (subprocess execution, filesystem, clock).
//! Pure types live in `saga_core`.

pub use saga_core::{Fault, Issue, SanityReport, qa_path, source_path_from_qa};

use failure_mode_core::Failure;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Instant;

// =============================================================================
// Embedded configs — compiled into the binary, written to tmp at runtime
// =============================================================================

static RUFF_TOML: &str = include_str!("../ruff.toml");
static PYRIGHTCONFIG_JSON: &str = include_str!("../pyrightconfig.json");
static V1_LEGACY_PROJECTS_TOML: &str = include_str!("../v1_legacy_projects.toml");

/// Parsed list of project roots still checked under V1 rules, each relative
/// to the AI root (e.g. `smidja/bifrost/`). V2 zone checks are the default;
/// this whitelist is the only exception and trends toward empty as projects
/// migrate.
static V1_LEGACY_PATHS: LazyLock<Vec<String>> = LazyLock::new(|| {
    #[derive(serde::Deserialize)]
    struct ProjectEntry {
        path: String,
    }
    #[derive(serde::Deserialize)]
    struct V1LegacyProjects {
        #[serde(default)]
        projects: Vec<ProjectEntry>,
    }
    let parsed: V1LegacyProjects =
        toml::from_str(V1_LEGACY_PROJECTS_TOML).expect("v1_legacy_projects.toml parse error");
    parsed.projects.into_iter().map(|entry| entry.path).collect()
});

/// Strip the AI root prefix, yielding the portion of the path relative to it.
///
/// Recognises both `~/ai/` and `~/.ai/` so that a single table stays correct
/// across a root rename, and while the tree is split with some sections
/// migrated and others not.
fn strip_ai_root(path: &str) -> Option<&str> {
    let home = std::env::var("HOME").unwrap_or_default();
    let roots = [format!("{home}/ai/"), format!("{home}/.ai/")];
    roots.iter().find_map(|root| path.strip_prefix(root.as_str()))
}

/// Check if a file belongs to a project on the V1 legacy whitelist.
///
/// Whitelist entries are relative to the AI root, so the root is stripped
/// from the file path before matching. Files outside the AI root are never
/// legacy.
fn is_v1_legacy(file_path: &Path) -> bool {
    let path_str = file_path.to_string_lossy();
    strip_ai_root(&path_str).is_some_and(|relative| {
        V1_LEGACY_PATHS.iter().any(|prefix| relative.starts_with(prefix.as_str()))
    })
}

/// Version override for `--test v1` / `--test v2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOverride {
    /// Default routing: v2 unless the project is on the v1 legacy whitelist
    Auto,
    /// Force v1 checks regardless of project
    ForceV1,
    /// Force v2 checks regardless of project
    ForceV2,
}

// Thread-local override for gleipnir version routing.
// Set via set_version_override(), read by run_gleipnir().
std::thread_local! {
    static VERSION_OVERRIDE: std::cell::Cell<VersionOverride> = const { std::cell::Cell::new(VersionOverride::Auto) };
}

/// Set the gleipnir version override. Affects all subsequent `run_gleipnir()` calls
/// on this thread.
pub fn set_version_override(version: VersionOverride) {
    VERSION_OVERRIDE.with(|cell| cell.set(version));
}

fn use_v2(file_path: &Path) -> bool {
    VERSION_OVERRIDE.with(|cell| match cell.get() {
        VersionOverride::Auto => !is_v1_legacy(file_path),
        VersionOverride::ForceV1 => false,
        VersionOverride::ForceV2 => true,
    })
}

fn saga_tmp() -> PathBuf {
    std::env::temp_dir().join("saga")
}

fn ensure_embedded_config(filename: &str, content: &str) -> PathBuf {
    let dir = saga_tmp();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(filename);
    let _ = std::fs::write(&path, content);
    path
}

fn ruff_config() -> PathBuf {
    ensure_embedded_config("ruff.toml", RUFF_TOML)
}

fn pyright_config() -> PathBuf {
    ensure_embedded_config("pyrightconfig.json", PYRIGHTCONFIG_JSON)
}

// =============================================================================
// Runners — gleipnir (native library), ruff + basedpyright (subprocess)
// =============================================================================

fn violation_to_issue(violation: gleipnir_core::Violation) -> Issue {
    Issue {
        tool: "gleipnir".into(),
        code: violation.check_name,
        severity: match violation.severity {
            gleipnir_core::Severity::Blocked => "blocked",
            gleipnir_core::Severity::Error => "error",
            gleipnir_core::Severity::Warning => "warning",
        }
        .into(),
        line: violation.line,
        column: None,
        message: violation.message,
        category: "structure".into(),
        fixable: false,
        detail: violation.detail,
        signal: violation.signal,
        direction: violation.direction,
        canary: violation.canary,
    }
}

/// The fault an instrument reports when it could not do its work: the registry's
/// failure, carried under the instrument's name.
fn fault(tool: &str, failure: Failure) -> Fault {
    Fault {
        tool: tool.into(),
        mode: failure.mode.into(),
        class: failure.class.into(),
        message: failure.standard_message(),
    }
}

/// One of gleipnir's entry points: a path and its source in, violations out.
type GleipnirChecks = fn(&str, &[u8]) -> Result<Vec<gleipnir_core::Violation>, String>;

/// A file to examine: the path it is known by, and where its content is read from.
/// The two differ only when the content arrived on stdin and sits in a temporary
/// file. Gleipnir applies its rules by the path the file is known by.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub known_as: &'a Path,
    pub content_at: &'a Path,
}

impl<'a> Source<'a> {
    /// A file examined where it lives.
    pub fn in_place(file_path: &'a Path) -> Self {
        Source { known_as: file_path, content_at: file_path }
    }
}

fn examine_with_gleipnir(source: Source, checks: GleipnirChecks) -> Result<Vec<Issue>, Fault> {
    let known_as = source.known_as.to_string_lossy();
    let content = std::fs::read(source.content_at).map_err(|io_err| {
        fault("gleipnir", failure_mode_core::file_unreadable(source.content_at.to_string_lossy(), io_err))
    })?;
    let violations = checks(&known_as, &content)
        .map_err(|reason| fault("gleipnir", failure_mode_core::source_not_examinable("gleipnir", &known_as, reason)))?;
    Ok(violations.into_iter().map(violation_to_issue).collect())
}

/// Run gleipnir guardrail checks on a Python file.
///
/// Routes to v2 zone checks by default; projects on the v1 legacy
/// whitelist (v1_legacy_projects.toml) use v1 rules instead.
pub fn run_gleipnir(source: Source) -> Result<Vec<Issue>, Fault> {
    let checks: GleipnirChecks =
        if use_v2(source.known_as) { gleipnir_core::run_checks_v2 } else { gleipnir_core::run_checks };
    examine_with_gleipnir(source, checks)
}

/// Run gleipnir Rust checks on a file.
pub fn run_gleipnir_rust(source: Source) -> Result<Vec<Issue>, Fault> {
    examine_with_gleipnir(source, gleipnir_core::run_checks_rust)
}

/// Run gleipnir Svelte/TypeScript checks on a file.
pub fn run_gleipnir_svelte(source: Source) -> Result<Vec<Issue>, Fault> {
    examine_with_gleipnir(source, gleipnir_core::run_checks_svelte)
}

/// Why an instrument's output could not be used: what it said on stderr, or
/// failing that the parse error, with how it exited.
fn unusable_output_reason(output: &std::process::Output, parse_error: impl std::fmt::Display) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    match stderr.lines().find(|line| !line.trim().is_empty()) {
        Some(line) => format!("{} ({})", line.trim(), output.status),
        None => format!("{parse_error} ({})", output.status),
    }
}

/// Run ruff linter on a file using Saga's embedded config.
pub fn run_ruff(file_path: &Path) -> Result<Vec<Issue>, Fault> {
    let path_str = file_path.to_string_lossy();
    let config = ruff_config();
    let output = Command::new("ruff")
        .arg("check")
        .arg("--output-format=json")
        .arg("--config")
        .arg(config.as_os_str())
        .arg(file_path.as_os_str())
        .output()
        .map_err(|io_err| fault("ruff", failure_mode_core::instrument_not_runnable("ruff", &path_str, io_err)))?;

    let items: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).map_err(|json_err| {
        let reason = unusable_output_reason(&output, json_err);
        fault("ruff", failure_mode_core::instrument_output_unusable("ruff", &path_str, reason))
    })?;

    Ok(items
        .iter()
        .map(|item| {
            // Ruff gives a finding without a rule no code.
            let code = item["code"].as_str().unwrap_or("uncoded").to_string();
            Issue {
                tool: "ruff".into(),
                category: categorize_ruff(&code).into(),
                code,
                severity: "warning".into(),
                line: item["location"]["row"].as_u64().unwrap_or(1) as usize,
                column: item["location"]["column"].as_u64().map(|c| c as usize),
                message: item["message"].as_str().unwrap_or("").into(),
                fixable: item.get("fix").is_some() && !item["fix"].is_null(),
                detail: String::new(),
                signal: String::new(),
                direction: String::new(),
                canary: String::new(),
            }
        })
        .collect())
}

/// Run basedpyright type checker on a file using Saga's embedded config.
pub fn run_basedpyright(file_path: &Path) -> Result<Vec<Issue>, Fault> {
    let path_str = file_path.to_string_lossy();
    let config = pyright_config();
    let output = Command::new("basedpyright")
        .arg("--outputjson")
        .arg("--project")
        .arg(config.as_os_str())
        .arg(file_path.as_os_str())
        .output()
        .map_err(|io_err| {
            fault("basedpyright", failure_mode_core::instrument_not_runnable("basedpyright", &path_str, io_err))
        })?;

    let unusable = |reason: String| {
        fault("basedpyright", failure_mode_core::instrument_output_unusable("basedpyright", &path_str, reason))
    };
    let data: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|json_err| unusable(unusable_output_reason(&output, json_err)))?;
    let diags = data["generalDiagnostics"]
        .as_array()
        .ok_or_else(|| unusable("the output holds no generalDiagnostics list".to_string()))?;

    Ok(diags
        .iter()
        .map(|diag| {
            let severity = match diag["severity"].as_str().unwrap_or("error") {
                "error" => "error",
                "warning" => "warning",
                "information" => "info",
                _ => "error",
            };
            Issue {
                tool: "basedpyright".into(),
                code: diag["rule"].as_str().unwrap_or("type-error").into(),
                severity: severity.into(),
                line: diag["range"]["start"]["line"].as_u64().unwrap_or(0) as usize + 1,
                column: Some(
                    diag["range"]["start"]["character"].as_u64().unwrap_or(0) as usize + 1,
                ),
                message: diag["message"].as_str().unwrap_or("").into(),
                category: "type".into(),
                fixable: false,
                detail: String::new(),
                signal: String::new(),
                direction: String::new(),
                canary: String::new(),
            }
        })
        .collect())
}

/// Run every instrument for the file's kind: what they found, and which of them
/// could not do their work.
fn examine(source: Source) -> (Vec<Issue>, Vec<Fault>) {
    let outcomes = match source.known_as.extension().and_then(|e| e.to_str()) {
        Some("rs") => vec![run_gleipnir_rust(source)],
        Some("svelte") => vec![run_gleipnir_svelte(source)],
        _ => vec![run_gleipnir(source), run_ruff(source.content_at), run_basedpyright(source.content_at)],
    };
    let mut issues = Vec::new();
    let mut faults = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok(found) => issues.extend(found),
            Err(instrument_fault) => faults.push(instrument_fault),
        }
    }
    (issues, faults)
}

// =============================================================================
// Report generation
// =============================================================================

/// The report for one file from what its examination produced.
fn assemble_report(
    file_path: &Path,
    project_root: Option<&Path>,
    content_hash: String,
    elapsed_ms: u64,
    (issues, faults): (Vec<Issue>, Vec<Fault>),
) -> SanityReport {
    let relative_path = project_root
        .and_then(|root| file_path.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| tail_path(file_path, 3));
    let total = issues.len();
    let (by_tool, by_severity, by_category) = summarize_issues(&issues);

    SanityReport {
        file: file_path.to_string_lossy().to_string(),
        relative_path,
        content_hash,
        generated_at: timestamp_now(),
        elapsed_ms,
        issues,
        faults,
        total,
        by_tool,
        by_severity,
        by_category,
    }
}

/// Generate a SanityReport for a file.
pub fn generate_report(file_path: &Path, project_root: Option<&Path>) -> SanityReport {
    let file_path = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());

    let start = Instant::now();
    let (issues, mut faults) = examine(Source::in_place(&file_path));
    let elapsed_ms = start.elapsed().as_millis() as u64;

    let content_hash = match hash_file(&file_path) {
        Ok(hash) => hash,
        Err(io_err) => {
            faults.push(fault("saga", failure_mode_core::file_unreadable(file_path.to_string_lossy(), io_err)));
            String::new()
        }
    };

    assemble_report(&file_path, project_root, content_hash, elapsed_ms, (issues, faults))
}

/// Generate a SanityReport from content string (for stdin / hook usage).
pub fn generate_report_from_content(
    file_path: &Path,
    content: &str,
    project_root: Option<&Path>,
) -> SanityReport {
    let tmp_dir = saga_tmp();
    let _ = std::fs::create_dir_all(&tmp_dir);

    let file_name = file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let tmp_file = tmp_dir.join(file_name.as_ref());
    let content_hash = hash_content(content);

    if let Err(io_err) = std::fs::write(&tmp_file, content) {
        let not_examined = fault("saga", failure_mode_core::file_unwritable(tmp_file.to_string_lossy(), io_err));
        return assemble_report(file_path, project_root, content_hash, 0, (Vec::new(), vec![not_examined]));
    }

    let start = Instant::now();
    let findings = examine(Source { known_as: file_path, content_at: &tmp_file });
    let elapsed_ms = start.elapsed().as_millis() as u64;

    let _ = std::fs::remove_file(&tmp_file);

    assemble_report(file_path, project_root, content_hash, elapsed_ms, findings)
}

// =============================================================================
// Sidecar I/O
// =============================================================================

/// Whether report JSON matches `schemas/tools/sanity-report.schema.json`, or why not.
fn check_against_schema(report_json: &str) -> Result<(), String> {
    match schemas_embedded::SANITY_REPORT.validate(report_json) {
        Ok(result) if result.valid => Ok(()),
        Ok(result) => Err(result.message),
        Err(schema_err) => Err(schema_err.to_string()),
    }
}

/// Write a SanityReport to its .qa sidecar. A report that does not match the
/// schema is not written.
pub fn save_sidecar(report: &SanityReport) -> Result<PathBuf, Failure> {
    let path = qa_path(Path::new(&report.file));
    let json = serde_json::to_string_pretty(report)
        .map_err(|json_err| failure_mode_core::sanity_report_invalid(&report.file, json_err))?;
    check_against_schema(&json).map_err(|reason| failure_mode_core::sanity_report_invalid(&report.file, reason))?;
    std::fs::write(&path, json)
        .map_err(|io_err| failure_mode_core::file_unwritable(path.to_string_lossy(), io_err))?;
    Ok(path)
}

/// Load a SanityReport from a .qa file. A sidecar that cannot be read, or does
/// not match the schema, is a failure saying which and why.
pub fn load_qa_file(qa_file: &Path) -> Result<SanityReport, Failure> {
    let shown = qa_file.to_string_lossy();
    let content = std::fs::read_to_string(qa_file)
        .map_err(|io_err| failure_mode_core::file_unreadable(&shown, io_err))?;
    check_against_schema(&content).map_err(|reason| failure_mode_core::sidecar_not_matching(&shown, reason))?;
    serde_json::from_str(&content).map_err(|json_err| failure_mode_core::sidecar_not_matching(&shown, json_err))
}

/// Remove .qa sidecars whose source files no longer exist.
pub fn remove_orphaned_sidecars(search_dir: &Path) -> Vec<PathBuf> {
    let qa_files = find_files(search_dir, &[], &|name| {
        name.ends_with(".qa") && name.starts_with('.')
    });
    let mut removed = Vec::new();
    for qa_file in qa_files {
        if let Some(source) = source_path_from_qa(&qa_file) {
            if !source.exists() {
                if std::fs::remove_file(&qa_file).is_ok() {
                    removed.push(qa_file);
                }
            }
        }
    }
    removed
}

// =============================================================================
// Directory walking
// =============================================================================

/// Recursively collect files matching a predicate, skipping junk directories.
pub fn walk_files(
    search_dir: &Path,
    extra_skip: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    results: &mut Vec<PathBuf>,
) {
    let entries = match std::fs::read_dir(search_dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            if name.starts_with('.')
                || SKIP_DIRS.contains(&name.as_str())
                || extra_skip.contains(&name.as_str())
            {
                continue;
            }
            walk_files(&path, extra_skip, predicate, results);
        } else if predicate(&name) {
            results.push(path);
        }
    }
}

/// Collect files matching a predicate under a directory, sorted.
pub fn find_files(
    search_dir: &Path,
    extra_skip: &[&str],
    predicate: &dyn Fn(&str) -> bool,
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_files(search_dir, extra_skip, predicate, &mut files);
    files.sort();
    files
}

// =============================================================================
// Internal helpers
// =============================================================================

/// Directories always skipped during recursive file discovery.
const SKIP_DIRS: &[&str] = &["__pycache__", "node_modules", ".venv", "venv", "target"];

/// Last N path components as a string. Disambiguates `lib.rs` across crates.
fn tail_path(path: &Path, depth: usize) -> String {
    let components: Vec<_> = path.components().collect();
    let start = components.len().saturating_sub(depth);
    components[start..]
        .iter()
        .collect::<PathBuf>()
        .to_string_lossy()
        .to_string()
}

fn hash_content(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn summarize_issues(
    issues: &[Issue],
) -> (
    HashMap<String, usize>,
    HashMap<String, usize>,
    HashMap<String, usize>,
) {
    let mut by_tool: HashMap<&str, usize> = HashMap::new();
    let mut by_severity: HashMap<&str, usize> = HashMap::new();
    let mut by_category: HashMap<&str, usize> = HashMap::new();
    for issue in issues {
        *by_tool.entry(&issue.tool).or_default() += 1;
        *by_severity.entry(&issue.severity).or_default() += 1;
        *by_category.entry(&issue.category).or_default() += 1;
    }
    let to_owned = |map: HashMap<&str, usize>| -> HashMap<String, usize> {
        map.into_iter().map(|(key, count)| (key.to_string(), count)).collect()
    };
    (to_owned(by_tool), to_owned(by_severity), to_owned(by_category))
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let bytes_read = file.read(&mut buf)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buf[..bytes_read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn timestamp_now() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}Z", duration.as_secs())
}

/// Ruff code → category mapping table.
const RUFF_CATEGORIES: &[(&str, &str)] = &[
    ("PERF", "complexity"),
    ("ASYNC", "lint"),
    ("ANN", "type"),
    ("COM", "style"),
    ("CPY", "style"),
    ("DTZ", "lint"),
    ("T10", "lint"),
    ("T20", "lint"),
    ("EM", "style"),
    ("EXE", "lint"),
    ("FA", "type"),
    ("ISC", "style"),
    ("ICN", "style"),
    ("LOG", "lint"),
    ("INP", "lint"),
    ("PIE", "lint"),
    ("PYI", "type"),
    ("PT", "lint"),
    ("RSE", "lint"),
    ("RET", "lint"),
    ("SLF", "lint"),
    ("SIM", "lint"),
    ("TID", "style"),
    ("TCH", "type"),
    ("ARG", "lint"),
    ("PTH", "lint"),
    ("TD", "style"),
    ("FIX", "style"),
    ("ERA", "lint"),
    ("PD", "lint"),
    ("PGH", "lint"),
    ("PL", "lint"),
    ("TRY", "lint"),
    ("FLY", "lint"),
    ("NPY", "lint"),
    ("FURB", "lint"),
    ("RUF", "lint"),
    ("UP", "style"),
    ("YTT", "lint"),
    ("BLE", "lint"),
    ("FBT", "lint"),
    ("C4", "lint"),
    ("DJ", "lint"),
    ("SLOT", "lint"),
    ("INT", "lint"),
    ("E", "style"),
    ("W", "style"),
    ("F", "lint"),
    ("C", "complexity"),
    ("I", "style"),
    ("N", "style"),
    ("D", "style"),
    ("S", "lint"),
    ("B", "lint"),
    ("A", "lint"),
    ("G", "style"),
    ("Q", "style"),
];

fn categorize_ruff(code: &str) -> &'static str {
    for (prefix, category) in RUFF_CATEGORIES {
        if code.starts_with(prefix) {
            return category;
        }
    }
    "lint"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("saga_runner_{test_name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn content_held_in_a_temporary_file_is_examined_under_the_path_it_is_known_by() {
        let dir = temp_dir("known_as");
        let held = dir.join("simple.py");
        std::fs::write(&held, "import tomllib\n").unwrap();
        let known_as = Path::new("/project/src/pkg/logic/impure/registry/simple.py");

        let issues = run_gleipnir(Source { known_as, content_at: &held }).unwrap();
        let codes: Vec<&str> = issues.iter().map(|issue| issue.code.as_str()).collect();
        assert!(codes.contains(&"no_direct_file_io"), "{codes:?}");
        assert!(!codes.contains(&"unknown_file_in_zone"), "{codes:?}");
        assert!(issues.iter().all(|issue| !issue.message.contains("outside v2 zone layout")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_gleipnir_issue_carries_its_check_s_detail() {
        let dir = temp_dir("detail");
        let held = dir.join("simple.py");
        std::fs::write(&held, "import tomllib\n").unwrap();
        let known_as = Path::new("/project/src/pkg/logic/impure/registry/simple.py");

        let issues = run_gleipnir(Source { known_as, content_at: &held }).unwrap();
        let hit = issues.iter().find(|issue| issue.code == "no_direct_file_io").unwrap();
        assert_eq!(hit.detail, gleipnir_core::messages::NO_DIRECT_FILE_IO.detail);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_source_that_cannot_be_read_is_a_fault_not_an_empty_result() {
        let missing = Path::new("/nonexistent/saga_runner/missing.rs");
        let not_examined = run_gleipnir_rust(Source::in_place(missing)).unwrap_err();
        assert_eq!(not_examined.tool, "gleipnir");
        assert_eq!(not_examined.mode, "file_unreadable");
        assert_eq!(not_examined.class, "read_error");
    }

    #[test]
    fn a_report_of_an_unreadable_file_carries_its_faults_and_no_hash() {
        let missing = Path::new("/nonexistent/saga_runner/missing.rs");
        let report = generate_report(missing, None);
        assert!(report.issues.is_empty());
        assert_eq!(report.faults.len(), 2, "{:?}", report.faults);
        assert!(report.content_hash.is_empty());
    }

    #[test]
    fn a_saved_sidecar_loads_back_and_matches_the_schema() {
        let dir = temp_dir("roundtrip");
        let source = dir.join("lib.rs");
        std::fs::write(&source, "fn main() { let x = Some(1).unwrap(); }\n").unwrap();

        let report = generate_report(&source, None);
        let sidecar = save_sidecar(&report).unwrap();
        let loaded = load_qa_file(&sidecar).unwrap();
        assert_eq!(loaded.total, report.total);
        assert!(loaded.faults.is_empty());
        assert_eq!(loaded.content_hash.len(), 64);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_without_faults_is_refused_as_not_matching() {
        let dir = temp_dir("old_format");
        let sidecar = dir.join(".a.py.qa");
        let old_format = r#"{"file":"/p/a.py","relative_path":"a.py","content_hash":"","generated_at":"1Z","elapsed_ms":1,"issues":[],"total":0,"by_tool":{},"by_severity":{},"by_category":{}}"#;
        std::fs::write(&sidecar, old_format).unwrap();

        let failure = load_qa_file(&sidecar).unwrap_err();
        assert_eq!(failure.mode, "sidecar_not_matching");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
