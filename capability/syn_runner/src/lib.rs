//! Syn runner: everything syn does that touches the world.
//!
//! Capability crate — performs I/O (policy files, sidecars, stdin, the Hlidskjalf
//! socket). The filter engine and the policy types live in `syn_core`; rendering
//! lives in `report_render_core`.
//!
//! Its consumers are the `syn` binary, which adds argument parsing, and the write
//! hook, which calls `assess_written_file`.

pub use saga_runner::SourceKind;

use std::io::Read;
use std::path::{Path, PathBuf};

use failure_mode_core::Failure;
use reader_io::{ReaderError, SchemaSource, SourceFormat};
use report_render_core::{CheckGroup, LocatedFault, OutputMode, format_output, report_to_json, total_issues};
use saga_runner::{SanityReport, SidecarState};
use serde_json::Value;
use syn_core::{CompiledFilter, FilterOverrides, FilteredOutput, PolicyMode, SynConfig};

// =============================================================================
// Policy loading (.syn/warn.toml, .syn/deny.toml)
// =============================================================================

/// What one policy file states. Its shape is `schemas/tools/syn-policy.schema.json`.
#[derive(serde::Deserialize)]
struct SynFilterToml {
    filter: String,
}

/// The failure of a policy file the schema refused: that it states no filter
/// when it states none, the schema's report under the file's name otherwise.
fn schema_refusal(shown_path: &str, policy_data: &Value, reader_error: ReaderError) -> Failure {
    match reader_error {
        ReaderError::ValidationFailed(_) if policy_data.get("filter").is_none() => {
            failure_mode_core::policy_filter_missing(shown_path)
        }
        ReaderError::ValidationFailed(report) => {
            failure_mode_core::data_not_matching_schema(format!("The policy file '{shown_path}':\n{report}"))
        }
        other => other.failure(),
    }
}

/// The filter a policy file states. A file that is there but cannot be read, is
/// not TOML, or does not match the schema is refused: the default never stands
/// in for it.
fn stated_filter(policy_file: &Path) -> Result<String, Failure> {
    let shown_path = policy_file.to_string_lossy();
    let (policy_json, policy_data) =
        reader_io::read_converted(SourceFormat::Toml, &shown_path).map_err(|reader_error| reader_error.failure())?;
    let schema = SchemaSource::Embedded { validator: &schemas_embedded::SYN_POLICY, verify_paths: false };
    reader_io::validate_against(&policy_data, &policy_json, &schema)
        .map_err(|reader_error| schema_refusal(&shown_path, &policy_data, reader_error))?;
    let policy: SynFilterToml =
        serde_json::from_value(policy_data).map_err(failure_mode_core::data_not_matching_schema)?;
    Ok(policy.filter)
}

/// The filter for one policy file, as its expression and compiled: the one the
/// file states, or the default when the file is absent.
pub fn load_filter_config(policy_file: &Path, default_expression: &str) -> Result<(String, CompiledFilter), Failure> {
    let expression =
        if policy_file.exists() { stated_filter(policy_file)? } else { default_expression.to_string() };
    let filter = syn_core::compile_filter(&expression)
        .map_err(|reason| failure_mode_core::policy_filter_invalid(policy_file.display(), reason))?;
    Ok((expression, filter))
}

/// The policy of a directory: the filters its `.syn/warn.toml` and
/// `.syn/deny.toml` state, each replaced by its default where the file is absent.
pub fn load_config(policy_directory: &Path) -> Result<SynConfig, Failure> {
    let syn_directory = policy_directory.join(".syn");
    let (_warn_expression, warn_filter) =
        load_filter_config(&syn_directory.join("warn.toml"), syn_core::DEFAULT_WARN_FILTER)?;
    let (_deny_expression, deny_filter) =
        load_filter_config(&syn_directory.join("deny.toml"), syn_core::DEFAULT_DENY_FILTER)?;
    Ok(SynConfig { warn_filter, deny_filter })
}

// =============================================================================
// Sidecar discovery and loading
// =============================================================================

/// The subtree of a directory whose sidecars are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Everything but `tests` directories.
    Src,
    /// Everything but `src` directories.
    Tests,
    /// The whole tree.
    All,
}

/// Whether a sidecar belongs to a source of the wanted kind: every sidecar does
/// when no kind is named.
fn is_sidecar_of_kind(sidecar_name: &str, wanted: Option<SourceKind>) -> bool {
    match wanted {
        Some(only) => SourceKind::of_sidecar_name(sidecar_name) == Some(only),
        None => true,
    }
}

/// The sidecars a path names: the sidecar itself, the sidecar of a source file,
/// or every sidecar of the wanted kind in the target subtree of a directory.
pub fn find_qa_files(path: &Path, target: Target, wanted: Option<SourceKind>) -> Result<Vec<PathBuf>, Failure> {
    if path.is_file() && path.extension().is_some_and(|extension| extension == "qa") {
        let sidecar_name = path.file_name().unwrap_or_default().to_string_lossy();
        if is_sidecar_of_kind(&sidecar_name, wanted) {
            return Ok(vec![path.to_path_buf()]);
        }
        return Ok(Vec::new());
    }

    if path.is_file() {
        let sidecar = saga_runner::qa_path(path);
        if sidecar.exists() {
            return Ok(vec![sidecar]);
        }
        return Ok(Vec::new());
    }

    let skipped_directory = match target {
        Target::Src => Some("tests"),
        Target::Tests => Some("src"),
        Target::All => None,
    };
    let extra_skip: Vec<&str> = skipped_directory.into_iter().collect();
    saga_runner::find_files(path, &extra_skip, &|file_name| {
        file_name.ends_with(".qa") && file_name.starts_with('.') && is_sidecar_of_kind(file_name, wanted)
    })
}

/// A fault syn itself reports about a path: the registry's failure, located.
fn syn_fault(about: &Path, failure: &Failure) -> LocatedFault {
    LocatedFault {
        file: about.display().to_string(),
        tool: "syn".to_string(),
        mode: failure.mode.to_string(),
        class: failure.class.to_string(),
        message: failure.standard_message(),
    }
}

/// The fault of a loaded sidecar that does not describe its source as it is now:
/// the source is gone, or holds other content than saga examined, or was
/// examined by other checks than saga's now. `None` for a current sidecar.
fn currency_fault(qa_file: &Path) -> Option<LocatedFault> {
    let source = saga_runner::source_path_from_qa(qa_file)?;
    if !source.exists() {
        let absent = failure_mode_core::sidecar_source_absent(qa_file.display(), source.display());
        return Some(syn_fault(qa_file, &absent));
    }
    match saga_runner::sidecar_state(&source) {
        SidecarState::Stale => {
            let stale = failure_mode_core::sidecar_stale(qa_file.display(), source.display());
            Some(syn_fault(qa_file, &stale))
        }
        SidecarState::Current | SidecarState::Missing | SidecarState::Unreadable => None,
    }
}

/// The reports of the sidecars that load, and the faults of the sidecars: one
/// for each that does not load — it says nothing about its file — and one for
/// each that loads and is not current, whose report is used all the same.
pub fn load_sidecars(qa_files: &[PathBuf]) -> (Vec<SanityReport>, Vec<LocatedFault>) {
    let mut reports = Vec::new();
    let mut faults = Vec::new();
    for qa_file in qa_files {
        match saga_runner::load_qa_file(qa_file) {
            Ok(report) => {
                reports.push(report);
                faults.extend(currency_fault(qa_file));
            }
            Err(failure) => faults.push(syn_fault(qa_file, &failure)),
        }
    }
    (reports, faults)
}

/// The fault of one source file that has no sidecar: saga has not examined it,
/// and the fault names the command that does. `None` for every other path — a
/// directory, a sidecar, a file of no kind saga examines, a source with its
/// sidecar.
fn unexamined_source_fault(path: &Path) -> Option<LocatedFault> {
    if !path.is_file() {
        return None;
    }
    SourceKind::of_file_name(path.file_name()?.to_str()?)?;
    if saga_runner::qa_path(path).exists() {
        return None;
    }
    Some(syn_fault(path, &failure_mode_core::sidecar_absent(path.display())))
}

/// The sidecars under a path and their reports. A tree that cannot be listed whole
/// is a fault naming what could not be read: its files are not known to be clean.
/// One source file without a sidecar is a fault too: it was not examined.
pub fn discover_and_load(
    path: &Path,
    target: Target,
    wanted: Option<SourceKind>,
) -> (Vec<SanityReport>, Vec<LocatedFault>) {
    if let Some(unexamined) = unexamined_source_fault(path) {
        return (Vec::new(), vec![unexamined]);
    }
    match find_qa_files(path, target, wanted) {
        Ok(qa_files) => load_sidecars(&qa_files),
        Err(failure) => (Vec::new(), vec![syn_fault(path, &failure)]),
    }
}

// =============================================================================
// Reports from text
// =============================================================================

/// One report from its JSON value, validated against
/// `schemas/tools/sanity-report.schema.json`.
fn report_from_value(report_value: Value) -> Result<SanityReport, Failure> {
    match schemas_embedded::SANITY_REPORT.validate(&report_value.to_string()) {
        Ok(result) if result.valid => {
            serde_json::from_value(report_value).map_err(failure_mode_core::data_not_matching_schema)
        }
        Ok(result) => Err(failure_mode_core::data_not_matching_schema(result.message)),
        Err(schema_error) => Err(failure_mode_core::embedded_schema_unusable("sanity-report", schema_error)),
    }
}

/// The reports a text holds: one report, or an array of them. Text that is not
/// JSON, and a report that does not match the schema, are refused.
pub fn reports_from_text(text: &str) -> Result<Vec<SanityReport>, Failure> {
    let parsed: Value =
        serde_json::from_str(text).map_err(|json_error| failure_mode_core::text_not_json("stdin", json_error))?;
    let report_values = match parsed {
        Value::Array(items) => items,
        single => vec![single],
    };
    report_values.into_iter().map(report_from_value).collect()
}

/// The reports handed over on stdin.
pub fn reports_from_stdin() -> Result<Vec<SanityReport>, Failure> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|io_error| failure_mode_core::file_unreadable("stdin", io_error))?;
    reports_from_text(&text)
}

// =============================================================================
// Hlidskjalf broadcast
// =============================================================================

/// What syn found, as it is broadcast.
pub struct Outcome<'a> {
    pub groups: &'a [CheckGroup],
    pub faults: &'a [LocatedFault],
    pub decision: &'a str,
    pub deny_count: usize,
}

/// Whether there is anything to report: a visible group or a fault.
pub fn has_findings(groups: &[CheckGroup], faults: &[LocatedFault]) -> bool {
    !groups.is_empty() || !faults.is_empty()
}

/// The quality datagram for one run. A fault is high priority: a file that was
/// not examined is not known to be clean.
fn quality_datagram(outcome: &Outcome, workspace: String, classifier: &str) -> datagram_io::Datagram {
    let priority = if outcome.decision == "deny" || !outcome.faults.is_empty() {
        datagram_io::Priority::High
    } else if outcome.decision == "warn" {
        datagram_io::Priority::Normal
    } else {
        datagram_io::Priority::Low
    };
    datagram_io::Datagram {
        timestamp: datagram_io::now(),
        source: "syn".to_string(),
        kind: datagram_io::DatagramKind::Quality,
        classifier: Some(classifier.into()),
        priority,
        workspace,
        detail: Some(format!(
            "{} issues, {} deny, {} faults, decision: {}",
            total_issues(outcome.groups), outcome.deny_count, outcome.faults.len(), outcome.decision
        )),
        speech: None,
        payload: Some(report_to_json(outcome.groups, outcome.faults)),
    }
}

/// Broadcast what syn found to Hlidskjalf, under the workspace of the policy
/// directory. An outcome with nothing to report is not broadcast.
pub fn broadcast(outcome: &Outcome, policy_directory: &Path, scan_path: &Path) {
    if !has_findings(outcome.groups, outcome.faults) {
        return;
    }
    let classifier = if scan_path.is_file() { "file" } else { "directory" };
    let workspace = datagram_io::workspace_from_path(policy_directory);
    datagram_io::emit_validated_or_alert(&quality_datagram(outcome, workspace, classifier), "syn");
}

// =============================================================================
// The assessment of one written file
// =============================================================================

/// What syn says of one file: whether there is nothing to report, and the report
/// as a worker session is shown it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    /// No visible group and no fault.
    pub clean: bool,
    /// The report in TOON.
    pub report_text: String,
}

/// The assessment a filtered output amounts to.
fn assessment_of(output: &FilteredOutput) -> Assessment {
    Assessment {
        clean: !has_findings(&output.warn_groups, &output.faults),
        report_text: format_output(&output.warn_groups, &output.faults, OutputMode::Toon),
    }
}

/// The failure of an assessment that did not happen: the instrument that failed,
/// and what it failed with.
fn not_run(source_file: &Path, instrument: &str, failure: &Failure) -> Failure {
    let reason = failure.standard.trim_end().trim_end_matches('.');
    failure_mode_core::quality_check_not_run(source_file.display(), instrument, reason)
}

/// Assess a file that was just written: saga examines it and writes its sidecar,
/// and syn applies the policy of `policy_directory` to the report, in report
/// mode. What there is to report is broadcast. A sidecar that cannot be written
/// and a policy that cannot be used are the failure `quality_check_not_run`.
pub fn assess_written_file(source_file: &Path, policy_directory: &Path) -> Result<Assessment, Failure> {
    let report = saga_runner::generate_report(source_file, None);
    saga_runner::save_sidecar(&report).map_err(|failure| not_run(source_file, "saga", &failure))?;
    let config = load_config(policy_directory).map_err(|failure| not_run(source_file, "syn", &failure))?;

    let output = syn_core::apply_filters(vec![report], &config, PolicyMode::Report, &FilterOverrides::default());
    let outcome = Outcome {
        groups: &output.warn_groups,
        faults: &output.faults,
        decision: output.decision,
        deny_count: output.deny_issues,
    };
    broadcast(&outcome, policy_directory, source_file);
    Ok(assessment_of(&output))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("syn_runner_{test_name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // =========================================================================
    // policy loading
    // =========================================================================

    #[test]
    fn an_absent_policy_file_leaves_the_default_filter() {
        let absent = Path::new("/nonexistent/syn_runner/warn.toml");
        let default_expression = r#".tool == "gleipnir""#;
        let (expression, _filter) = load_filter_config(absent, default_expression).unwrap();
        assert_eq!(expression, default_expression);
    }

    #[test]
    fn a_policy_file_states_the_filter_that_is_used() {
        let dir = temp_dir("stated");
        let policy_file = dir.join("warn.toml");
        std::fs::write(&policy_file, r#"filter = '.tool == "ruff"'"#).unwrap();

        let (expression, _filter) = load_filter_config(&policy_file, syn_core::DEFAULT_WARN_FILTER).unwrap();
        assert_eq!(expression, r#".tool == "ruff""#);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What loading a policy file holding `content` is refused with.
    fn refusal_for(test_name: &str, content: &[u8]) -> Failure {
        let dir = temp_dir(test_name);
        let policy_file = dir.join("warn.toml");
        std::fs::write(&policy_file, content).unwrap();
        let loaded = load_filter_config(&policy_file, syn_core::DEFAULT_WARN_FILTER);
        let _ = std::fs::remove_dir_all(&dir);
        match loaded {
            Ok((expression, _filter)) => panic!("the file was accepted with the filter {expression}"),
            Err(failure) => failure,
        }
    }

    #[test]
    fn a_policy_file_that_states_no_filter_is_refused_not_replaced_by_the_default() {
        let failure = refusal_for("nokey", b"something_else = 42\n");
        assert_eq!(failure.mode, "policy_filter_missing");
        assert_eq!(failure.class, "validation_error");
        assert!(failure.standard.contains("states no filter"), "{failure}");
    }

    #[test]
    fn a_policy_file_that_states_more_than_its_filter_is_refused_by_the_schema() {
        let failure = refusal_for("extrakey", b"filter = '.fixable'\nthreshold = 3\n");
        assert_eq!(failure.mode, "data_not_matching_schema");
        assert_eq!(failure.class, "validation_error");
        assert!(failure.standard.contains("warn.toml"), "{failure}");
    }

    #[test]
    fn a_policy_file_whose_filter_is_empty_or_not_text_is_refused_by_the_schema() {
        assert_eq!(refusal_for("emptyfilter", b"filter = ''\n").mode, "data_not_matching_schema");
        assert_eq!(refusal_for("numberfilter", b"filter = 42\n").mode, "data_not_matching_schema");
    }

    #[test]
    fn a_policy_file_that_is_not_toml_is_refused_not_replaced_by_the_default() {
        let failure = refusal_for("nottoml", b"filter = = =\n");
        assert_eq!(failure.mode, "file_not_in_format");
        assert_eq!(failure.class, "parse_error");
    }

    #[test]
    fn a_policy_file_that_cannot_be_read_is_refused_not_replaced_by_the_default() {
        let failure = refusal_for("unreadable", &[0xff, 0xfe, 0xfd]);
        assert_eq!(failure.mode, "file_unreadable");
        assert_eq!(failure.class, "read_error");
    }

    #[test]
    fn a_policy_file_whose_filter_does_not_compile_is_refused() {
        let failure = refusal_for("badjq", br#"filter = '.[[['"#);
        assert_eq!(failure.mode, "policy_filter_invalid");
        assert_eq!(failure.class, "validation_error");
    }

    fn issue_of(tool: &str, severity: &str) -> saga_runner::Issue {
        saga_runner::Issue {
            tool: tool.into(),
            code: "no_print".into(),
            severity: severity.into(),
            line: 3,
            column: None,
            message: "print call".into(),
            category: "structure".into(),
            fixable: false,
            detail: "the rule".into(),
            signal: "why".into(),
            direction: "the way forward".into(),
            canary: "the gaming move".into(),
        }
    }

    #[test]
    fn the_policy_of_a_directory_is_read_from_its_syn_directory() {
        let dir = temp_dir("policy_dir");
        std::fs::create_dir_all(dir.join(".syn")).unwrap();
        std::fs::write(dir.join(".syn/warn.toml"), r#"filter = '.tool == "ruff"'"#).unwrap();

        let config = load_config(&dir).unwrap();
        assert!(syn_core::matches_filter(&issue_of("ruff", "warning"), &config.warn_filter));
        assert!(!syn_core::matches_filter(&issue_of("gleipnir", "warning"), &config.warn_filter));
        assert!(syn_core::matches_filter(&issue_of("ruff", "blocked"), &config.deny_filter));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_whose_policy_file_is_refused_has_no_policy() {
        let dir = temp_dir("policy_dir_refused");
        std::fs::create_dir_all(dir.join(".syn")).unwrap();
        std::fs::write(dir.join(".syn/deny.toml"), "filter = = =\n").unwrap();

        let refused = load_config(&dir).err().map(|failure| failure.mode);
        assert_eq!(refused, Some("file_not_in_format"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // =========================================================================
    // sidecar discovery and loading
    // =========================================================================

    #[test]
    fn a_sidecar_is_of_the_wanted_kind_by_its_source_s_name() {
        assert!(is_sidecar_of_kind(".a.py.qa", Some(SourceKind::Python)));
        assert!(is_sidecar_of_kind(".lib.rs.qa", None));
        assert!(is_sidecar_of_kind(".notes.txt.qa", None));
    }

    #[test]
    fn a_sidecar_of_another_kind_is_not_of_the_wanted_kind() {
        assert!(!is_sidecar_of_kind(".a.py.qa", Some(SourceKind::Rust)));
        assert!(!is_sidecar_of_kind(".notes.txt.qa", Some(SourceKind::Python)));
    }

    /// A source in a fresh directory with the sidecar saga writes for it.
    fn source_with_sidecar(test_name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = temp_dir(test_name).canonicalize().unwrap();
        let source = dir.join("lib.rs");
        std::fs::write(&source, "fn main() {}\n").unwrap();
        let sidecar = saga_runner::save_sidecar(&saga_runner::generate_report(&source, None)).unwrap();
        (dir, source, sidecar)
    }

    #[test]
    fn the_sidecars_of_a_directory_are_found_by_kind() {
        let (dir, _source, sidecar) = source_with_sidecar("find");
        std::fs::write(dir.join(".a.py.qa"), "").unwrap();

        assert_eq!(find_qa_files(&dir, Target::All, Some(SourceKind::Rust)).unwrap(), [sidecar.clone()]);
        assert_eq!(find_qa_files(&dir, Target::All, None).unwrap(), [dir.join(".a.py.qa"), sidecar]);
        assert!(find_qa_files(&dir, Target::All, Some(SourceKind::Svelte)).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_current_sidecar_gives_its_report_and_no_fault() {
        let (dir, _source, sidecar) = source_with_sidecar("load_current");
        let (reports, faults) = load_sidecars(&[sidecar]);
        assert_eq!(reports.len(), 1);
        assert!(faults.is_empty(), "{faults:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_sidecar_gives_its_report_and_a_fault_naming_the_refresh() {
        let (dir, source, sidecar) = source_with_sidecar("load_stale");
        std::fs::write(&source, "fn main() { let changed = 1; }\n").unwrap();

        let (reports, faults) = load_sidecars(&[sidecar]);
        assert_eq!(reports.len(), 1);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert_eq!(faults[0].tool, "syn");
        assert_eq!(faults[0].mode, "sidecar_stale");
        assert!(faults[0].message.contains("is stale"), "{}", faults[0].message);
        assert!(faults[0].message.contains(&format!("saga {} --sidecar", source.display())), "{}", faults[0].message);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_examined_by_other_checks_gives_its_report_and_the_stale_fault() {
        let (dir, _source, sidecar) = source_with_sidecar("load_other_examiner");
        let mut report: Value = serde_json::from_str(&std::fs::read_to_string(&sidecar).unwrap()).unwrap();
        report["examined_by"] = "0000000000000000".into();
        std::fs::write(&sidecar, report.to_string()).unwrap();

        let (reports, faults) = load_sidecars(&[sidecar]);
        assert_eq!(reports.len(), 1);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert_eq!(faults[0].mode, "sidecar_stale");
        assert!(faults[0].message.contains("other than saga's checks now"), "{}", faults[0].message);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_source_file_without_a_sidecar_gives_a_fault_naming_the_command_that_examines_it() {
        let dir = temp_dir("unexamined_file").canonicalize().unwrap();
        let source = dir.join("lib.rs");
        std::fs::write(&source, "fn main() {}\n").unwrap();

        let (reports, faults) = discover_and_load(&source, Target::Src, None);
        assert!(reports.is_empty());
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert_eq!(faults[0].tool, "syn");
        assert_eq!(faults[0].mode, "sidecar_absent");
        assert_eq!(faults[0].class, "validation_error");
        assert!(faults[0].message.contains("has not examined it"), "{}", faults[0].message);
        assert!(faults[0].message.contains(&format!("saga {} --sidecar", source.display())), "{}", faults[0].message);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_source_file_with_its_sidecar_gives_its_report_and_no_fault() {
        let (dir, source, _sidecar) = source_with_sidecar("examined_file");
        let (reports, faults) = discover_and_load(&source, Target::Src, None);
        assert_eq!(reports.len(), 1);
        assert!(faults.is_empty(), "{faults:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_of_sources_without_sidecars_and_a_file_of_no_kind_give_no_fault() {
        let dir = temp_dir("unexamined_directory").canonicalize().unwrap();
        std::fs::write(dir.join("lib.rs"), "fn main() {}\n").unwrap();
        let notes = dir.join("notes.txt");
        std::fs::write(&notes, "nothing saga examines\n").unwrap();

        for path in [dir.as_path(), notes.as_path()] {
            let (reports, faults) = discover_and_load(path, Target::All, None);
            assert!(reports.is_empty(), "{}", path.display());
            assert!(faults.is_empty(), "{}: {faults:?}", path.display());
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_whose_source_is_gone_gives_a_fault_saying_so() {
        let (dir, source, sidecar) = source_with_sidecar("load_absent");
        std::fs::remove_file(&source).unwrap();

        let (reports, faults) = load_sidecars(&[sidecar]);
        assert_eq!(reports.len(), 1);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert_eq!(faults[0].mode, "sidecar_source_absent");
        assert!(faults[0].message.contains("is not there"), "{}", faults[0].message);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_that_does_not_load_gives_a_fault_and_no_report() {
        let (dir, _source, sidecar) = source_with_sidecar("load_unreadable");
        std::fs::write(&sidecar, "not a report").unwrap();

        let (reports, faults) = load_sidecars(&[sidecar]);
        assert!(reports.is_empty());
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert_eq!(faults[0].mode, "sidecar_not_matching");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // =========================================================================
    // reports from text
    // =========================================================================

    fn report_for(relative_path: &str) -> SanityReport {
        SanityReport {
            file: format!("/project/{relative_path}"),
            relative_path: relative_path.into(),
            generated_at: "1Z".into(),
            issues: vec![issue_of("gleipnir", "error")],
            ..Default::default()
        }
    }

    #[test]
    fn a_text_holding_one_report_gives_that_report() {
        let text = serde_json::to_string(&report_for("src/a.py")).unwrap();
        let reports = reports_from_text(&text).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].relative_path, "src/a.py");
    }

    #[test]
    fn a_text_holding_an_array_of_reports_gives_each_of_them() {
        let text = serde_json::to_string(&[report_for("src/a.py"), report_for("src/b.py")]).unwrap();
        let reports = reports_from_text(&text).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[1].relative_path, "src/b.py");
    }

    #[test]
    fn a_text_that_is_not_json_is_refused_as_a_parse_error() {
        let refused = reports_from_text("All checks passed.").err().map(|failure| failure.class);
        assert_eq!(refused, Some("parse_error"));
    }

    #[test]
    fn a_report_that_does_not_match_the_schema_is_refused_as_a_validation_error() {
        let without_faults = r#"{"file":"/p/a.py","relative_path":"a.py","generated_at":"1Z","elapsed_ms":1,"issues":[],"total":0,"by_tool":{},"by_severity":{},"by_category":{}}"#;
        let refused = reports_from_text(without_faults).err().map(|failure| failure.class);
        assert_eq!(refused, Some("validation_error"));

        let one_good_one_bad = format!("[{},{{}}]", serde_json::to_string(&report_for("src/a.py")).unwrap());
        let refused = reports_from_text(&one_good_one_bad).err().map(|failure| failure.mode);
        assert_eq!(refused, Some("data_not_matching_schema"));
    }

    // =========================================================================
    // the quality datagram
    // =========================================================================

    fn fault_for(relative_path: &str) -> LocatedFault {
        LocatedFault {
            file: relative_path.into(),
            tool: "ruff".into(),
            mode: "instrument_not_runnable".into(),
            class: "instrument_error".into(),
            message: format!("ruff could not be run on '{relative_path}': not found."),
        }
    }

    #[test]
    fn a_quality_datagram_with_a_group_and_a_fault_matches_the_datagram_schema() {
        let groups = report_render_core::group_issues(&[report_for("src/a.py")]);
        let faults = vec![fault_for("src/b.py")];
        let outcome = Outcome { groups: &groups, faults: &faults, decision: "warn", deny_count: 0 };
        let datagram = quality_datagram(&outcome, "@smidja:nornir".into(), "directory");

        assert!(matches!(datagram.priority, datagram_io::Priority::High));
        let json = serde_json::to_string(&datagram).unwrap();
        let result = schemas_embedded::DATAGRAM.validate(&json).unwrap();
        assert!(result.valid, "{}", result.message);
    }

    #[test]
    fn a_quality_datagram_without_a_fault_or_a_deny_is_not_high_priority() {
        let groups = report_render_core::group_issues(&[report_for("src/a.py")]);
        let outcome = Outcome { groups: &groups, faults: &[], decision: "warn", deny_count: 0 };
        let datagram = quality_datagram(&outcome, "@smidja:nornir".into(), "file");
        assert!(matches!(datagram.priority, datagram_io::Priority::Normal));
    }

    #[test]
    fn a_group_or_a_fault_is_a_finding() {
        let groups = report_render_core::group_issues(&[report_for("src/a.py")]);
        assert!(has_findings(&groups, &[]));
        assert!(has_findings(&[], &[fault_for("src/b.py")]));
    }

    #[test]
    fn no_group_and_no_fault_is_no_finding() {
        assert!(!has_findings(&[], &[]));
    }

    // =========================================================================
    // the assessment
    // =========================================================================

    fn default_config() -> SynConfig {
        SynConfig {
            warn_filter: syn_core::compile_filter(syn_core::DEFAULT_WARN_FILTER).unwrap(),
            deny_filter: syn_core::compile_filter(syn_core::DEFAULT_DENY_FILTER).unwrap(),
        }
    }

    fn filtered(report: SanityReport) -> FilteredOutput {
        syn_core::apply_filters(vec![report], &default_config(), PolicyMode::Report, &FilterOverrides::default())
    }

    #[test]
    fn a_report_with_a_visible_issue_is_not_clean_and_its_text_names_the_check() {
        let assessment = assessment_of(&filtered(report_for("src/a.py")));
        assert!(!assessment.clean);
        assert!(assessment.report_text.contains("no_print"), "{}", assessment.report_text);
    }

    #[test]
    fn a_report_with_a_fault_is_not_clean() {
        let faulted = SanityReport {
            issues: Vec::new(),
            faults: vec![saga_runner::Fault {
                tool: "ruff".into(),
                mode: "instrument_not_runnable".into(),
                class: "instrument_error".into(),
                message: "ruff could not be run.".into(),
            }],
            ..report_for("src/a.py")
        };
        assert!(!assessment_of(&filtered(faulted)).clean);
    }

    #[test]
    fn a_report_with_nothing_visible_and_no_fault_is_clean() {
        let only_hidden = SanityReport { issues: vec![issue_of("ruff", "warning")], ..report_for("src/a.py") };
        assert!(assessment_of(&filtered(only_hidden)).clean);
        let empty = SanityReport { issues: Vec::new(), ..report_for("src/a.py") };
        assert!(assessment_of(&filtered(empty)).clean);
    }

    #[test]
    fn a_written_file_with_nothing_to_report_is_clean_and_has_its_sidecar() {
        let dir = temp_dir("assess_clean");
        let source = dir.join("lib.rs");
        std::fs::write(&source, "fn main() {}\n").unwrap();

        let assessment = assess_written_file(&source, &dir).unwrap();
        assert!(assessment.clean, "{}", assessment.report_text);
        assert_eq!(saga_runner::sidecar_state(&source), SidecarState::Current);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_written_file_under_a_policy_that_cannot_be_used_is_the_check_not_run() {
        let dir = temp_dir("assess_refused");
        std::fs::create_dir_all(dir.join(".syn")).unwrap();
        std::fs::write(dir.join(".syn/warn.toml"), "something_else = 42\n").unwrap();
        let source = dir.join("lib.rs");
        std::fs::write(&source, "fn main() {}\n").unwrap();

        let failure = assess_written_file(&source, &dir).unwrap_err();
        assert_eq!(failure.mode, "quality_check_not_run");
        assert!(failure.standard.contains("syn failed"), "{}", failure.standard);
        assert!(failure.standard.contains("states no filter"), "{}", failure.standard);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
