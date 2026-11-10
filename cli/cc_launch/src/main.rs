mod assembly;
mod library;
mod model;
mod permissions;
mod profiles;
mod tui;

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;

use model::{AppState, TuiOutcome};

#[derive(Parser)]
#[command(
    name = "cc_launch",
    about = "TUI launcher for Claude Code with composable prompt library"
)]
struct Cli {
    /// Update system prompt for an existing session (no launch)
    #[arg(long)]
    update: bool,

    /// Target session ID (with --update)
    #[arg(long, requires = "update")]
    session: Option<String>,

    /// Target workspace path (with --update); resolves to latest session
    #[arg(long, requires = "update")]
    workspace: Option<PathBuf>,

    /// Phase 1 of the shell-integrated launch: run the TUI, assemble the
    /// prompt, write the launch spec — but do NOT launch. Exit 0 means a spec
    /// is ready; the shell function then cds to the workspace and runs --go.
    #[arg(long, conflicts_with_all = ["go", "spec_workspace"])]
    pick: bool,

    /// Phase 2: consume the launch spec and exec claude in the current
    /// directory (the shell has already cd'd to the workspace).
    #[arg(long, conflicts_with_all = ["pick", "spec_workspace"])]
    go: bool,

    /// Print the workspace path from the pending launch spec (for the shell
    /// function's cd step). Prints nothing if the spec has no workspace.
    #[arg(long, conflicts_with_all = ["pick", "go"])]
    spec_workspace: bool,

    /// Flags passed through to claude (e.g., --continue, --resume)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    claude_args: Vec<String>,
}

/// Exit code from --pick meaning "nothing to launch" (quit, or update
/// completed). The shell function stops silently on any nonzero exit.
const EXIT_NO_LAUNCH: i32 = 3;

fn main() {
    let arguments = Cli::parse();
    if let Err(error) = run(arguments) {
        eprintln!("cc_launch: {error}");
        process::exit(1);
    }
}

fn run(arguments: Cli) -> Result<(), String> {
    // Spec-only modes need no library scan and no TUI.
    if arguments.spec_workspace {
        return print_spec_workspace();
    }
    if arguments.go {
        return exec_from_spec();
    }

    let ai_home = write_engine::ai_home();
    let library_path = ai_home.join("control/library");
    let profiles_path = ai_home.join("control/cc_launch_profiles.toml");

    let update_mode = arguments.update;
    let update_session = arguments.session;
    let update_workspace = arguments.workspace;
    let pick_mode = arguments.pick;

    let library = library::scan_library(&library_path)?;
    let workspace_profiles = profiles::load_profiles(&profiles_path)?;
    // Where the launcher was invoked. With no profile selected this IS the
    // workspace — without it, "Auto" resolves to no directory at all and every
    // path-dependent behaviour silently no-ops.
    let launch_directory = std::env::current_dir()
        .map_err(|error| format!("get current directory: {error}"))?;

    let mut state = AppState::new(
        library,
        workspace_profiles,
        arguments.claude_args,
        update_mode,
        launch_directory,
    );

    match tui::run_tui(&mut state)? {
        TuiOutcome::Launch => {
            if update_mode {
                run_update(&state, &update_session, &update_workspace)?;
                if pick_mode {
                    process::exit(EXIT_NO_LAUNCH); // update done, nothing to launch
                }
                Ok(())
            } else {
                let prompt_path = assembly::write_prompt_file(&state)?;
                if pick_mode {
                    assembly::write_launch_spec(&state, &prompt_path)?;
                    Ok(()) // exit 0: spec ready — shell cds, then runs --go
                } else {
                    exec_claude(&prompt_path, &state)
                }
            }
        }
        TuiOutcome::Quit => {
            if pick_mode {
                assembly::discard_launch_spec();
                process::exit(EXIT_NO_LAUNCH);
            }
            Ok(())
        }
    }
}

/// Print the workspace path from the pending spec (shell cd step).
/// Prints nothing (still exit 0) when no workspace was selected.
fn print_spec_workspace() -> Result<(), String> {
    let spec_path = assembly::launch_spec_path();
    let raw = std::fs::read_to_string(&spec_path)
        .map_err(|_| "no launch spec found — run `cc_launch --pick` first".to_string())?;
    let table: toml::Table = raw.parse().map_err(|e| format!("parse launch spec: {e}"))?;
    if let Some(workspace) = table.get("workspace_path").and_then(|value| value.as_str()) {
        println!("{workspace}");
    }
    Ok(())
}

/// Consume the launch spec and exec claude in the current directory.
fn exec_from_spec() -> Result<(), String> {
    let spec = assembly::consume_launch_spec()?;

    // Persona-bound permission grant (see permissions.rs).
    if let Some(persona_id) = &spec.persona_id {
        if let Some(allow_paths) = permissions::allow_paths_for_persona(persona_id) {
            std::env::set_var("HOOK_LLM_ALLOW_PATHS", allow_paths);
        }
        if let Some(patterns) = permissions::allow_patterns_for_persona(persona_id) {
            std::env::set_var("HOOK_LLM_ALLOW_PATTERNS", patterns);
        }
    }

    let mut command = std::process::Command::new("claude");
    command
        .arg("--system-prompt-file")
        .arg(&spec.prompt_path)
        .args(&spec.claude_args);

    // The shell has already cd'd; set current_dir anyway so a bare `--go`
    // (run without the shell function) still lands in the right place.
    if let Some(workspace_path) = &spec.workspace_path {
        if workspace_path.is_dir() {
            command.current_dir(workspace_path);
        }
    }

    let error = command.exec();
    Err(format!("exec claude: {error}"))
}

fn run_update(
    state: &AppState,
    session_arg: &Option<String>,
    workspace_arg: &Option<PathBuf>,
) -> Result<(), String> {
    // Resolve workspace name and session ID
    let (workspace_name, session_id) = resolve_update_target(session_arg, workspace_arg)?;

    let target = assembly::write_update_file(state, &session_id, &workspace_name)?;
    eprintln!("Updated: {}", target.display());
    eprintln!("Session: {session_id}");
    eprintln!("Workspace: {workspace_name}");
    eprintln!("Next compact/resume will inject the new prompt.");
    Ok(())
}

/// Resolve the target session for --update.
/// Three modes:
///   --session <id>        → look up workspace from session
///   --workspace <path>    → resolve workspace name, find latest session
///   (neither)             → use cwd to resolve workspace, find latest session
fn resolve_update_target(
    session_arg: &Option<String>,
    workspace_arg: &Option<PathBuf>,
) -> Result<(String, String), String> {
    if let Some(session_id) = session_arg {
        // Explicit session — look up its workspace
        let workspace_name = workspace_registry::workspace_from_session(session_id)?
            .ok_or_else(|| format!("No workspace found for session: {session_id}"))?;
        return Ok((workspace_name, session_id.clone()));
    }

    // Resolve workspace name from --workspace path or cwd
    let resolve_path = match workspace_arg {
        Some(path) => path.to_string_lossy().to_string(),
        None => std::env::current_dir()
            .map_err(|e| format!("get cwd: {e}"))?
            .to_string_lossy()
            .to_string(),
    };

    let workspace_name = workspace_registry::resolve_workspace_from_path(&resolve_path)?
        .ok_or_else(|| format!("No registered workspace for path: {resolve_path}"))?;

    let session_id = workspace_registry::latest_session_for_workspace(&workspace_name)?
        .ok_or_else(|| format!("No sessions found for workspace: {workspace_name}"))?;

    Ok((workspace_name, session_id))
}

fn exec_claude(prompt_path: &Path, state: &AppState) -> Result<(), String> {
    // Set hook exemption env vars if the selected persona carries a permission
    // (persona-bound: Tyr always, everyone else never — see permissions.rs)
    if let Some(allow_paths) = assembly::build_allow_paths(state) {
        std::env::set_var("HOOK_LLM_ALLOW_PATHS", allow_paths);
    }
    if let Some(persona_id) = assembly::selected_persona_id(state) {
        if let Some(patterns) = permissions::allow_patterns_for_persona(persona_id) {
            std::env::set_var("HOOK_LLM_ALLOW_PATTERNS", patterns);
        }
    }

    let mut command = std::process::Command::new("claude");
    command
        .arg("--system-prompt-file")
        .arg(prompt_path)
        .args(&state.passthrough_flags);

    // cd to the workspace before launching (the selected profile's path, or the
    // directory cc_launch was invoked from)
    let workspace_path = state.workspace_path();
    if workspace_path.is_dir() {
        command.current_dir(workspace_path);
    }

    // exec replaces this process — only returns on error
    let error = command.exec();
    Err(format!("exec claude: {error}"))
}
