//! PreToolUse hook: dependency context injection for Python file reads.
//!
//! When the main LLM reads a Python file, resolves the symbols the file
//! imports — and what those use, transitively — and injects them as
//! additionalContext BEFORE the Read tool executes. The LLM sees the
//! dependencies first, then the file content from the normal Read result.
//! Dependency findings (cycles, plain project imports, names a module does
//! not define) are injected with the closure, so the reader learns what
//! needs fixing; they never block the Read.
//!
//! Always allows — the Read proceeds normally.
//!
//! Control:
//!   DEP_RESOLVE_DISABLE.lock — disable injection (lock file in control dir)
//!   DEP_RESOLVE_MODE env — "signatures" (default), "hybrid", or "full"
//!   DEP_RESOLVE_MAX_DEPTH env — max transitive depth (default 10)
//!
//! Usage (in ~/.claude/settings.json under PreToolUse):
//!     hook_post_llm_read

use std::path::Path;
use std::process::ExitCode;

use dep_resolve_core::render::render_closure;
use dep_resolve_core::resolve::{resolve_file, ResolveConfig, Target};
use dep_resolve_core::ResolveMode;
use hook_io::{HookDecision, HookInput};

/// What the injected text is for, said once before it.
const PREFACE: &str = "The following are the resolved dependencies of the file being read — the \
symbols it imports and what those use. Read them to understand the interfaces before modifying \
the code. A findings block, if present, names dependency problems in these files that need fixing.";

fn main() -> ExitCode {
    hook_io::run_pre_hook(decide)
}

fn decide(input: &HookInput) -> HookDecision {
    match resolve_context(input) {
        Some(context) => HookDecision::AllowWithContext { context },
        None => HookDecision::Allow,
    }
}

fn resolve_context(input: &HookInput) -> Option<String> {
    if is_disabled() {
        return None;
    }

    let file_path = input.target_path()?;
    if !file_path.ends_with(".py") {
        return None;
    }
    if !Path::new(file_path).is_file() {
        return None;
    }

    let project_root = detect_project_root(Path::new(file_path))?
        .to_string_lossy()
        .to_string();
    let source = std::fs::read(file_path).ok()?;
    let config = build_config();

    let target = Target { file_path, source: &source, project_root: &project_root };
    let closure = resolve_file(&target, |path| std::fs::read(path).ok(), &config).ok()?;

    let rendered = render_closure(&closure);
    if rendered.is_empty() {
        None
    } else {
        Some(format!("{PREFACE}\n\n{rendered}"))
    }
}

fn detect_project_root(start: &Path) -> Option<std::path::PathBuf> {
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

fn is_disabled() -> bool {
    let lock_path = write_engine::ai_home().join("control/DEP_RESOLVE_DISABLE.lock");
    lock_path.exists()
}

fn build_config() -> ResolveConfig {
    let mode = std::env::var("DEP_RESOLVE_MODE")
        .ok()
        .and_then(|value| ResolveMode::parse(&value).ok())
        .unwrap_or(ResolveMode::Signatures);

    let max_depth = std::env::var("DEP_RESOLVE_MAX_DEPTH")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(10);

    ResolveConfig { mode, max_depth }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_python_allows_silently() {
        let input = HookInput {
            tool_name: Some("Read".to_string()),
            tool_input: serde_json::json!({ "file_path": "/tmp/test.rs" }),
        };
        matches!(decide(&input), HookDecision::Allow);
    }

    #[test]
    fn missing_file_allows_silently() {
        let input = HookInput {
            tool_name: Some("Read".to_string()),
            tool_input: serde_json::json!({ "file_path": "/nonexistent/path/file.py" }),
        };
        matches!(decide(&input), HookDecision::Allow);
    }

    #[test]
    fn no_file_path_allows_silently() {
        let input = HookInput {
            tool_name: Some("Read".to_string()),
            tool_input: serde_json::json!({}),
        };
        matches!(decide(&input), HookDecision::Allow);
    }
}
