//! dead_code_find — the top-level functions and classes nothing reaches, in every
//! Python project under a directory.
//!
//! One line per dead definition, `path:line  name  (kind)`, its path relative to
//! the directory given: ready to hand to whoever wrote it. What counts as alive is
//! `dep_resolve_core::liveness`; how a project is found is `dead_code_io`.
//!
//! Exit codes: 0 the directory was examined, whatever was found; 1 a failure;
//! 2 wrong arguments.
//!
//! Usage:
//!     dead_code_find [<dir>]

use std::path::PathBuf;
use std::process;

use clap::Parser;

#[derive(Parser)]
#[command(name = "dead_code_find", about = "The functions and classes nothing reaches, in every Python project under a directory")]
struct Args {
    /// Directory to examine (defaults to the current directory).
    #[arg(default_value = ".")]
    directory: PathBuf,
}

fn main() {
    let args = Args::parse();
    let projects = match dead_code_io::find_dead_code(&args.directory) {
        Ok(projects) => projects,
        Err(failure) => {
            eprintln!("{}\n{failure}", failure.class);
            process::exit(1);
        }
    };
    if projects.is_empty() {
        println!("[dead code] no project (pyproject.toml) at or under {}", args.directory.display());
        return;
    }
    let report = dead_code_io::render_dead_code(&projects, &args.directory);
    if report.is_empty() {
        println!("[dead code] none in {} project(s) under {}", projects.len(), args.directory.display());
    } else {
        print!("{report}");
    }
}
