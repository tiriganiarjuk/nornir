//! Saga runner: quality tool execution and sidecar I/O.
//!
//! Capability crate — performs I/O (subprocess execution, filesystem, clock).
//! Pure types live in `saga_core`.

pub use saga_core::{Fault, Issue, SanityReport, SourceKind, qa_path, source_path_from_qa};

use directory_read_io::{list_directory, DirectoryListing, EntryKind, ListOptions};
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

/// Every file under this crate's `src/`, by its path under `src/`, with its text.
/// The routing and the running of the instruments live here and shape a report, so
/// the text is part of the examiner. A test holds the list to the directory.
const OWN_SOURCES: &[(&str, &str)] = &[("lib.rs", include_str!("lib.rs"))];

/// Everything that decides what a report holds, as texts.
#[derive(Debug, Clone, Copy)]
struct ExaminerInputs<'a> {
    /// `gleipnir_core::LOGIC_FINGERPRINT`.
    gleipnir_logic: &'a str,
    /// This crate's source files: path under `src/`, and text.
    own_sources: &'a [(&'a str, &'a str)],
    routing_list: &'a str,
    ruff_configuration: &'a str,
    pyright_configuration: &'a str,
    /// What the installed ruff answers to `--version`.
    ruff_version: &'a str,
    /// What the installed basedpyright answers to `--version`.
    basedpyright_version: &'a str,
}

/// The fingerprint of the inputs: one sha256 over all of them in a fixed order.
fn fingerprint_of_examiner(inputs: ExaminerInputs) -> String {
    let mut parts = vec![inputs.gleipnir_logic];
    for (path, text) in inputs.own_sources {
        parts.extend([*path, *text]);
    }
    parts.extend([
        inputs.routing_list,
        inputs.ruff_configuration,
        inputs.pyright_configuration,
        inputs.ruff_version,
        inputs.basedpyright_version,
    ]);
    combined_fingerprint(&parts)
}

/// The version text of an instrument from what it printed: the first line, or the
/// marker `<instrument>:unavailable` when it printed nothing or could not be run.
fn version_text(instrument: &str, printed: Option<&str>) -> String {
    printed
        .and_then(|output| output.lines().next())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map_or_else(|| format!("{instrument}:unavailable"), str::to_string)
}

/// Ask the instrument on PATH for its version.
fn installed_version(instrument: &str) -> String {
    let printed = Command::new(instrument)
        .arg("--version")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
    version_text(instrument, printed.as_deref())
}

/// The fingerprint of what examines a source in this saga, computed at first use, so
/// each instrument is asked for its version once per process.
static EXAMINER_FINGERPRINT: LazyLock<String> = LazyLock::new(|| {
    fingerprint_of_examiner(ExaminerInputs {
        gleipnir_logic: gleipnir_core::LOGIC_FINGERPRINT,
        own_sources: OWN_SOURCES,
        routing_list: V1_LEGACY_PROJECTS_TOML,
        ruff_configuration: RUFF_TOML,
        pyright_configuration: PYRIGHTCONFIG_JSON,
        ruff_version: &installed_version("ruff"),
        basedpyright_version: &installed_version("basedpyright"),
    })
});

/// One sha256 over the parts in order, each preceded by its length, so the same
/// text divided differently gives another fingerprint.
fn combined_fingerprint(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// The fingerprint of what examines a source in this saga under default routing:
/// gleipnir's logic fingerprint, this crate's own source, the V1 routing list, the
/// ruff and basedpyright configurations, and the versions of the installed ruff and
/// basedpyright. A report examined under default routing records it as
/// `examined_by`, and a sidecar that records another is stale.
pub fn examiner_fingerprint() -> &'static str {
    EXAMINER_FINGERPRINT.as_str()
}

/// What a report examined under the routing records as `examined_by`: the
/// fingerprint itself under default routing, and the fingerprint with the forced
/// version appended under a forced one — which is another examiner than the default.
fn examined_by_under(fingerprint: &str, routing: VersionOverride) -> String {
    match routing {
        VersionOverride::Auto => fingerprint.to_string(),
        VersionOverride::ForceV1 => format!("{fingerprint}+forced-v1"),
        VersionOverride::ForceV2 => format!("{fingerprint}+forced-v2"),
    }
}

/// The AI root, `$HOME/ai/`, with its closing slash.
fn ai_root() -> String {
    format!("{}/ai/", std::env::var("HOME").unwrap_or_default())
}

/// Whether a path lies in one of the listed projects. The listed paths are
/// relative to the root, so the root is stripped from the path before
/// matching; a path outside the root lies in none of them.
fn is_in_listed_project(path: &str, root: &str, listed_projects: &[String]) -> bool {
    path.strip_prefix(root)
        .is_some_and(|relative| listed_projects.iter().any(|project| relative.starts_with(project.as_str())))
}

/// Check if a file belongs to a project on the V1 legacy whitelist.
fn is_v1_legacy(file_path: &Path) -> bool {
    is_in_listed_project(&file_path.to_string_lossy(), &ai_root(), &V1_LEGACY_PATHS)
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

/// The version override set on this thread.
fn version_override() -> VersionOverride {
    VERSION_OVERRIDE.with(|cell| cell.get())
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

/// The interpreter of the project a file belongs to, found from the nearest
/// directory above the file that holds a `pyproject.toml` and an environment.
/// basedpyright resolves the project's own package and its dependencies through
/// it; given none, it checks the file against no environment and every import
/// it makes is unresolved.
fn project_interpreter(known_as: &Path) -> Option<PathBuf> {
    let derived_root = std::env::var("DERIVED_ROOT").ok().filter(|root| !root.is_empty());
    let home = std::env::var("HOME").unwrap_or_default();
    project_interpreter_under(known_as, derived_root.as_deref(), &home)
}

fn project_interpreter_under(known_as: &Path, derived_root: Option<&str>, home: &str) -> Option<PathBuf> {
    let absolute = std::path::absolute(known_as).ok()?;
    absolute
        .ancestors()
        .skip(1)
        .filter(|directory| directory.join("pyproject.toml").is_file())
        .flat_map(|directory| environment_interpreters(directory, derived_root, home))
        .find(|interpreter| interpreter.is_file())
}

/// Where a project's interpreter can be, in order: its environment on the
/// Derived volume, then a `.venv` inside it.
///
/// The Derived place is the rule of `_derived_project_venv` in
/// `~/.config/zsh/derived_volume.zsh`, which is its authority: the project's
/// path under `~/ai`, or its absolute path outside it, under
/// `$DERIVED_ROOT/uv-venvs`. A change to that rule is a change here too.
fn environment_interpreters(directory: &Path, derived_root: Option<&str>, home: &str) -> Vec<PathBuf> {
    let mut interpreters = Vec::new();
    if let Some(root) = derived_root {
        let ai_home = Path::new(home).join("ai");
        let mirrored = directory.strip_prefix(&ai_home).unwrap_or_else(|_| directory.strip_prefix("/").unwrap_or(directory));
        interpreters.push(Path::new(root).join("uv-venvs").join(mirrored).join("bin").join("python"));
    }
    interpreters.push(directory.join(".venv").join("bin").join("python"));
    interpreters
}

/// The project a file belongs to that has no interpreter for basedpyright: the
/// nearest directory above it holding a `pyproject.toml`, when no directory
/// above the file holds both that and `.venv/bin/python`. A file in no project
/// is not one of them: it has no environment to be missing.
fn project_without_environment(known_as: &Path) -> Option<PathBuf> {
    if project_interpreter(known_as).is_some() {
        return None;
    }
    let absolute = std::path::absolute(known_as).ok()?;
    absolute.ancestors().skip(1).find(|directory| directory.join("pyproject.toml").is_file()).map(Path::to_path_buf)
}

/// Run basedpyright type checker on a file using Saga's embedded config, in the
/// environment of the project the file is known to belong to. A file whose
/// project has no environment is a fault: checked without one, every import of
/// a dependency is unresolved and every type after it unknown.
pub fn run_basedpyright(source: Source) -> Result<Vec<Issue>, Fault> {
    if let Some(missing) = environment_fault(source.known_as) {
        return Err(missing);
    }
    let file_path = source.content_at;
    let path_str = file_path.to_string_lossy();
    let config = pyright_config();
    let mut command = Command::new("basedpyright");
    command.arg("--outputjson").arg("--project").arg(config.as_os_str());
    if let Some(interpreter) = project_interpreter(source.known_as) {
        command.arg("--pythonpath").arg(interpreter.as_os_str());
    }
    let output = command
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

    Ok(diags.iter().map(diagnostic_issue).collect())
}

/// The fault of a file whose project has no environment for basedpyright.
fn environment_fault(known_as: &Path) -> Option<Fault> {
    let project = project_without_environment(known_as)?;
    let reason = format!("the project at '{}' has no .venv/bin/python", project.display());
    Some(fault("basedpyright", failure_mode_core::instrument_environment_missing("basedpyright", known_as.display(), reason)))
}

/// One basedpyright diagnostic as an issue.
fn diagnostic_issue(diag: &serde_json::Value) -> Issue {
    let severity = match diag["severity"].as_str().unwrap_or("error") {
        "warning" => "warning",
        "information" => "info",
        _ => "error",
    };
    Issue {
        tool: "basedpyright".into(),
        code: diag["rule"].as_str().unwrap_or("type-error").into(),
        severity: severity.into(),
        line: diag["range"]["start"]["line"].as_u64().unwrap_or(0) as usize + 1,
        column: Some(diag["range"]["start"]["character"].as_u64().unwrap_or(0) as usize + 1),
        message: diag["message"].as_str().unwrap_or("").into(),
        category: "type".into(),
        fixable: false,
        detail: String::new(),
        signal: String::new(),
        direction: String::new(),
        canary: String::new(),
    }
}

/// The kind of a file, by the name it is known by.
fn kind_of(known_as: &Path) -> Option<SourceKind> {
    known_as.file_name().and_then(|file_name| file_name.to_str()).and_then(SourceKind::of_file_name)
}

/// The fault of a file of no kind saga examines: no instrument ran on it.
fn not_examinable(known_as: &Path) -> Fault {
    let extensions: Vec<&str> = SourceKind::ALL.iter().map(|kind| kind.extension()).collect();
    let reason = format!("its name ends in none of the extensions saga examines ({})", extensions.join(", "));
    fault("saga", failure_mode_core::source_not_examinable("saga", known_as.to_string_lossy(), reason))
}

/// Run every instrument for the file's kind: what they found, and which of them
/// could not do their work. On a file of no kind no instrument runs, and that is
/// a fault.
fn examine(source: Source) -> (Vec<Issue>, Vec<Fault>) {
    let outcomes = match kind_of(source.known_as) {
        Some(SourceKind::Rust) => vec![run_gleipnir_rust(source)],
        Some(SourceKind::Svelte) => vec![run_gleipnir_svelte(source)],
        Some(SourceKind::Python) => {
            vec![run_gleipnir(source), run_ruff(source.content_at), run_basedpyright(source)]
        }
        None => vec![Err(not_examinable(source.known_as))],
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
        examined_by: examined_by_under(examiner_fingerprint(), version_override()),
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

    let content_hash = match directory_read_io::file_sha256(&file_path.to_string_lossy()) {
        Ok(hash) => hash,
        Err(read_err) => {
            faults.push(fault("saga", read_err.failure()));
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

/// How a source file's sidecar stands to the source as it is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarState {
    /// The source has no sidecar.
    Missing,
    /// The sidecar records the source as it is now, examined by this saga's
    /// checks and configuration.
    Current,
    /// The sidecar records other content than the source holds now, or records
    /// no hash, or the source cannot be hashed; or it records another examiner
    /// than this saga's, or none.
    Stale,
    /// The sidecar is there and does not load.
    Unreadable,
}

impl SidecarState {
    /// The state's name in lowercase, as saga prints it.
    pub fn label(self) -> &'static str {
        match self {
            SidecarState::Missing => "missing",
            SidecarState::Current => "current",
            SidecarState::Stale => "stale",
            SidecarState::Unreadable => "unreadable",
        }
    }
}

/// The state of a sidecar that loaded: current when the hash it recorded is the
/// source's hash now, stale otherwise.
fn state_of_loaded(recorded_hash: &str, source_hash_now: Option<&str>) -> SidecarState {
    match source_hash_now {
        Some(hash_now) if !recorded_hash.is_empty() && hash_now == recorded_hash => SidecarState::Current,
        _ => SidecarState::Stale,
    }
}

/// Whether a report was written by the examiner named: it records an examiner,
/// and it is that one.
fn was_examined_by(recorded_examiner: &str, examiner: &str) -> bool {
    !recorded_examiner.is_empty() && recorded_examiner == examiner
}

/// The state of a source file's sidecar: the one definition of whether a sidecar
/// is current. A sidecar that loads is current when this saga's examiner wrote it
/// and the hash it recorded is the source's hash now.
pub fn sidecar_state(source_file: &Path) -> SidecarState {
    let sidecar = qa_path(source_file);
    if !sidecar.exists() {
        return SidecarState::Missing;
    }
    let Ok(report) = load_qa_file(&sidecar) else {
        return SidecarState::Unreadable;
    };
    if !was_examined_by(&report.examined_by, examiner_fingerprint()) {
        return SidecarState::Stale;
    }
    let source_hash_now = directory_read_io::file_sha256(&source_file.to_string_lossy()).ok();
    state_of_loaded(&report.content_hash, source_hash_now.as_deref())
}

/// What the orphan sweep did: the sidecars it removed, and those it could not.
#[derive(Debug, Default)]
pub struct OrphanSweep {
    pub removed: Vec<PathBuf>,
    /// A sidecar whose source is gone but which could not be removed, with why.
    pub not_removed: Vec<(PathBuf, String)>,
}

/// Remove .qa sidecars whose source the listing shows absent. A tree that cannot
/// be listed whole is a failure, and nothing is removed: absence is only what a
/// complete listing shows.
pub fn remove_orphaned_sidecars(search_dir: &Path) -> Result<OrphanSweep, Failure> {
    let listing = list_tree(search_dir, &[])?;
    let present: std::collections::HashSet<&str> = listing
        .entries
        .iter()
        .filter(|entry| entry.kind != EntryKind::Directory)
        .map(|entry| entry.path.as_str())
        .collect();

    let mut sweep = OrphanSweep::default();
    for entry in listing.entries.iter().filter(|entry| entry.kind == EntryKind::File) {
        let Some(source) = source_path_from_qa(Path::new(&entry.path)) else { continue };
        if present.contains(source.to_string_lossy().as_ref()) {
            continue;
        }
        let qa_file = Path::new(&listing.root).join(&entry.path);
        match std::fs::remove_file(&qa_file) {
            Ok(()) => sweep.removed.push(qa_file),
            Err(io_err) => sweep.not_removed.push((qa_file, io_err.to_string())),
        }
    }
    Ok(sweep)
}

// =============================================================================
// Directory walking — through the one directory walker, `directory_read_io`
// =============================================================================

/// The tree under a directory as saga walks it: hidden files kept (the sidecars),
/// hidden directories and junk directories never entered. A tree that cannot be
/// listed whole is a failure naming every path that could not be read.
fn list_tree(search_dir: &Path, extra_skip: &[&str]) -> Result<DirectoryListing, Failure> {
    let root = search_dir.canonicalize().unwrap_or_else(|_| search_dir.to_path_buf());
    let options = ListOptions {
        include_hidden: true,
        prune_hidden_directories: true,
        prune_directories: SKIP_DIRS.iter().chain(extra_skip).map(|name| name.to_string()).collect(),
        ..ListOptions::default()
    };
    list_directory(&root.to_string_lossy(), options).map_err(|list_err| list_err.failure())
}

/// The files under a directory whose name the predicate accepts, sorted.
pub fn find_files(
    search_dir: &Path,
    extra_skip: &[&str],
    predicate: &dyn Fn(&str) -> bool,
) -> Result<Vec<PathBuf>, Failure> {
    let listing = list_tree(search_dir, extra_skip)?;
    let root = Path::new(&listing.root);
    Ok(listing
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .filter(|entry| Path::new(&entry.path).file_name().and_then(|name| name.to_str()).is_some_and(predicate))
        .map(|entry| root.join(&entry.path))
        .collect())
}

// =============================================================================
// Internal helpers
// =============================================================================

/// Directories saga never enters. Hidden directories are pruned besides.
const SKIP_DIRS: &[&str] = &["__pycache__", "node_modules", "venv", "target"];

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

    /// A project directory: a `pyproject.toml`, and `.venv/bin/python` when asked for.
    fn project(root: &Path, with_interpreter: bool) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("pyproject.toml"), "[project]\nname = \"specimen\"\n").unwrap();
        if with_interpreter {
            std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
            std::fs::write(root.join(".venv/bin/python"), "").unwrap();
        }
    }

    #[test]
    fn a_file_is_checked_in_its_project_s_environment() {
        let dir = temp_dir("interpreter_found");
        project(&dir, true);
        let file = dir.join("src/pkg/logic/pure/plan/simple.py");
        assert_eq!(project_interpreter(&file), Some(dir.join(".venv/bin/python")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_without_an_environment_takes_the_nearest_one_above_it() {
        // The specimen shape: a workspace member with no .venv of its own.
        let dir = temp_dir("interpreter_workspace");
        project(&dir, true);
        project(&dir.join("members/tool"), false);
        let file = dir.join("members/tool/src/tool/cli.py");
        assert_eq!(project_interpreter(&file), Some(dir.join(".venv/bin/python")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_s_environment_on_the_derived_volume_is_found_first() {
        // The specimen: draupnir, its environment under Derived/uv-venvs, no .venv of its own.
        let dir = temp_dir("interpreter_derived");
        let home = dir.join("home");
        let project_root = home.join("ai/smidja/draupnir");
        project(&project_root, true);
        let derived = dir.join("Derived");
        let mirrored = derived.join("uv-venvs/smidja/draupnir/bin");
        std::fs::create_dir_all(&mirrored).unwrap();
        std::fs::write(mirrored.join("python"), "").unwrap();
        let file = project_root.join("src/draupnir/structure/config/manifest.py");
        let found = project_interpreter_under(&file, derived.to_str(), home.to_str().unwrap());
        assert_eq!(found, Some(mirrored.join("python")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_outside_the_ai_root_mirrors_its_absolute_path() {
        let dir = temp_dir("interpreter_outside");
        project(&dir.join("elsewhere/tool"), false);
        let derived = dir.join("Derived");
        let mirrored = derived.join("uv-venvs").join(dir.join("elsewhere/tool").strip_prefix("/").unwrap()).join("bin");
        std::fs::create_dir_all(&mirrored).unwrap();
        std::fs::write(mirrored.join("python"), "").unwrap();
        let file = dir.join("elsewhere/tool/src/tool/cli.py");
        assert_eq!(project_interpreter_under(&file, derived.to_str(), "/nonexistent/home"), Some(mirrored.join("python")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_no_derived_volume_named_an_in_project_environment_is_still_found() {
        let dir = temp_dir("interpreter_no_derived");
        project(&dir, true);
        assert_eq!(project_interpreter_under(&dir.join("src/a.py"), None, "/nonexistent/home"), Some(dir.join(".venv/bin/python")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_environment_is_named() {
        // The specimen: draupnir, a pyproject.toml and no .venv.
        let dir = temp_dir("environment_missing");
        project(&dir, false);
        let file = dir.join("src/pkg/structure/config/manifest.py");
        assert_eq!(project_without_environment(&file), Some(dir.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_in_a_project_with_an_environment_is_not_named() {
        let dir = temp_dir("environment_present");
        project(&dir, true);
        project(&dir.join("members/tool"), false);
        assert_eq!(project_without_environment(&dir.join("members/tool/src/tool/cli.py")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_in_no_project_has_no_environment_to_miss() {
        let dir = temp_dir("environment_no_project");
        assert_eq!(project_without_environment(&dir.join("loose/script.py")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_environment_is_a_basedpyright_fault() {
        let dir = temp_dir("environment_fault");
        project(&dir, false);
        let file = dir.join("src/pkg/manifest.py");
        let found = run_basedpyright(Source::in_place(&file)).unwrap_err();
        assert_eq!((found.tool.as_str(), found.mode.as_str()), ("basedpyright", "instrument_environment_missing"));
        assert!(found.message.contains("has no .venv/bin/python"), "{}", found.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_environment_with_no_project_beside_it_is_not_taken() {
        let dir = temp_dir("interpreter_none");
        std::fs::create_dir_all(dir.join(".venv/bin")).unwrap();
        std::fs::write(dir.join(".venv/bin/python"), "").unwrap();
        assert_eq!(project_interpreter(&dir.join("loose/script.py")), None);
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
    fn a_file_of_no_known_kind_is_not_examined_and_carries_the_fault() {
        let dir = temp_dir("no_kind");
        let notes = dir.join("notes.txt");
        std::fs::write(&notes, "import tomllib\n").unwrap();

        let report = generate_report(&notes, None);
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(report.faults.len(), 1, "{:?}", report.faults);
        assert_eq!(report.faults[0].tool, "saga");
        assert_eq!(report.faults[0].mode, "source_not_examinable");
        assert!(report.faults[0].message.contains("py, rs, svelte"), "{}", report.faults[0].message);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_of_a_known_kind_carries_no_fault_for_its_kind() {
        let dir = temp_dir("known_kind");
        let source = dir.join("lib.rs");
        std::fs::write(&source, "fn main() {}\n").unwrap();

        let report = generate_report(&source, None);
        assert!(report.faults.iter().all(|found| found.mode != "source_not_examinable"), "{:?}", report.faults);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_under_the_root_in_a_listed_project_is_in_it() {
        let listed = vec!["smidja/bifrost/".to_string()];
        assert!(is_in_listed_project("/home/u/ai/smidja/bifrost/src/a.py", "/home/u/ai/", &listed));
    }

    #[test]
    fn a_path_outside_the_root_or_outside_every_listed_project_is_in_none() {
        let listed = vec!["smidja/bifrost/".to_string()];
        assert!(!is_in_listed_project("/home/u/ai/smidja/nornir/src/a.py", "/home/u/ai/", &listed));
        assert!(!is_in_listed_project("/home/u/.ai/smidja/bifrost/src/a.py", "/home/u/ai/", &listed));
        assert!(!is_in_listed_project("/elsewhere/smidja/bifrost/src/a.py", "/home/u/ai/", &listed));
    }

    /// A source in a fresh directory with the sidecar saga writes for it.
    fn source_with_sidecar(test_name: &str) -> (PathBuf, PathBuf) {
        let dir = temp_dir(test_name);
        let source = dir.join("lib.rs");
        std::fs::write(&source, "fn main() {}\n").unwrap();
        save_sidecar(&generate_report(&source, None)).unwrap();
        (dir, source)
    }

    #[test]
    fn a_source_without_a_sidecar_is_missing_one() {
        let dir = temp_dir("state_missing");
        let source = dir.join("lib.rs");
        std::fs::write(&source, "fn main() {}\n").unwrap();
        assert_eq!(sidecar_state(&source), SidecarState::Missing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_written_for_the_source_as_it_is_now_is_current() {
        let (dir, source) = source_with_sidecar("state_current");
        assert_eq!(sidecar_state(&source), SidecarState::Current);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_whose_source_changed_after_it_was_written_is_stale() {
        let (dir, source) = source_with_sidecar("state_stale");
        std::fs::write(&source, "fn main() { let changed = 1; }\n").unwrap();
        assert_eq!(sidecar_state(&source), SidecarState::Stale);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Rewrite a source's sidecar with its `examined_by` set to the value, or
    /// taken out when there is none.
    fn rewrite_examined_by(source: &Path, examined_by: Option<&str>) {
        let sidecar = qa_path(source);
        let mut report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&sidecar).unwrap()).unwrap();
        let fields = report.as_object_mut().unwrap();
        match examined_by {
            Some(examiner) => fields.insert("examined_by".into(), examiner.into()),
            None => fields.remove("examined_by"),
        };
        std::fs::write(&sidecar, report.to_string()).unwrap();
    }

    #[test]
    fn a_saved_sidecar_records_this_saga_s_examiner() {
        let (dir, source) = source_with_sidecar("state_examiner");
        let loaded = load_qa_file(&qa_path(&source)).unwrap();
        assert_eq!(loaded.examined_by, examiner_fingerprint());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_rewritten_with_its_own_examiner_is_still_current() {
        let (dir, source) = source_with_sidecar("state_same_examiner");
        rewrite_examined_by(&source, Some(examiner_fingerprint()));
        assert_eq!(sidecar_state(&source), SidecarState::Current);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_examined_by_other_logic_is_stale() {
        let (dir, source) = source_with_sidecar("state_other_examiner");
        rewrite_examined_by(&source, Some("0000000000000000"));
        assert_eq!(sidecar_state(&source), SidecarState::Stale);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_that_records_no_examiner_is_stale() {
        let (dir, source) = source_with_sidecar("state_no_examiner");
        rewrite_examined_by(&source, None);
        assert_eq!(load_qa_file(&qa_path(&source)).unwrap().examined_by, "");
        assert_eq!(sidecar_state(&source), SidecarState::Stale);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_report_was_examined_by_the_examiner_it_records() {
        assert!(was_examined_by("abc", "abc"));
    }

    #[test]
    fn a_report_recording_another_examiner_or_none_was_not_examined_by_it() {
        assert!(!was_examined_by("abc", "def"));
        assert!(!was_examined_by("", "abc"));
        assert!(!was_examined_by("", ""));
    }

    #[test]
    fn the_examiner_fingerprint_is_one_sha256_and_the_same_at_every_call() {
        assert_eq!(examiner_fingerprint().len(), 64);
        assert_eq!(examiner_fingerprint(), examiner_fingerprint());
    }

    #[test]
    fn a_fingerprint_follows_every_part_and_how_the_text_is_divided() {
        assert_eq!(combined_fingerprint(&["ab", "c"]), combined_fingerprint(&["ab", "c"]));
        assert_ne!(combined_fingerprint(&["ab", "c"]), combined_fingerprint(&["ab", "d"]));
        assert_ne!(combined_fingerprint(&["ab", "c"]), combined_fingerprint(&["a", "bc"]));
    }

    fn examiner_inputs() -> ExaminerInputs<'static> {
        ExaminerInputs {
            gleipnir_logic: "logic",
            own_sources: &[("lib.rs", "source")],
            routing_list: "routing",
            ruff_configuration: "ruff configuration",
            pyright_configuration: "pyright configuration",
            ruff_version: "ruff 1.0.0",
            basedpyright_version: "basedpyright 1.0.0",
        }
    }

    #[test]
    fn the_same_examiner_inputs_give_the_same_fingerprint() {
        assert_eq!(fingerprint_of_examiner(examiner_inputs()), fingerprint_of_examiner(examiner_inputs()));
        assert_eq!(fingerprint_of_examiner(examiner_inputs()).len(), 64);
    }

    #[test]
    fn an_examiner_fingerprint_differs_when_any_one_input_differs() {
        let same = examiner_inputs();
        let changed = [
            ExaminerInputs { gleipnir_logic: "other logic", ..same },
            ExaminerInputs { own_sources: &[("lib.rs", "other source")], ..same },
            ExaminerInputs { own_sources: &[("routing.rs", "source")], ..same },
            ExaminerInputs { own_sources: &[("lib.rs", "source"), ("routing.rs", "")], ..same },
            ExaminerInputs { routing_list: "other routing", ..same },
            ExaminerInputs { ruff_configuration: "other ruff configuration", ..same },
            ExaminerInputs { pyright_configuration: "other pyright configuration", ..same },
            ExaminerInputs { ruff_version: "ruff 1.0.1", ..same },
            ExaminerInputs { ruff_version: "ruff:unavailable", ..same },
            ExaminerInputs { basedpyright_version: "basedpyright 1.0.1", ..same },
        ];
        for inputs in changed {
            assert_ne!(fingerprint_of_examiner(inputs), fingerprint_of_examiner(same), "{inputs:?}");
        }
    }

    #[test]
    fn an_instrument_s_version_is_the_first_line_it_printed() {
        assert_eq!(version_text("ruff", Some("ruff 0.9.1\nmore\n")), "ruff 0.9.1");
        assert_eq!(version_text("basedpyright", Some("  basedpyright 1.2.3  \n")), "basedpyright 1.2.3");
    }

    #[test]
    fn an_instrument_that_printed_nothing_or_could_not_be_run_is_unavailable() {
        assert_eq!(version_text("ruff", None), "ruff:unavailable");
        assert_eq!(version_text("ruff", Some("")), "ruff:unavailable");
        assert_eq!(version_text("basedpyright", Some("  \nlater\n")), "basedpyright:unavailable");
    }

    #[test]
    fn an_instrument_that_is_not_installed_has_the_unavailable_version() {
        assert_eq!(installed_version("saga-runner-no-such-instrument"), "saga-runner-no-such-instrument:unavailable");
    }

    /// The path under `src/` of every file there that is not hidden, sorted.
    fn files_under(directory: &Path, prefix: &str) -> Vec<String> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if entry.path().is_dir() {
                found.extend(files_under(&entry.path(), &format!("{prefix}{name}/")));
            } else {
                found.push(format!("{prefix}{name}"));
            }
        }
        found.sort();
        found
    }

    #[test]
    fn the_fingerprinted_sources_are_every_file_under_src() {
        let on_disk = files_under(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), "");
        let fingerprinted: Vec<&str> = OWN_SOURCES.iter().map(|(path, _)| *path).collect();
        assert_eq!(fingerprinted, on_disk);
    }

    #[test]
    fn a_report_examined_under_default_routing_records_the_fingerprint_itself() {
        assert_eq!(examined_by_under("abc", VersionOverride::Auto), "abc");
    }

    #[test]
    fn a_report_examined_under_a_forced_version_records_another_examiner() {
        assert_eq!(examined_by_under("abc", VersionOverride::ForceV1), "abc+forced-v1");
        assert_eq!(examined_by_under("abc", VersionOverride::ForceV2), "abc+forced-v2");
    }

    #[test]
    fn a_sidecar_written_under_a_forced_version_is_stale() {
        for forced in [VersionOverride::ForceV1, VersionOverride::ForceV2] {
            set_version_override(forced);
            let (dir, source) = source_with_sidecar("state_forced");
            let recorded = load_qa_file(&qa_path(&source)).unwrap().examined_by;
            set_version_override(VersionOverride::Auto);

            assert_ne!(recorded, examiner_fingerprint());
            assert_eq!(sidecar_state(&source), SidecarState::Stale);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_sidecar_written_after_a_forced_version_is_reset_is_current() {
        set_version_override(VersionOverride::ForceV1);
        set_version_override(VersionOverride::Auto);
        let (dir, source) = source_with_sidecar("state_unforced");
        assert_eq!(sidecar_state(&source), SidecarState::Current);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_whose_source_is_gone_is_stale() {
        let (dir, source) = source_with_sidecar("state_gone");
        std::fs::remove_file(&source).unwrap();
        assert_eq!(sidecar_state(&source), SidecarState::Stale);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_that_does_not_load_is_unreadable() {
        let (dir, source) = source_with_sidecar("state_unreadable");
        std::fs::write(qa_path(&source), "not a report").unwrap();
        assert_eq!(sidecar_state(&source), SidecarState::Unreadable);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_loaded_sidecar_is_current_only_with_the_hash_the_source_has_now() {
        assert_eq!(state_of_loaded("abc", Some("abc")), SidecarState::Current);
        assert_eq!(state_of_loaded("abc", Some("def")), SidecarState::Stale);
        assert_eq!(state_of_loaded("", Some("")), SidecarState::Stale);
        assert_eq!(state_of_loaded("abc", None), SidecarState::Stale);
    }

    /// A tree with a source and its sidecar, an orphaned sidecar, and files in
    /// directories saga never enters.
    fn walked_tree(test_name: &str) -> PathBuf {
        let dir = temp_dir(test_name).canonicalize().unwrap();
        for sub in ["src", ".git", "node_modules", "target"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        for file in ["src/a.py", "src/.a.py.qa", "src/.gone.py.qa", ".git/b.py", "node_modules/c.py", "target/d.py"] {
            std::fs::write(dir.join(file), "").unwrap();
        }
        dir
    }

    #[test]
    fn the_walk_keeps_hidden_files_and_never_enters_hidden_or_junk_directories() {
        let dir = walked_tree("walk");
        let sources = find_files(&dir, &[], &|name| name.ends_with(".py")).unwrap();
        assert_eq!(sources, [dir.join("src/a.py")]);
        let sidecars = find_files(&dir, &[], &|name| name.ends_with(".qa")).unwrap();
        assert_eq!(sidecars, [dir.join("src/.a.py.qa"), dir.join("src/.gone.py.qa")]);
        assert!(find_files(&dir, &["src"], &|name| name.ends_with(".py")).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tree_that_cannot_be_listed_whole_is_a_failure_naming_what_could_not_be_read() {
        use std::os::unix::fs::PermissionsExt;
        let dir = walked_tree("unlistable");
        let locked = dir.join("src");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let failure = find_files(&dir, &[], &|name| name.ends_with(".py")).unwrap_err();
        let sweep = remove_orphaned_sidecars(&dir);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(failure.class, "read_error");
        assert!(failure.to_string().contains("src"), "{failure}");
        assert!(sweep.is_err(), "nothing is removed from a tree that was not listed whole");
        assert!(dir.join("src/.gone.py.qa").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_sidecar_whose_source_the_listing_shows_absent_is_removed() {
        let dir = walked_tree("orphans");
        let sweep = remove_orphaned_sidecars(&dir).unwrap();
        assert_eq!(sweep.removed, [dir.join("src/.gone.py.qa")]);
        assert!(sweep.not_removed.is_empty());
        assert!(dir.join("src/.a.py.qa").exists());
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
