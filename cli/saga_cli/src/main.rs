//! Saga CLI: quality truth recorder.
//!
//! Runs quality tools on source files (Python, Rust), outputs .qa JSON report.
//!
//! Usage:
//!   saga <file>                                    — run tools, JSON to stdout
//!   saga <file> --sidecar                          — run tools, write .qa sidecar
//!   saga <dir> [--kind py|rs|svelte|all]           — remove orphaned .qa, generate missing
//!   saga <dir> --force [--kind py|rs|svelte|all]   — remove orphaned .qa, regenerate all
//!   saga <dir> --strip                             — remove ALL .qa sidecars recursively
//!   echo "content" | saga --stdin <file>           — run tools on stdin content
//!
//! File vs directory is auto-detected from the path.

use clap::{Parser, ValueEnum};
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

    /// Regenerate even if .qa exists (directory only)
    #[arg(long)]
    force: bool,

    /// Remove ALL .qa sidecars recursively (directory only)
    #[arg(long)]
    strip: bool,

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

// =============================================================================
// Directory mode
// =============================================================================

fn find_source_files(search_dir: &Path, kind: Kind) -> Vec<PathBuf> {
    saga_runner::find_files(search_dir, &[], &|name| match kind {
        Kind::Py => name.ends_with(".py"),
        Kind::Rs => name.ends_with(".rs"),
        Kind::Svelte => name.ends_with(".svelte"),
        Kind::All => name.ends_with(".py") || name.ends_with(".rs") || name.ends_with(".svelte"),
    })
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Py => ".py",
        Kind::Rs => ".rs",
        Kind::Svelte => ".svelte",
        Kind::All => ".py/.rs/.svelte",
    }
}

fn strip_sidecars(search_dir: &Path) -> Result<(), String> {
    let resolved = search_dir.canonicalize().unwrap_or_else(|_| search_dir.to_path_buf());
    let qa_files = saga_runner::find_files(&resolved, &[], &|name| {
        name.ends_with(".qa") && name.starts_with('.')
    });

    let mut removed = 0usize;
    for qa_file in &qa_files {
        if std::fs::remove_file(qa_file).is_ok() {
            removed += 1;
        }
    }

    eprintln!("[saga] stripped {} .qa sidecars from {}", removed, search_dir.display());
    Ok(())
}

fn run_directory(search_dir: &Path, force: bool, kind: Kind) -> Result<(), String> {
    let project_root = search_dir.canonicalize().unwrap_or_else(|_| search_dir.to_path_buf());

    // Remove orphaned .qa sidecars before generation
    let orphans = saga_runner::remove_orphaned_sidecars(&project_root);
    if !orphans.is_empty() {
        eprintln!("[saga] removed {} orphaned .qa sidecars", orphans.len());
    }

    let files = find_source_files(&project_root, kind);

    if files.is_empty() {
        eprintln!("[saga] no {} files found in {}", kind_label(kind), search_dir.display());
        return Ok(());
    }

    let mut generated = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut faulted = 0usize;

    for file in &files {
        let sidecar = saga_runner::qa_path(file);
        if !force && sidecar.exists() {
            skipped += 1;
            continue;
        }

        let report = saga_runner::generate_report(file, Some(&project_root));
        if !report.faults.is_empty() {
            faulted += 1;
            for fault in &report.faults {
                eprintln!("  FAULT {}: {}: {}", file.display(), fault.tool, fault.message);
            }
        }
        match saga_runner::save_sidecar(&report) {
            Ok(path) => {
                let rel = path.strip_prefix(&project_root)
                    .unwrap_or(&path)
                    .display();
                eprintln!("  {}", rel);
                generated += 1;
            }
            Err(e) => {
                eprintln!("  FAIL {}: {}", file.display(), e);
                failed += 1;
            }
        }
    }

    eprintln!(
        "[saga] {} generated, {} skipped, {} failed, {} not fully examined (of {} {} files)",
        generated, skipped, failed, faulted, files.len(), kind_label(kind)
    );
    Ok(())
}

// =============================================================================
// Main
// =============================================================================

fn run_file(args: &Args) -> Result<(), String> {
    let project_root = args.project_dir.as_deref();

    let report = if args.stdin {
        let mut content = String::new();
        std::io::stdin().read_to_string(&mut content)
            .map_err(|e| format!("[saga] failed to read stdin: {}", e))?;
        saga_runner::generate_report_from_content(&args.path, &content, project_root)
    } else {
        if !args.path.exists() {
            return Err(format!("[saga] file not found: {}", args.path.display()));
        }
        saga_runner::generate_report(&args.path, project_root)
    };

    if args.sidecar {
        match saga_runner::save_sidecar(&report) {
            Ok(path) => eprintln!("[saga] wrote {}", path.display()),
            Err(e) => eprintln!("[saga] sidecar write failed: {}", e),
        }
    }

    let json = serde_json::to_string_pretty(&report)
        .map_err(|e| format!("[saga] json serialization failed: {}", e))?;
    println!("{}", json);
    Ok(())
}

fn main() {
    let args = Args::parse();

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
        if !args.path.is_dir() {
            eprintln!("[saga] --strip requires a directory");
            process::exit(2);
        }
        strip_sidecars(&args.path)
    } else if args.path.is_dir() {
        run_directory(&args.path, args.force, args.kind)
    } else {
        run_file(&args)
    };

    match result {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    }
}
