//! dep_resolve_file — the symbols a Python file imports, and what those use.
//!
//! Context for a reader: every project-local name the file imports, followed
//! through its own references, transitively. The file's own definitions are
//! not included. Output is in canonical order (dependencies first, ties by
//! `file::name`), so the same tree always gives the same bytes.
//!
//! Exit codes: 0 clean; 1 with findings standing (output is still produced,
//! except that `--digest` prints no digest); 2 usage.
//!
//! Usage:
//!     dep_resolve_file <file> [--mode signatures|hybrid|full] [--max-depth N]
//!                      [--project-root <path>] [--simulate] [--digest]

use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;

use dep_resolve_core::resolve::{resolve_file, ResolveConfig, Target};
use dep_resolve_core::{digest, render, Closure, ResolveError, ResolveMode};

#[derive(Parser)]
#[command(name = "dep_resolve_file", about = "The symbols a Python file imports, and what those use")]
struct Args {
    /// Python file to resolve.
    file: PathBuf,

    /// What to render per symbol: "signatures", "hybrid" or "full".
    #[arg(long, default_value = "signatures")]
    mode: String,

    /// Project root directory (auto-detected from pyproject.toml if omitted).
    #[arg(long)]
    project_root: Option<PathBuf>,

    /// Reference hops followed from the file's imports (which are hop 1).
    #[arg(long, default_value = "10")]
    max_depth: usize,

    /// Show the closure and the file content with banners, as a reader would see them.
    #[arg(long)]
    simulate: bool,

    /// Print only the sha256 of the rendered closure; nothing, and exit 1, if findings stand.
    #[arg(long)]
    digest: bool,
}

/// What the run produced, for main to print and exit with.
struct Outcome {
    stdout: String,
    stderr: String,
    code: i32,
}

fn main() {
    let args = Args::parse();
    match run(&args) {
        Ok(outcome) => {
            print!("{}", outcome.stdout);
            eprint!("{}", outcome.stderr);
            process::exit(outcome.code);
        }
        Err((code, message)) => {
            eprintln!("dep_resolve_file: {message}");
            process::exit(code);
        }
    }
}

fn run(args: &Args) -> Result<Outcome, (i32, String)> {
    let mode = ResolveMode::parse(&args.mode).map_err(|m| (2, m))?;
    let file_path = args
        .file
        .canonicalize()
        .map_err(|err| (1, format!("{}: {err}", args.file.display())))?;
    if !file_path.is_file() {
        return Err((2, format!("{} is not a file", file_path.display())));
    }
    let project_root = match &args.project_root {
        Some(root) => root
            .canonicalize()
            .map_err(|err| (1, format!("{}: {err}", root.display())))?,
        None => detect_project_root(&file_path)
            .ok_or_else(|| (1, "could not detect project root (no pyproject.toml found)".to_string()))?,
    };
    let source = std::fs::read(&file_path)
        .map_err(|err| (1, format!("read {}: {err}", file_path.display())))?;

    let target = Target {
        file_path: &file_path.to_string_lossy(),
        source: &source,
        project_root: &project_root.to_string_lossy(),
    };
    let config = ResolveConfig { mode, max_depth: args.max_depth };
    let closure = resolve_file(&target, |path| std::fs::read(path).ok(), &config)
        .map_err(|err| (exit_code_for(&err), err.to_string()))?;

    Ok(outcome_for(&closure, &source, mode, args.simulate, args.digest))
}

fn outcome_for(closure: &Closure, source: &[u8], mode: ResolveMode, simulate: bool, want_digest: bool) -> Outcome {
    let code = if closure.is_clean() { 0 } else { 1 };
    if want_digest {
        return if closure.is_clean() {
            Outcome { stdout: format!("{}\n", digest::closure_digest(&render::render_symbols(closure))), stderr: String::new(), code }
        } else {
            Outcome { stdout: String::new(), stderr: render::render_findings(closure), code }
        };
    }
    let stdout = if simulate {
        render::render_simulate(closure, std::str::from_utf8(source).unwrap_or(""), mode)
    } else {
        render::render_closure(closure)
    };
    Outcome { stdout, stderr: String::new(), code }
}

fn exit_code_for(error: &ResolveError) -> i32 {
    match error {
        ResolveError::Parse { .. } => 1,
        ResolveError::NotTopLevel { .. } => 2,
    }
}

fn detect_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.parent()?;
    loop {
        if current.join("pyproject.toml").exists() {
            return Some(current.to_path_buf());
        }
        if current.join("setup.py").exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}
