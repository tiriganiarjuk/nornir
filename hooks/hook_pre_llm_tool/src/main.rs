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
use hook_decision_core::rules::{first_match_for_path, rules_of, Rule, RuleSet};
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
    /// The home directory, normalised: a search from it or above it is refused.
    home: String,
}

fn read_config(args: &[String], allow_paths_env: &str, home: &str) -> Result<Config, String> {
    let severities = parse_category_flags(args, &[PROBING])?;
    if home.is_empty() {
        return Err("HOME is not set, so a search from the home directory cannot be told".to_string());
    }
    Ok(Config {
        probing: severities.get(PROBING).copied(),
        allow_paths: allow_paths_env
            .split(':')
            .filter(|path| !path.is_empty())
            .map(normalize)
            .collect(),
        home: normalize(home),
    })
}

// ── Decision logic ─────────────────────────────────────────────────

fn decide(input: &HookInput) -> HookDecision {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let allow_paths_env = std::env::var("HOOK_LLM_ALLOW_PATHS").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let config = match read_config(&args, &allow_paths_env, &home) {
        Ok(config) => config,
        Err(cause) => return ask_because_undecided(HOOK, &cause),
    };
    match hook_io::protected_paths() {
        Ok(protected) => decide_inner(input, &config, &protected),
        Err(cause) => ask_because_undecided(HOOK, &cause),
    }
}

/// The decision over every path the call reaches: the floor first — a search
/// from the home directory or above it, then every path — then probing for
/// the paths no grant exempts.
fn decide_inner(input: &HookInput, config: &Config, protected: &RuleSet) -> HookDecision {
    if let Some(denied) = input.search_root().and_then(|root| search_from_above_home(&root, &config.home)) {
        return denied;
    }

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
            first_match_for_path(rules_of(protected, PROBING), target)
                .map(|rule| path_decision(target, rule, PROBING, rule.severity.unwrap_or(probing_severity)))
        })
        .unwrap_or(HookDecision::Allow)
}

/// A search from the home directory or a directory above it reaches the
/// protected stores under home without naming one of them, so it is the floor's.
fn search_from_above_home(root: &str, home: &str) -> Option<HookDecision> {
    is_under(home, root).then(|| {
        make_decision(&DecisionInput {
            severity: Severity::Block,
            category: FLOOR,
            description: "a search from the home directory or above it, which reaches the protected stores unnamed — search from the project's directory",
            subject: root,
            verb_past: "searched",
            verb_present: "search",
            max_subject_chars: None,
        })
    })
}

fn floor_decision(target: &str, protected: &RuleSet) -> Option<HookDecision> {
    first_match_for_path(rules_of(protected, FLOOR), target)
        .map(|rule| path_decision(target, rule, FLOOR, Severity::Block))
}

/// The decision of a protected-path rule about the path that reached it. The
/// decision shows the path as the call reached it.
fn path_decision(target: &str, rule: &Rule, category: &str, severity: Severity) -> HookDecision {
    make_decision(&DecisionInput {
        severity,
        category,
        description: &rule.description,
        subject: target,
        verb_past: "accessed",
        verb_present: "access",
        max_subject_chars: None,
    })
}

fn is_exempt(target: &str, allow_paths: &[String]) -> bool {
    allow_paths.iter().any(|allowed| is_under(target, allowed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hook_decision_core::fixtures::{mismatch, mismatch_report, parse_fixture_set, FixtureCall};

    static FIXTURES_TOML: &str = include_str!("../fixtures.toml");

    /// The home directory the tests judge under, as the fixture set states it.
    const HOME: &str = "/home/user";

    fn protected() -> RuleSet {
        hook_io::protected_paths().unwrap()
    }

    #[test]
    fn a_session_with_no_home_is_undecided() {
        assert!(read_config(&[], "", "").is_err());
    }

    fn config(probing: Option<Severity>, allow_paths: &[&str]) -> Config {
        Config { probing, allow_paths: allow_paths.iter().map(|path| normalize(path)).collect(), home: HOME.to_string() }
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

    #[test]
    fn every_fixture_gets_its_verdict() {
        let set = parse_fixture_set(FIXTURES_TOML).unwrap();
        let config = read_config(&set.arguments, &set.allow_paths, &set.home).unwrap();
        let protected = protected();
        let mismatches: Vec<String> = set
            .fixtures
            .iter()
            .filter_map(|fixture| {
                let FixtureCall::Tool { tool, tool_input } = &fixture.call else {
                    return Some(format!("{}\n    is not a tool call", fixture.name));
                };
                let input = HookInput {
                    tool_name: tool.clone(),
                    tool_input: serde_json::to_value(tool_input).unwrap(),
                    cwd: Some(fixture.working_directory.clone()),
                };
                mismatch(fixture, &decide_inner(&input, &config, &protected))
            })
            .collect();
        assert!(mismatches.is_empty(), "{}", mismatch_report(&mismatches, set.fixtures.len()));
    }

    // ── read_config ──────────────────────────────────────────────

    fn args(written: &[&str]) -> Vec<String> {
        written.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn the_probing_flag_is_read() {
        let config = read_config(&args(&["--probing", "warn"]), "", HOME).unwrap();
        assert_eq!(config.probing, Some(Severity::Warn));
    }

    #[test]
    fn no_flag_means_probing_is_not_examined() {
        assert_eq!(read_config(&[], "", HOME).unwrap().probing, None);
    }

    #[test]
    fn a_misspelled_severity_is_refused() {
        assert!(read_config(&args(&["--probing", "wran"]), "", HOME).is_err());
    }

    #[test]
    fn a_flag_this_hook_does_not_read_is_refused() {
        assert!(read_config(&args(&["--gaming", "warn"]), "", HOME).is_err());
    }

    #[test]
    fn allowed_paths_are_split_and_normalised() {
        let config = read_config(&[], "/a/b/:/c/../d/", HOME).unwrap();
        assert_eq!(config.allow_paths, vec!["/a/b/", "/d/"]);
    }

    // ── Floor: under another configuration than the fixtures' ────

    #[test]
    fn the_floor_is_denied_with_no_flag_given() {
        let decision = decided(&read("/home/user/.ssh/id_rsa"), &config(None, &[]));
        assert!(matches!(decision, HookDecision::Deny { .. }));
        assert_eq!(category_of(&decision), FLOOR);
    }

    #[test]
    fn a_grant_never_exempts_the_floor() {
        let granted = config(Some(Severity::Block), &["/home/user/.ssh/", "/home/user/"]);
        assert_eq!(category_of(&decided(&read("/home/user/.ssh/id_rsa"), &granted)), FLOOR);
    }

    // ── Probing: under another configuration than the fixtures' ──

    #[test]
    fn probing_at_block_denies() {
        let decision = decided(&read("/home/user/.claude/hooks/pre_tool"), &blocking());
        assert!(matches!(decision, HookDecision::Deny { .. }));
        assert_eq!(category_of(&decision), PROBING);
    }

    #[test]
    fn probing_without_its_flag_is_allowed() {
        assert!(matches!(decided(&read("/home/user/.claude/hooks/x"), &config(None, &[])), HookDecision::Allow));
    }

    #[test]
    fn the_settings_rule_asks_when_probing_blocks() {
        let decision = decided(&read("/home/user/.claude/settings.json"), &blocking());
        assert!(matches!(decision, HookDecision::Ask { .. }));
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
}
