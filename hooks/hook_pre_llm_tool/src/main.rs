//! PreToolUse hook: what the file tools of an interactive session may reach.
//!
//! Every path a call reaches is matched against the protected path list
//! (`hook_io`'s `protected_paths.toml`): the floor is always blocked, and
//! probing — reading the security infrastructure — is met at the severity given
//! on the command line. Subagents have their own hook, hook_pre_subagent_tool.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_pre_llm_tool --probing warn
//!
//! Env var (set by cc_launch at launch, persona-bound; setting it in-session
//! is itself detected as subversion by hook_pre_llm_bash):
//!     HOOK_LLM_ALLOW_PATHS=/path1:/path2  — directories exempt from probing
//! Nothing exempts the floor.

use std::process::ExitCode;

use hook_decision_core::flags::parse_category_flags;
use hook_decision_core::paths::{is_under, normalize};
use hook_decision_core::rules::{decide_category, first_match, path_subject, rules_of, RuleSet};
use hook_decision_core::{ask_because_undecided, make_decision, DecisionInput, HookDecision, Severity};
use hook_io::{HookInput, WhenUndecided, FLOOR, PROBING};

const HOOK: &str = "hook_pre_llm_tool";

fn main() -> ExitCode {
    hook_io::run_pre_hook(WhenUndecided::AskUser, decide)
}

// ── Config ─────────────────────────────────────────────────────────

#[derive(Debug)]
struct Config {
    /// The severity of probing; `None` when the flag is not given.
    probing: Option<Severity>,
    /// Directories exempt from probing, normalised.
    allow_paths: Vec<String>,
}

fn read_config(args: &[String], allow_paths_env: &str) -> Result<Config, String> {
    let severities = parse_category_flags(args, &[PROBING])?;
    Ok(Config {
        probing: severities.get(PROBING).copied(),
        allow_paths: allow_paths_env
            .split(':')
            .filter(|path| !path.is_empty())
            .map(normalize)
            .collect(),
    })
}

// ── Decision logic ─────────────────────────────────────────────────

fn decide(input: &HookInput) -> HookDecision {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let allow_paths_env = std::env::var("HOOK_LLM_ALLOW_PATHS").unwrap_or_default();
    let config = match read_config(&args, &allow_paths_env) {
        Ok(config) => config,
        Err(cause) => return ask_because_undecided(HOOK, &cause),
    };
    match hook_io::protected_paths() {
        Ok(protected) => decide_inner(input, &config, &protected),
        Err(cause) => ask_because_undecided(HOOK, &cause),
    }
}

/// The decision over every path the call reaches: the floor first, for all of
/// them, then probing for those no grant exempts.
fn decide_inner(input: &HookInput, config: &Config, protected: &RuleSet) -> HookDecision {
    let targets = input.targets();

    if let Some(denied) = targets.iter().find_map(|target| floor_decision(target, protected)) {
        return denied;
    }

    let Some(probing_severity) = config.probing else {
        return HookDecision::Allow;
    };
    targets
        .iter()
        .filter(|target| !is_exempt(target, &config.allow_paths))
        .find_map(|target| {
            decide_category(rules_of(protected, PROBING), &path_subject(target), PROBING, probing_severity)
        })
        .unwrap_or(HookDecision::Allow)
}

fn floor_decision(target: &str, protected: &RuleSet) -> Option<HookDecision> {
    first_match(rules_of(protected, FLOOR), target).map(|rule| {
        make_decision(&DecisionInput {
            severity: Severity::Block,
            category: FLOOR,
            description: &rule.description,
            subject: target,
            verb_past: "accessed",
            verb_present: "access",
            max_subject_chars: None,
        })
    })
}

fn is_exempt(target: &str, allow_paths: &[String]) -> bool {
    allow_paths.iter().any(|allowed| is_under(target, allowed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protected() -> RuleSet {
        hook_io::protected_paths().unwrap()
    }

    fn config(probing: Option<Severity>, allow_paths: &[&str]) -> Config {
        Config { probing, allow_paths: allow_paths.iter().map(|path| normalize(path)).collect() }
    }

    fn blocking() -> Config {
        config(Some(Severity::Block), &[])
    }

    fn read(path: &str) -> HookInput {
        HookInput::for_tool("Read", serde_json::json!({ "file_path": path }))
    }

    fn decided(input: &HookInput, config: &Config) -> HookDecision {
        decide_inner(input, config, &protected())
    }

    fn category_of(decision: &HookDecision) -> &str {
        match decision {
            HookDecision::Deny { category, .. }
            | HookDecision::Ask { category, .. }
            | HookDecision::Warn { category, .. } => category,
            _ => "",
        }
    }

    // ── read_config ──────────────────────────────────────────────

    fn args(written: &[&str]) -> Vec<String> {
        written.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn the_probing_flag_is_read() {
        let config = read_config(&args(&["--probing", "warn"]), "").unwrap();
        assert_eq!(config.probing, Some(Severity::Warn));
    }

    #[test]
    fn no_flag_means_probing_is_not_examined() {
        assert_eq!(read_config(&[], "").unwrap().probing, None);
    }

    #[test]
    fn a_misspelled_severity_is_refused() {
        assert!(read_config(&args(&["--probing", "wran"]), "").is_err());
    }

    #[test]
    fn a_flag_this_hook_does_not_read_is_refused() {
        assert!(read_config(&args(&["--gaming", "warn"]), "").is_err());
    }

    #[test]
    fn allowed_paths_are_split_and_normalised() {
        let config = read_config(&[], "/a/b/:/c/../d/").unwrap();
        assert_eq!(config.allow_paths, vec!["/a/b/", "/d/"]);
    }

    // ── Floor: always denied ─────────────────────────────────────

    #[test]
    fn every_credential_store_is_denied_with_no_flag_given() {
        let stores = [
            "/home/user/.ssh/id_rsa",
            "/home/user/.aws/credentials",
            "/home/user/.gnupg/secring.gpg",
            "/home/user/.kube/config",
            "/home/user/.docker/config.json",
            "/home/user/.netrc",
        ];
        for store in stores {
            let decision = decided(&read(store), &config(None, &[]));
            assert!(matches!(decision, HookDecision::Deny { .. }), "{store}");
            assert_eq!(category_of(&decision), FLOOR, "{store}");
        }
    }

    #[test]
    fn a_grant_never_exempts_the_floor() {
        let granted = config(Some(Severity::Block), &["/home/user/.ssh/", "/home/user/"]);
        assert_eq!(category_of(&decided(&read("/home/user/.ssh/id_rsa"), &granted)), FLOOR);
    }

    #[test]
    fn a_path_spelled_around_the_floor_is_denied() {
        let decision = decided(&read("/home/user/project/../.ssh/id_rsa"), &config(None, &[]));
        assert_eq!(category_of(&decision), FLOOR);
    }

    #[test]
    fn a_glob_with_an_absolute_pattern_and_no_path_is_denied() {
        let glob = HookInput::for_tool("Glob", serde_json::json!({ "pattern": "/home/user/.ssh/*" }));
        assert_eq!(category_of(&decided(&glob, &config(None, &[]))), FLOOR);
    }

    #[test]
    fn a_glob_pattern_into_the_floor_under_a_plain_path_is_denied() {
        let glob = HookInput::for_tool("Glob", serde_json::json!({ "path": "/home/user", "pattern": ".aws/*" }));
        assert_eq!(category_of(&decided(&glob, &config(None, &[]))), FLOOR);
    }

    #[test]
    fn a_grep_glob_into_the_floor_is_denied() {
        let grep = HookInput::for_tool(
            "Grep",
            serde_json::json!({ "pattern": "key", "path": "/home/user", "glob": ".ssh/*" }),
        );
        assert_eq!(category_of(&decided(&grep, &config(None, &[]))), FLOOR);
    }

    #[test]
    fn a_notebook_on_the_floor_is_denied() {
        let edit = HookInput::for_tool("NotebookEdit", serde_json::json!({ "notebook_path": "/u/.aws/n.ipynb" }));
        assert_eq!(category_of(&decided(&edit, &config(None, &[]))), FLOOR);
    }

    #[test]
    fn a_search_run_from_inside_the_floor_is_denied() {
        let grep = HookInput {
            tool_name: "Grep".to_string(),
            tool_input: serde_json::json!({ "pattern": "BEGIN" }),
            cwd: Some("/home/user/.ssh/keys".to_string()),
        };
        assert_eq!(category_of(&decided(&grep, &config(None, &[]))), FLOOR);
    }

    #[test]
    fn a_grep_for_text_that_names_the_floor_is_allowed() {
        let grep = HookInput::for_tool("Grep", serde_json::json!({ "pattern": "/.ssh/", "path": "/home/user/docs" }));
        assert!(matches!(decided(&grep, &blocking()), HookDecision::Allow));
    }

    #[test]
    fn a_name_that_only_resembles_a_store_is_allowed() {
        for near in ["/home/user/ssh/notes.md", "/home/user/.sshx/id", "/home/user/aws/readme.md"] {
            assert!(matches!(decided(&read(near), &blocking()), HookDecision::Allow), "{near}");
        }
    }

    // ── Probing ──────────────────────────────────────────────────

    #[test]
    fn probing_at_block_denies() {
        let decision = decided(&read("/home/user/.claude/hooks/pre_tool"), &blocking());
        assert!(matches!(decision, HookDecision::Deny { .. }));
        assert_eq!(category_of(&decision), PROBING);
    }

    #[test]
    fn probing_at_warn_warns() {
        let decision = decided(&read("/n/hooks/hook_pre_subagent_bash/src/main.rs"), &config(Some(Severity::Warn), &[]));
        assert!(matches!(decision, HookDecision::Warn { .. }));
    }

    #[test]
    fn probing_without_its_flag_is_allowed() {
        assert!(matches!(decided(&read("/home/user/.claude/hooks/x"), &config(None, &[])), HookDecision::Allow));
    }

    #[test]
    fn the_settings_rule_asks_at_any_category_severity() {
        let decision = decided(&read("/home/user/.claude/settings.json"), &blocking());
        assert!(matches!(decision, HookDecision::Ask { .. }));
    }

    #[test]
    fn the_grant_file_is_watched() {
        let decision = decided(&read("/n/cli/cc_launch/src/permissions.rs"), &blocking());
        assert_eq!(category_of(&decision), PROBING);
    }

    #[test]
    fn a_granted_directory_is_exempt_from_probing() {
        let granted = config(Some(Severity::Block), &["/n/"]);
        assert!(matches!(decided(&read("/n/core/gleipnir_core/src/lib.rs"), &granted), HookDecision::Allow));
    }

    #[test]
    fn a_path_that_climbs_out_of_a_granted_directory_is_not_exempt() {
        let granted = config(Some(Severity::Block), &["/a/tools/"]);
        let decision = decided(&read("/a/tools/../nornir/core/gleipnir_core/src/lib.rs"), &granted);
        assert_eq!(category_of(&decision), PROBING);
    }

    #[test]
    fn a_sibling_of_a_granted_directory_is_not_exempt() {
        let granted = config(Some(Severity::Block), &["/a/tools"]);
        let decision = decided(&read("/a/tools_other/hook_io/src/lib.rs"), &granted);
        assert_eq!(category_of(&decision), PROBING);
    }

    #[test]
    fn one_exempt_path_does_not_exempt_another_in_the_same_call() {
        let granted = config(Some(Severity::Block), &["/n/"]);
        let grep = HookInput::for_tool(
            "Grep",
            serde_json::json!({ "pattern": "x", "path": "/n/core", "glob": "../../other/hook_io/*" }),
        );
        assert_eq!(category_of(&decided(&grep, &granted)), PROBING);
    }

    // ── Allow ────────────────────────────────────────────────────

    #[test]
    fn an_ordinary_file_is_allowed() {
        assert!(matches!(decided(&read("/home/user/project/src/main.rs"), &blocking()), HookDecision::Allow));
    }

    #[test]
    fn a_call_that_reaches_no_path_is_allowed() {
        let empty = HookInput::for_tool("Read", serde_json::json!({}));
        assert!(matches!(decided(&empty, &blocking()), HookDecision::Allow));
    }
}
