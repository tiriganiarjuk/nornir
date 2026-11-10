//! dep_resolve_function — one Python function's dependency closure, and its digest.
//!
//! Identity for a cache: the named top-level definition, the same-file
//! definitions it references, the imported symbols it references, and so on
//! transitively. Output is in canonical order (dependencies first, ties by
//! `file::name`, the target last), so the same tree always gives the same
//! bytes, and `--digest` is the sha256 of those bytes.
//!
//! Exit codes: 0 clean; 1 with findings standing (the closure is still
//! printed, but `--digest` prints no digest — an unclean closure cannot be
//! cached); 2 usage, including a name that is not a top-level definition.
//!
//! Usage:
//!     dep_resolve_function <file> <name> [--mode signatures|hybrid|full] [--max-depth N]
//!                          [--project-root <path>] [--simulate] [--digest]

use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;

use dep_resolve_core::resolve::{resolve_function, ResolveConfig, Target};
use dep_resolve_core::{digest, render, Closure, ResolveError, ResolveMode};

#[derive(Parser)]
#[command(name = "dep_resolve_function", about = "One Python function's dependency closure, and its digest")]
struct Args {
    /// Python file holding the definition.
    file: PathBuf,

    /// The top-level function or class name.
    name: String,

    /// What to render per symbol: "signatures", "hybrid" or "full".
    #[arg(long, default_value = "full")]
    mode: String,

    /// Project root directory (auto-detected from pyproject.toml if omitted).
    #[arg(long)]
    project_root: Option<PathBuf>,

    /// Reference hops followed from the definition (which is hop 0).
    #[arg(long, default_value = "10")]
    max_depth: usize,

    /// Show the closure and the file content with banners.
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
            eprintln!("dep_resolve_function: {message}");
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
    let closure = resolve_function(&target, &args.name, |path| std::fs::read(path).ok(), &config)
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
