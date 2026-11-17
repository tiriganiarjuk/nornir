//! Saga CLI: quality truth recorder.
//!
//! Runs the quality instruments on source files (Python, Rust, Svelte) and
//! records what they found as a .qa report.
//!
//! Usage:
//!   saga <file>                                    — examine, JSON to stdout
//!   saga <file> --sidecar                          — examine, write the .qa sidecar too
//!   saga <dir> [--kind py|rs|svelte|all]           — remove orphaned .qa, write every sidecar that is not current
//!   saga <dir> --force [--kind py|rs|svelte|all]   — remove orphaned .qa, write every sidecar
//!   saga <dir> --check [--kind py|rs|svelte|all]   — print every source whose sidecar is not current
//!   saga <dir> --orphans                           — remove only the .qa whose source is gone; examine nothing
//!   saga <dir> --strip                             — remove ALL .qa sidecars recursively
//!   echo "content" | saga --stdin <file>           — examine stdin content
//!
//! File vs directory is auto-detected from the path.
//!
//! Exit codes: 0 success, 1 refusal or failure, 2 wrong arguments.

use clap::{Parser, ValueEnum};
use saga_runner::{SidecarState, SourceKind};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

// =============================================================================
// CLI
// =============================================================================

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Py,
    Rs,
    Svelte,
    All,
}

#[derive(Clone, Copy, ValueEnum)]
enum TestVersion {
    V1,
    V2,
}

/// Saga — quality truth recorder. Runs quality tools on source files.
#[derive(Parser)]
#[command(name = "saga")]
struct Args {
    /// File or directory to process (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Write every sidecar, also where it is current (directory only)
    #[arg(long)]
    force: bool,

    /// Print every source whose sidecar is not current, and change nothing (directory only)
    #[arg(long, conflicts_with_all = ["force", "strip"])]
    check: bool,

    /// Remove ALL .qa sidecars recursively (directory only)
    #[arg(long)]
    strip: bool,

    /// Remove only the .qa sidecars whose source is gone, and examine nothing (directory only)
    #[arg(long, conflicts_with_all = ["force", "strip", "check"])]
    orphans: bool,

    /// Write .qa sidecar file (file mode)
    #[arg(long)]
    sidecar: bool,

    /// Read file content from stdin
    #[arg(long)]
    stdin: bool,

    /// Project root for relative paths
    #[arg(long)]
    project_dir: Option<PathBuf>,

    /// File types to process in directory mode
    #[arg(long, default_value = "all")]
    kind: Kind,

    /// Override gleipnir version routing (ignore the v1 legacy whitelist)
    #[arg(long)]
    test: Option<TestVersion>,
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

/// What is wrong with a directory-only flag given with a path that is no directory.
fn directory_flag_without_directory(strip: bool, check: bool, orphans: bool, path_is_directory: bool) -> Option<&'static str> {
    if path_is_directory {
        None
    } else if strip {
        Some("[saga] --strip requires a directory")
    } else if check {
        Some("[saga] --check requires a directory")
    } else if orphans {
        Some("[saga] --orphans requires a directory")
    } else {
        None
    }
}

// =============================================================================
// Directory mode
// =============================================================================

/// What a refusal prints: the failure class on the first line, then the message.
fn refusal(failure: failure_mode_core::Failure) -> String {
    format!("{}\n{failure}", failure.class)
}

/// Whether a file is a source of the wanted kind: of any kind when none is named.
fn is_wanted_source(file_name: &str, wanted: Option<SourceKind>) -> bool {
    match (SourceKind::of_file_name(file_name), wanted) {
        (Some(found), Some(only)) => found == only,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn find_source_files(search_dir: &Path, wanted: Option<SourceKind>) -> Result<Vec<PathBuf>, String> {
    saga_runner::find_files(search_dir, &[], &|file_name| is_wanted_source(file_name, wanted)).map_err(refusal)
}

/// The extensions of the wanted kind as a summary line shows them: `.py`, or
/// `.py/.rs/.svelte` when none is named.
fn kind_label(wanted: Option<SourceKind>) -> String {
    let dotted = |kind: SourceKind| format!(".{}", kind.extension());
    match wanted {
        Some(only) => dotted(only),
        None => SourceKind::ALL.into_iter().map(dotted).collect::<Vec<String>>().join("/"),
    }
}

fn strip_sidecars(search_dir: &Path) -> Result<(), String> {
    let resolved = search_dir.canonicalize().unwrap_or_else(|_| search_dir.to_path_buf());
    let qa_files = saga_runner::find_files(&resolved, &[], &|name| {
        name.ends_with(".qa") && name.starts_with('.')
    })
    .map_err(refusal)?;

    let mut removed = 0usize;
    let mut not_removed = 0usize;
    for qa_file in &qa_files {
        match std::fs::remove_file(qa_file) {
            Ok(()) => removed += 1,
            Err(io_err) => {
                eprintln!("  FAIL {}: {}", qa_file.display(), io_err);
                not_removed += 1;
            }
        }
    }

    eprintln!(
        "[saga] stripped {} .qa sidecars from {}, {} could not be removed",
        removed, search_dir.display(), not_removed
    );
    Ok(())
}

/// What directory mode does with one source file's sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarStep {
    /// Write the sidecar the source does not have.
    Generate,
    /// Replace the sidecar the source has.
    Refresh,
    /// Leave the sidecar as it is.
    Leave,
}

/// The step for a sidecar in a state: a current sidecar is replaced only under
/// `--force`, every other is written.
fn step_for(state: SidecarState, force: bool) -> SidecarStep {
    match state {
        SidecarState::Missing => SidecarStep::Generate,
        SidecarState::Stale | SidecarState::Unreadable => SidecarStep::Refresh,
        SidecarState::Current if force => SidecarStep::Refresh,
        SidecarState::Current => SidecarStep::Leave,
    }
}

/// What a directory run did, counted per source file.
#[derive(Debug, Default, PartialEq, Eq)]
struct DirectoryTally {
    /// Sidecars written where there was none.
    generated: usize,
    /// Sidecars written in place of one that was there.
    refreshed: usize,
    /// Sidecars left, being current.
    current: usize,
    /// Sidecars that could not be written.
    failed: usize,
    /// Sources an instrument could not fully examine.
    faulted: usize,
}

/// Examine one source and write its sidecar; whether the sidecar was written.
/// A source not fully examined and a sidecar not written are counted here.
fn write_sidecar(file: &Path, project_root: &Path, tally: &mut DirectoryTally) -> bool {
    let report = saga_runner::generate_report(file, Some(project_root));
    if !report.faults.is_empty() {
        tally.faulted += 1;
        for fault in &report.faults {
            eprintln!("  FAULT {}: {}: {}", file.display(), fault.tool, fault.message);
        }
    }
    match saga_runner::save_sidecar(&report) {
        Ok(sidecar) => {
            eprintln!("  {}", sidecar.strip_prefix(project_root).unwrap_or(&sidecar).display());
            true
        }
        Err(failure) => {
            eprintln!("  FAIL {}: {}", file.display(), failure);
            tally.failed += 1;
            false
        }
    }
}

/// Remove the sidecars whose source is gone, printing each that could not be removed;
/// what the sweep did. A tree that cannot be listed whole is a refusal.
fn sweep_orphans(project_root: &Path) -> Result<saga_runner::OrphanSweep, String> {
    let orphans = saga_runner::remove_orphaned_sidecars(project_root).map_err(refusal)?;
    for (sidecar, reason) in &orphans.not_removed {
        eprintln!("  FAIL {}: orphaned, and could not be removed: {}", sidecar.display(), reason);
    }
    Ok(orphans)
}

/// The summary line of an orphan sweep.
fn orphan_summary(orphans: &saga_runner::OrphanSweep) -> String {
    format!(
        "[saga] removed {} orphaned .qa sidecars, {} could not be removed",
        orphans.removed.len(),
        orphans.not_removed.len()
    )
}

/// `--orphans`: the sweep alone, after files were deleted. Nothing is examined. A
/// sidecar that could not be removed fails the run, removing them being its whole job.
fn run_orphans(search_dir: &Path) -> Result<(), String> {
    let project_root = search_dir.canonicalize().unwrap_or_else(|_| search_dir.to_path_buf());
    let orphans = sweep_orphans(&project_root)?;
    let summary = orphan_summary(&orphans);
    if orphans.not_removed.is_empty() {
        eprintln!("{summary}");
        return Ok(());
    }
    Err(summary)
}

fn run_directory(search_dir: &Path, force: bool, wanted: Option<SourceKind>) -> Result<(), String> {
    let project_root = search_dir.canonicalize().unwrap_or_else(|_| search_dir.to_path_buf());

    // Remove orphaned .qa sidecars before generation
    let orphans = sweep_orphans(&project_root)?;
    if !orphans.removed.is_empty() || !orphans.not_removed.is_empty() {
        eprintln!("{}", orphan_summary(&orphans));
    }

    let files = find_source_files(&project_root, wanted)?;

    if files.is_empty() {
        eprintln!("[saga] no {} files found in {}", kind_label(wanted), search_dir.display());
        return Ok(());
    }

    let mut tally = DirectoryTally::default();
    for file in &files {
        match step_for(saga_runner::sidecar_state(file), force) {
            SidecarStep::Leave => tally.current += 1,
            SidecarStep::Generate => tally.generated += usize::from(write_sidecar(file, &project_root, &mut tally)),
            SidecarStep::Refresh => tally.refreshed += usize::from(write_sidecar(file, &project_root, &mut tally)),
        }
    }

    eprintln!(
        "[saga] {} generated, {} refreshed, {} current, {} failed, {} not fully examined (of {} {} files)",
        tally.generated, tally.refreshed, tally.current, tally.failed, tally.faulted, files.len(), kind_label(wanted)
    );
    Ok(())
}

// =============================================================================
// Check mode
// =============================================================================

/// The line `--check` prints for a source: its sidecar's state and its path
/// relative to the directory. A source whose sidecar is current has no line.
fn check_line(state: SidecarState, relative_path: &Path) -> Option<String> {
    match state {
        SidecarState::Current => None,
        SidecarState::Missing | SidecarState::Stale | SidecarState::Unreadable => {
            Some(format!("{}\t{}", state.label(), relative_path.display()))
        }
    }
}

/// How many of the states are the given one.
fn count_of(states: &[SidecarState], counted: SidecarState) -> usize {
    states.iter().filter(|state| **state == counted).count()
}

fn run_check(search_dir: &Path, wanted: Option<SourceKind>) -> Result<(), String> {
    let project_root = search_dir.canonicalize().unwrap_or_else(|_| search_dir.to_path_buf());
    let files = find_source_files(&project_root, wanted)?;

    let mut states = Vec::with_capacity(files.len());
    for file in &files {
        let state = saga_runner::sidecar_state(file);
        if let Some(line) = check_line(state, file.strip_prefix(&project_root).unwrap_or(file)) {
            println!("{line}");
        }
        states.push(state);
    }

    eprintln!(
        "[saga] {} missing, {} stale, {} unreadable, {} current (of {} {} files)",
        count_of(&states, SidecarState::Missing),
        count_of(&states, SidecarState::Stale),
        count_of(&states, SidecarState::Unreadable),
        count_of(&states, SidecarState::Current),
        files.len(),
        kind_label(wanted)
    );
    Ok(())
}

// =============================================================================
// Main
// =============================================================================

/// The content handed over on stdin. A stdin that cannot be read is a refusal.
fn stdin_content() -> Result<String, String> {
    let mut content = String::new();
    std::io::stdin()
        .read_to_string(&mut content)
        .map_err(|io_err| refusal(failure_mode_core::file_unreadable("stdin", io_err)))?;
    Ok(content)
}

/// The refusal of a file that is not there to examine; `None` for a file that is.
fn unreadable_file_refusal(file: &Path) -> Option<String> {
    std::fs::metadata(file)
        .err()
        .map(|io_err| refusal(failure_mode_core::file_unreadable(file.display(), io_err)))
}

fn run_file(args: &Args) -> Result<(), String> {
    let project_root = args.project_dir.as_deref();

    let report = if args.stdin {
        saga_runner::generate_report_from_content(&args.path, &stdin_content()?, project_root)
    } else {
        if let Some(not_there) = unreadable_file_refusal(&args.path) {
            return Err(not_there);
        }
        saga_runner::generate_report(&args.path, project_root)
    };

    if args.sidecar {
        let sidecar = saga_runner::save_sidecar(&report).map_err(refusal)?;
        eprintln!("[saga] wrote {}", sidecar.display());
    }

    println!("{}", report_json(&report, &report.file)?);
    Ok(())
}

/// The report of a file as JSON text. A report that cannot be written as JSON is
/// refused as `sanity_report_invalid`.
fn report_json<Report: serde::Serialize>(report: &Report, file: &str) -> Result<String, String> {
    serde_json::to_string_pretty(report)
        .map_err(|json_err| refusal(failure_mode_core::sanity_report_invalid(file, json_err)))
}

fn main() {
    let args = Args::parse();

    let path_is_directory = args.path.is_dir();
    if let Some(usage) = directory_flag_without_directory(args.strip, args.check, args.orphans, path_is_directory) {
        eprintln!("{usage}");
        process::exit(2);
    }

    // Set version override before any processing
    if let Some(test_version) = args.test {
        let override_value = match test_version {
            TestVersion::V1 => saga_runner::VersionOverride::ForceV1,
            TestVersion::V2 => saga_runner::VersionOverride::ForceV2,
        };
        saga_runner::set_version_override(override_value);
        eprintln!("[saga] version override: {:?}", override_value);
    }

    let result = if args.strip {
        strip_sidecars(&args.path)
    } else if args.orphans {
        run_orphans(&args.path)
    } else if args.check {
        run_check(&args.path, wanted_kind(args.kind))
    } else if path_is_directory {
        run_directory(&args.path, args.force, wanted_kind(args.kind))
    } else {
        run_file(&args)
    };

    match result {
        Ok(()) => process::exit(0),
        Err(message) => {
            eprintln!("{}", message);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_is_not_there_is_refused_with_its_failure_class_on_the_first_line() {
        let missing = Path::new("/nonexistent/saga_cli/missing.rs");
        let message = unreadable_file_refusal(missing).unwrap_or_default();
        let mut lines = message.lines();
        assert_eq!(lines.next(), Some("read_error"));
        assert!(lines.next().is_some_and(|line| line.starts_with("Cannot read '/nonexistent/saga_cli/missing.rs'")));
    }

    #[test]
    fn a_file_that_is_there_is_not_refused() {
        assert_eq!(unreadable_file_refusal(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml").as_path()), None);
    }

    #[test]
    fn a_refusal_opens_with_the_failure_class_and_carries_the_message_after_it() {
        let message = refusal(failure_mode_core::file_unreadable("stdin", "stream did not contain valid UTF-8"));
        assert_eq!(message.lines().next(), Some("read_error"));
        assert!(message.contains("Cannot read 'stdin': stream did not contain valid UTF-8."), "{message}");
    }

    #[test]
    fn a_report_is_handed_over_as_json() {
        let report = saga_runner::generate_report(Path::new("/nonexistent/saga_cli/missing.rs"), None);
        let json = report_json(&report, &report.file).unwrap_or_default();
        assert!(json.contains("\"file\": \"/nonexistent/saga_cli/missing.rs\""), "{json}");
    }

    #[test]
    fn a_report_that_cannot_be_written_as_json_is_refused_with_its_failure_class_on_the_first_line() {
        let keyed_by_a_pair = std::collections::HashMap::from([((1, 2), 3)]);
        let message = report_json(&keyed_by_a_pair, "/project/a.py").unwrap_err();
        let mut lines = message.lines();
        assert_eq!(lines.next(), Some("validation_error"));
        assert!(
            lines.next().is_some_and(|line| line.starts_with("The report for '/project/a.py' does not match sanity-report")),
            "{message}"
        );
    }

    #[test]
    fn a_source_of_the_wanted_kind_is_wanted() {
        assert!(is_wanted_source("lib.rs", Some(SourceKind::Rust)));
        assert!(is_wanted_source("a.py", None));
        assert!(is_wanted_source("App.svelte", None));
    }

    #[test]
    fn a_file_of_another_kind_or_of_none_is_not_wanted() {
        assert!(!is_wanted_source("a.py", Some(SourceKind::Rust)));
        assert!(!is_wanted_source("notes.txt", None));
        assert!(!is_wanted_source(".a.py.qa", None));
    }

    #[test]
    fn the_kind_label_names_the_wanted_extension_or_every_one() {
        assert_eq!(kind_label(Some(SourceKind::Python)), ".py");
        assert_eq!(kind_label(None), ".py/.rs/.svelte");
    }

    #[test]
    fn every_sidecar_that_is_not_current_is_written() {
        assert_eq!(step_for(SidecarState::Missing, false), SidecarStep::Generate);
        assert_eq!(step_for(SidecarState::Stale, false), SidecarStep::Refresh);
        assert_eq!(step_for(SidecarState::Unreadable, false), SidecarStep::Refresh);
        assert_eq!(step_for(SidecarState::Current, true), SidecarStep::Refresh);
    }

    #[test]
    fn a_current_sidecar_is_left_without_force() {
        assert_eq!(step_for(SidecarState::Current, false), SidecarStep::Leave);
    }

    #[test]
    fn check_prints_the_state_and_the_relative_path_of_a_sidecar_that_is_not_current() {
        assert_eq!(check_line(SidecarState::Stale, Path::new("src/a.py")).as_deref(), Some("stale\tsrc/a.py"));
        assert_eq!(check_line(SidecarState::Missing, Path::new("b.rs")).as_deref(), Some("missing\tb.rs"));
        assert_eq!(check_line(SidecarState::Unreadable, Path::new("c.rs")).as_deref(), Some("unreadable\tc.rs"));
    }

    #[test]
    fn check_prints_nothing_for_a_current_sidecar() {
        assert_eq!(check_line(SidecarState::Current, Path::new("src/a.py")), None);
    }

    #[test]
    fn a_directory_only_flag_on_a_file_is_wrong_arguments() {
        assert!(directory_flag_without_directory(true, false, false, false).is_some_and(|usage| usage.contains("--strip")));
        assert!(directory_flag_without_directory(false, true, false, false).is_some_and(|usage| usage.contains("--check")));
        assert!(directory_flag_without_directory(false, false, true, false).is_some_and(|usage| usage.contains("--orphans")));
    }

    #[test]
    fn a_directory_only_flag_on_a_directory_and_no_flag_on_a_file_are_accepted() {
        assert_eq!(directory_flag_without_directory(true, false, false, true), None);
        assert_eq!(directory_flag_without_directory(false, true, false, true), None);
        assert_eq!(directory_flag_without_directory(false, false, true, true), None);
        assert_eq!(directory_flag_without_directory(false, false, false, false), None);
    }

    #[test]
    fn orphans_is_refused_together_with_any_other_directory_mode() {
        for other in ["--force", "--strip", "--check"] {
            assert!(Args::try_parse_from(["saga", ".", "--orphans", other]).is_err(), "{other}");
        }
    }

    #[test]
    fn orphans_alone_is_accepted() {
        assert!(Args::try_parse_from(["saga", ".", "--orphans"]).is_ok_and(|args| args.orphans));
    }

    /// A tree with a source and its sidecar, a source with none, and a sidecar whose source is gone.
    fn tree_after_a_delete(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("saga_cli_{test_name}_{}", process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        for file in ["src/kept.py", "src/.kept.py.qa", "src/new.py", "src/.deleted.py.qa"] {
            std::fs::write(dir.join(file), "").unwrap();
        }
        dir.canonicalize().unwrap()
    }

    #[test]
    fn orphans_removes_the_sidecar_of_a_deleted_source_and_nothing_else() {
        let dir = tree_after_a_delete("orphans_only");
        let outcome = run_orphans(&dir);
        let deleted_gone = !dir.join("src/.deleted.py.qa").exists();
        let kept_stays = dir.join("src/.kept.py.qa").exists();
        let new_untouched = !dir.join("src/.new.py.qa").exists();
        let _ = std::fs::remove_dir_all(&dir);

        assert!(outcome.is_ok(), "{outcome:?}");
        assert!(deleted_gone, "the orphan is removed");
        assert!(kept_stays, "a sidecar whose source is there is left");
        assert!(new_untouched, "nothing is examined, so no sidecar is written");
    }

    #[test]
    fn check_is_refused_together_with_force_or_strip() {
        assert!(Args::try_parse_from(["saga", ".", "--check", "--force"]).is_err());
        assert!(Args::try_parse_from(["saga", ".", "--check", "--strip"]).is_err());
    }

    #[test]
    fn check_alone_is_accepted() {
        assert!(Args::try_parse_from(["saga", ".", "--check"]).is_ok_and(|args| args.check));
    }

    #[test]
    fn the_states_are_counted_one_by_one() {
        let states = [SidecarState::Stale, SidecarState::Current, SidecarState::Stale];
        assert_eq!(count_of(&states, SidecarState::Stale), 2);
        assert_eq!(count_of(&states, SidecarState::Missing), 0);
    }
}
