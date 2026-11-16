//! Syn: quality policy gate.
//!
//! Named after Old Norse "syn" — denial, refusal. The gatekeeper.
//!
//! Reads .qa sidecars (Saga's truth), filters via jq expressions (jaq-interpret),
//! returns decision (allow/warn/deny) + formatted output.
//!
//! Modes:
//!   syn --mode report <path>   — filter + format issues (informational)
//!   syn --mode gate <path>     — enforce policy, exit 0/1 (deterministic)
//!
//! Output:
//!   (default)   TOON when piped, --colored when tty
//!   --colored   ANSI terminal output
//!   --json      Machine-readable JSON
//!
//! Config:
//!   .syn/warn.toml   — noise filter (what's visible)
//!   .syn/deny.toml   — enforcement threshold (what blocks)

use clap::{Parser, ValueEnum};
use std::io::{self, Read as IoRead};
use std::path::{Path, PathBuf};
use std::process;

use saga_runner::SanityReport;
use syn_core::{FilterOverrides, PolicyMode, SynConfig};

use report_render_core::{LocatedFault, OutputMode, total_issues, report_to_json, format_output};

// =============================================================================
// Config loading (.syn/warn.toml, .syn/deny.toml)
// =============================================================================

#[derive(serde::Deserialize)]
struct SynFilterToml {
    filter: Option<String>,
}

fn load_filter_config(config_path: &Path, default_expr: &str) -> Result<(String, syn_core::CompiledFilter), String> {
    let expr = if config_path.exists() {
        std::fs::read_to_string(config_path)
            .ok()
            .and_then(|content| {
                let config: SynFilterToml = toml::from_str(&content).ok()?;
                config.filter
            })
            .unwrap_or_else(|| default_expr.to_string())
    } else {
        default_expr.to_string()
    };

    let filter = syn_core::compile_filter(&expr)
        .map_err(|e| format!("[syn] fatal: bad jq filter in {}: {}", config_path.display(), e))?;

    Ok((expr, filter))
}

fn load_config(project_dir: &Path) -> Result<SynConfig, String> {
    let syn_dir = project_dir.join(".syn");
    let (_warn_expr, warn_filter) = load_filter_config(&syn_dir.join("warn.toml"), syn_core::DEFAULT_WARN_FILTER)?;
    let (_deny_expr, deny_filter) = load_filter_config(&syn_dir.join("deny.toml"), syn_core::DEFAULT_DENY_FILTER)?;
    Ok(SynConfig { warn_filter, deny_filter })
}

// =============================================================================
// CLI args
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
enum Mode {
    Report,
    Gate,
}

impl From<Mode> for PolicyMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Report => PolicyMode::Report,
            Mode::Gate => PolicyMode::Gate,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
enum Target {
    Src,
    Tests,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
enum Kind {
    Py,
    Rs,
    Svelte,
    All,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Output {
    Colored,
    Json,
    Toon,
}

impl From<Output> for OutputMode {
    fn from(output: Output) -> Self {
        match output {
            Output::Colored => OutputMode::Colored,
            Output::Json => OutputMode::Json,
            Output::Toon => OutputMode::Toon,
        }
    }
}

fn is_tty() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

/// Syn — quality policy gate. Reads .qa sidecars, filters, and enforces policy.
#[derive(Parser)]
#[command(name = "syn")]
struct CliArgs {
    /// File, directory, or omit for cwd
    path: Option<PathBuf>,

    /// Operating mode
    #[arg(long, default_value = "report")]
    mode: Mode,

    /// Output format (default: colored on tty, toon on pipe)
    #[arg(long)]
    output: Option<Output>,

    /// Suppress Hlidskjalf broadcast
    #[arg(long)]
    silent: bool,

    /// Read .qa JSON from stdin
    #[arg(long)]
    stdin: bool,

    /// Project root for relative paths
    #[arg(long)]
    project_dir: Option<PathBuf>,

    /// Subtree scope
    #[arg(long, default_value = "src")]
    target: Target,

    /// File types to include
    #[arg(long, default_value = "all")]
    kind: Kind,

    /// Filter by tool name (report mode only)
    #[arg(long)]
    tool: Option<String>,

    /// Minimum severity level (report mode only)
    #[arg(long)]
    level: Option<String>,

    /// Custom jq filter expression (report mode only)
    #[arg(long)]
    filter: Option<String>,

    /// Show only a specific check code (report mode only)
    #[arg(long)]
    narrow: Option<String>,
}

struct Args {
    mode: Mode,
    output: OutputMode,
    silent: bool,
    stdin: bool,
    project_dir: Option<PathBuf>,
    target: Target,
    kind: Kind,
    tool_filter: Option<String>,
    level_filter: Option<String>,
    custom_filter: Option<String>,
    narrow_filter: Option<String>,
    path: Option<PathBuf>,
}

fn resolve_args(parsed: CliArgs) -> Result<Args, String> {
    if parsed.mode == Mode::Gate {
        if parsed.tool.is_some() || parsed.level.is_some() || parsed.filter.is_some() || parsed.narrow.is_some() {
            return Err("[syn] gate mode rejects --tool/--level/--filter/--narrow (locked to config)".into());
        }
    }

    let output = parsed.output
        .map(OutputMode::from)
        .unwrap_or_else(|| if is_tty() { OutputMode::Colored } else { OutputMode::Toon });

    Ok(Args {
        mode: parsed.mode,
        output,
        silent: parsed.silent,
        stdin: parsed.stdin,
        project_dir: parsed.project_dir,
        target: parsed.target,
        kind: parsed.kind,
        tool_filter: parsed.tool,
        level_filter: parsed.level,
        custom_filter: parsed.filter,
        narrow_filter: parsed.narrow,
        path: parsed.path,
    })
}

// =============================================================================
// Input discovery
// =============================================================================

fn matches_kind(qa_name: &str, kind: Kind) -> bool {
    match kind {
        Kind::All => true,
        Kind::Py => qa_name.ends_with(".py.qa"),
        Kind::Rs => qa_name.ends_with(".rs.qa"),
        Kind::Svelte => qa_name.ends_with(".svelte.qa"),
    }
}

fn find_qa_files(path: &Path, target: Target, kind: Kind) -> Vec<PathBuf> {
    if path.is_file() && path.extension().map_or(false, |e| e == "qa") {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if matches_kind(&name, kind) {
            return vec![path.to_path_buf()];
        }
        return Vec::new();
    }

    if path.is_file() {
        let sidecar = saga_runner::qa_path(path);
        if sidecar.exists() {
            return vec![sidecar];
        }
        return Vec::new();
    }

    let mut results = Vec::new();
    let skip = match target {
        Target::Src => Some("tests"),
        Target::Tests => Some("src"),
        Target::All => None,
    };
    let extra_skip: Vec<&str> = skip.into_iter().collect();
    saga_runner::walk_files(
        path,
        &extra_skip,
        &|name| name.ends_with(".qa") && name.starts_with('.') && matches_kind(name, kind),
        &mut results,
    );
    results
}

/// The reports of the sidecars that load, and a fault for each one that does not:
/// a sidecar syn cannot read says nothing about its file.
fn load_sidecars(qa_files: &[PathBuf]) -> (Vec<SanityReport>, Vec<LocatedFault>) {
    let mut reports = Vec::new();
    let mut unloaded = Vec::new();
    for qa_file in qa_files {
        match saga_runner::load_qa_file(qa_file) {
            Ok(report) => reports.push(report),
            Err(failure) => unloaded.push(LocatedFault {
                file: qa_file.display().to_string(),
                tool: "syn".to_string(),
                mode: failure.mode.to_string(),
                class: failure.class.to_string(),
                message: failure.standard_message(),
            }),
        }
    }
    (reports, unloaded)
}

fn load_reports_from_stdin() -> Result<Vec<SanityReport>, String> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)
        .map_err(|_| "[syn] failed to read stdin".to_string())?;

    if let Ok(report) = serde_json::from_str::<SanityReport>(&buf) {
        return Ok(vec![report]);
    }
    if let Ok(reports) = serde_json::from_str::<Vec<SanityReport>>(&buf) {
        return Ok(reports);
    }

    Err("[syn] stdin is not valid .qa JSON".to_string())
}

// =============================================================================
// Hlidskjalf broadcast
// =============================================================================

/// What syn found, as it is broadcast.
struct Outcome<'a> {
    groups: &'a [report_render_core::CheckGroup],
    faults: &'a [LocatedFault],
    decision: &'a str,
    deny_count: usize,
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

fn broadcast(outcome: &Outcome, workspace: String, scan_path: &Path) {
    let classifier = if scan_path.is_file() { "file" } else { "directory" };
    datagram_io::emit_validated_or_alert(&quality_datagram(outcome, workspace, classifier), "syn");
}

// =============================================================================
// Main
// =============================================================================

fn build_overrides(args: &Args) -> FilterOverrides {
    let custom_compiled = args.custom_filter
        .as_ref()
        .and_then(|expr| syn_core::compile_filter(expr).ok());

    FilterOverrides {
        tool: args.tool_filter.clone(),
        level: args.level_filter.clone(),
        custom_filter: custom_compiled,
        narrow: args.narrow_filter.clone(),
    }
}

fn run(args: &Args) -> Result<i32, String> {
    let default_dir;
    let project_dir = match &args.project_dir {
        Some(dir) => dir.as_path(),
        None => {
            default_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            default_dir.as_path()
        }
    };

    let config = load_config(project_dir)?;

    let (reports, unloaded) = if args.stdin {
        (load_reports_from_stdin()?, Vec::new())
    } else {
        let path = args.path.as_deref().unwrap_or_else(|| Path::new("."));
        load_sidecars(&find_qa_files(path, args.target, args.kind))
    };

    let overrides = build_overrides(args);
    let result = syn_core::apply_filters(reports, &config, args.mode.into(), &overrides);
    let faults: Vec<LocatedFault> = unloaded.into_iter().chain(result.faults).collect();

    let output = format_output(&result.warn_groups, &faults, args.output);
    if !output.is_empty() {
        println!("{}", output);
    }

    if args.mode == Mode::Gate {
        let total = total_issues(&result.warn_groups);
        eprintln!(
            "[syn] {} — {} visible, {} deny, {} faults",
            result.decision.to_uppercase(), total, result.deny_issues, faults.len()
        );
    }

    if !args.silent && (!result.warn_groups.is_empty() || !faults.is_empty()) {
        let scan_path = args.path.as_deref().unwrap_or(project_dir);
        let workspace = datagram_io::workspace_from_path(project_dir);
        let outcome = Outcome {
            groups: &result.warn_groups,
            faults: &faults,
            decision: result.decision,
            deny_count: result.deny_issues,
        };
        broadcast(&outcome, workspace, scan_path);
    }

    match result.decision {
        "deny" => Ok(1),
        _ => Ok(0),
    }
}

fn main() {
    let cli_args = CliArgs::parse();

    let args = match resolve_args(cli_args) {
        Ok(resolved) => resolved,
        Err(err) => {
            eprintln!("{}", err);
            process::exit(2);
        }
    };

    match run(&args) {
        Ok(code) => process::exit(code),
        Err(err) => {
            eprintln!("{}", err);
            process::exit(2);
        }
    }
}

// =============================================================================
// Tests (I/O tests that stay in the binary)
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // load_filter_config
    // =========================================================================

    #[test]
    fn load_filter_config_nonexistent_path_uses_default() {
        let path = std::path::Path::new("/nonexistent/path/warn.toml");
        let default_expr = r#".tool == "gleipnir""#;
        let (expr, _filter) = load_filter_config(path, default_expr).unwrap();
        assert_eq!(expr, default_expr);
    }

    #[test]
    fn load_filter_config_valid_toml_file() {
        let dir = std::env::temp_dir().join(format!("syn_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("warn.toml");
        std::fs::write(&config_path, r#"filter = '.tool == "ruff"'"#).unwrap();

        let (expr, _filter) = load_filter_config(&config_path, syn_core::DEFAULT_WARN_FILTER).unwrap();
        assert_eq!(expr, r#".tool == "ruff""#);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_filter_config_toml_missing_filter_key_uses_default() {
        let dir = std::env::temp_dir().join(format!("syn_test_nokey_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("warn.toml");
        std::fs::write(&config_path, "something_else = 42\n").unwrap();

        let (expr, _filter) = load_filter_config(&config_path, syn_core::DEFAULT_WARN_FILTER).unwrap();
        assert_eq!(expr, syn_core::DEFAULT_WARN_FILTER);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // =========================================================================
    // the quality datagram
    // =========================================================================

    #[test]
    fn a_quality_datagram_with_a_group_and_a_fault_matches_the_datagram_schema() {
        let issue = saga_runner::Issue {
            tool: "gleipnir".into(),
            code: "no_print".into(),
            severity: "error".into(),
            line: 3,
            column: None,
            message: "print call".into(),
            category: "structure".into(),
            fixable: false,
            detail: "the rule".into(),
            signal: "why".into(),
            direction: "the way forward".into(),
            canary: "the gaming move".into(),
        };
        let report = SanityReport {
            file: "/project/src/a.py".into(),
            relative_path: "src/a.py".into(),
            issues: vec![issue],
            ..Default::default()
        };
        let groups = report_render_core::group_issues(&[report]);
        let faults = vec![LocatedFault {
            file: "src/b.py".into(),
            tool: "ruff".into(),
            mode: "instrument_not_runnable".into(),
            class: "instrument_error".into(),
            message: "ruff could not be run on 'src/b.py': not found.".into(),
        }];
        let outcome = Outcome { groups: &groups, faults: &faults, decision: "warn", deny_count: 0 };
        let datagram = quality_datagram(&outcome, "@smidja:nornir".into(), "directory");

        assert!(matches!(datagram.priority, datagram_io::Priority::High));
        let json = serde_json::to_string(&datagram).unwrap();
        let result = schemas_embedded::DATAGRAM.validate(&json).unwrap();
        assert!(result.valid, "{}", result.message);
    }

    #[test]
    fn load_filter_config_invalid_jq_in_file_returns_error() {
        let dir = std::env::temp_dir().join(format!("syn_test_badjq_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("deny.toml");
        std::fs::write(&config_path, r#"filter = '.[[['"#).unwrap();

        let result = load_filter_config(&config_path, syn_core::DEFAULT_DENY_FILTER);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
