//! Syn: quality policy gate.
//!
//! Named after Old Norse "syn" — denial, refusal. The gatekeeper.
//!
//! Reads .qa sidecars (Saga's truth), filters their issues via jq expressions,
//! and returns a decision (allow/warn/deny) with the formatted report. This
//! binary is the argument parsing: the I/O is `syn_runner`, the filter engine
//! `syn_core`, the rendering `report_render_core`.
//!
//! Modes:
//!   syn --mode report <path>   — filter + format issues (informational)
//!   syn --mode gate <path>     — enforce the policy (deterministic)
//!
//! Output:
//!   --output colored|json|toon   — colored on a tty and toon on a pipe when not given
//!
//! Policy, read from `.syn/` in `--project-dir` (the current directory when not given):
//!   .syn/warn.toml   — noise filter (what's visible)
//!   .syn/deny.toml   — enforcement threshold (what a gate denies)
//!
//! Exit codes:
//!   0 — the report was made; in gate mode, nothing is denied
//!   1 — gate mode denies; or a refusal or failure, its class on the first stderr line
//!   2 — wrong arguments

use clap::{Parser, ValueEnum};
use failure_mode_core::Failure;
use std::path::{Path, PathBuf};
use std::process;

use report_render_core::{OutputMode, format_output, total_issues};
use syn_core::{FilterOverrides, PolicyMode};
use syn_runner::SourceKind;

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

impl From<Target> for syn_runner::Target {
    fn from(target: Target) -> Self {
        match target {
            Target::Src => syn_runner::Target::Src,
            Target::Tests => syn_runner::Target::Tests,
            Target::All => syn_runner::Target::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
enum Kind {
    Py,
    Rs,
    Svelte,
    All,
}

/// The one source kind a `--kind` value names; `None` when it names all of them.
fn wanted_kind(kind: Kind) -> Option<SourceKind> {
    match kind {
        Kind::Py => Some(SourceKind::Python),
        Kind::Rs => Some(SourceKind::Rust),
        Kind::Svelte => Some(SourceKind::Svelte),
        Kind::All => None,
    }
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

    /// Do not broadcast the result to Hlidskjalf
    #[arg(long)]
    no_broadcast: bool,

    /// Read .qa JSON from stdin
    #[arg(long)]
    stdin: bool,

    /// The directory whose .syn/ policy is read (default: the current directory)
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
    no_broadcast: bool,
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
        no_broadcast: parsed.no_broadcast,
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
// Main
// =============================================================================

/// The overrides the command line states. A `--filter` expression that does not
/// compile is refused.
fn build_overrides(args: &Args) -> Result<FilterOverrides, Failure> {
    let custom_filter = match &args.custom_filter {
        Some(expression) => Some(
            syn_core::compile_filter(expression)
                .map_err(|reason| failure_mode_core::command_line_filter_invalid("--filter", reason))?,
        ),
        None => None,
    };

    Ok(FilterOverrides {
        tool: args.tool_filter.clone(),
        level: args.level_filter.clone(),
        custom_filter,
        narrow: args.narrow_filter.clone(),
    })
}

/// The exit code of a run that was not refused: a deny is 1, every other decision 0.
fn exit_code_of(decision: &str) -> i32 {
    match decision {
        "deny" => 1,
        _ => 0,
    }
}

/// The directory a dead-code banner is written for: the path syn was given, when it
/// is a directory and reports are not read from stdin. A file, stdin and the write
/// hook never reach here.
fn dead_code_directory(args: &Args) -> Option<&Path> {
    if args.stdin {
        return None;
    }
    let path = args.path.as_deref().unwrap_or_else(|| Path::new("."));
    path.is_dir().then_some(path)
}

/// The dead code of every project under the directory, as `dead_code_find` prints
/// it; nothing when there is none. An examination that fails says so in one line
/// and does not fail syn.
fn dead_code_banner(directory: &Path) -> String {
    match dead_code_io::find_dead_code(directory) {
        Ok(projects) => dead_code_io::render_dead_code(&projects, directory),
        Err(failure) => format!("[dead code] not examined: {failure}\n"),
    }
}

fn run(args: &Args) -> Result<i32, Failure> {
    let policy_directory = match &args.project_dir {
        Some(stated) => stated.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };

    let config = syn_runner::load_config(&policy_directory)?;
    let overrides = build_overrides(args)?;

    let (reports, unloaded) = if args.stdin {
        (syn_runner::reports_from_stdin()?, Vec::new())
    } else {
        let path = args.path.as_deref().unwrap_or_else(|| Path::new("."));
        syn_runner::discover_and_load(path, args.target.into(), wanted_kind(args.kind))
    };

    let filtered = syn_core::apply_filters(reports, &config, args.mode.into(), &overrides);
    let result = syn_core::with_faults(filtered, args.mode.into(), unloaded);
    let faults = &result.faults;

    let output = format_output(&result.warn_groups, faults, args.output);
    if !output.is_empty() {
        println!("{}", output);
    }

    if let Some(directory) = dead_code_directory(args) {
        eprint!("{}", dead_code_banner(directory));
    }

    if args.mode == Mode::Gate {
        let total = total_issues(&result.warn_groups);
        eprintln!(
            "[syn] {} — {} visible, {} deny, {} faults",
            result.decision.to_uppercase(), total, result.deny_issues, faults.len()
        );
    }

    if !args.no_broadcast {
        let scan_path = args.path.as_deref().unwrap_or(&policy_directory);
        let outcome = syn_runner::Outcome {
            groups: &result.warn_groups,
            faults,
            decision: result.decision,
            deny_count: result.deny_issues,
        };
        syn_runner::broadcast(&outcome, &policy_directory, scan_path);
    }

    Ok(exit_code_of(result.decision))
}

fn main() {
    let cli_args = CliArgs::parse();

    let args = match resolve_args(cli_args) {
        Ok(resolved) => resolved,
        Err(usage) => {
            eprintln!("{}", usage);
            process::exit(2);
        }
    };

    match run(&args) {
        Ok(code) => process::exit(code),
        Err(failure) => {
            eprintln!("{}\n{failure}", failure.class);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_from(command_line: &[&str]) -> Result<Args, String> {
        let parsed = CliArgs::try_parse_from(command_line).map_err(|clap_error| clap_error.to_string())?;
        resolve_args(parsed)
    }

    #[test]
    fn a_filter_that_compiles_becomes_an_override() {
        let args = args_from(&["syn", "--filter", ".fixable"]).unwrap();
        let overrides = build_overrides(&args).unwrap();
        assert!(overrides.custom_filter.is_some());
    }

    #[test]
    fn a_filter_that_does_not_compile_is_refused_not_dropped() {
        let args = args_from(&["syn", "--filter", ".[[["]).unwrap();
        let refused = build_overrides(&args).err().map(|failure| (failure.mode, failure.class));
        assert_eq!(refused, Some(("command_line_filter_invalid", "validation_error")));
    }

    #[test]
    fn a_refused_filter_is_reported_as_the_option_s_and_names_no_policy_file() {
        let args = args_from(&["syn", "--filter", ".[[["]).unwrap();
        let message = build_overrides(&args).err().map(|failure| failure.to_string()).unwrap_or_default();
        assert!(message.starts_with("The filter given with --filter is not a usable jq expression"), "{message}");
        assert!(!message.contains("policy file"), "{message}");
    }

    #[test]
    fn no_filter_is_no_override() {
        let args = args_from(&["syn"]).unwrap();
        assert!(build_overrides(&args).unwrap().custom_filter.is_none());
    }

    #[test]
    fn no_broadcast_is_a_flag() {
        assert!(args_from(&["syn", "--no-broadcast"]).unwrap().no_broadcast);
        assert!(!args_from(&["syn"]).unwrap().no_broadcast);
    }

    #[test]
    fn a_flag_syn_does_not_have_is_wrong_arguments() {
        assert!(args_from(&["syn", "--not-a-flag"]).is_err());
    }

    #[test]
    fn gate_mode_takes_no_override() {
        assert!(args_from(&["syn", "--mode", "gate", "--narrow", "no_print"]).is_err());
        assert!(args_from(&["syn", "--mode", "gate"]).is_ok());
    }

    #[test]
    fn only_a_deny_exits_in_failure() {
        assert_eq!(exit_code_of("deny"), 1);
        assert_eq!(exit_code_of("warn"), 0);
        assert_eq!(exit_code_of("allow"), 0);
    }

    #[test]
    fn a_kind_names_its_source_kind_and_all_names_none() {
        assert_eq!(wanted_kind(Kind::Rs), Some(SourceKind::Rust));
        assert_eq!(wanted_kind(Kind::All), None);
    }
}
