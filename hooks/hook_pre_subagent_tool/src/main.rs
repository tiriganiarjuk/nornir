//! PreToolUse hook: the file-tool sandbox of a subagent.
//!
//! Usage (in agent frontmatter):
//!     hook_pre_subagent_tool Read=/schemas/,/docs/ Grep=/schemas/ Write=/output/
//!
//! Each argument is Tool=directory1,directory2: a tool the subagent may use,
//! and the directories it may use it in. A call is allowed only when its tool
//! is named and every path it reaches lies under one of that tool's
//! directories. A tool not named, a call that reaches no path, and a path
//! outside are all denied.

use std::collections::BTreeMap;
use std::process::ExitCode;

use hook_decision_core::paths::{is_under, normalize};
use hook_decision_core::{deny_because_undecided, HookDecision};
use hook_io::{HookInput, WhenUndecided};

const HOOK: &str = "hook_pre_subagent_tool";

fn main() -> ExitCode {
    hook_io::run_pre_hook(WhenUndecided::Deny, decide)
}

/// Tool → the directories it may be used in, normalised.
type ToolGrants = BTreeMap<String, Vec<String>>;

/// The command line, read strictly: every argument is `Tool=/dir[,/dir]`, and
/// at least one is given.
fn read_grants(args: &[String]) -> Result<ToolGrants, String> {
    if args.is_empty() {
        return Err("no tool is granted any directory".to_string());
    }
    let mut grants = ToolGrants::new();
    for arg in args {
        let (tool, directories) = tool_grant(arg)?;
        grants.entry(tool).or_default().extend(directories);
    }
    Ok(grants)
}

fn tool_grant(argument: &str) -> Result<(String, Vec<String>), String> {
    let malformed = || format!("{argument} is not Tool=/directory[,/directory]");
    let (tool, written) = argument.split_once('=').ok_or_else(malformed)?;
    let directories: Vec<String> = written.split(',').map(normalize).collect();
    if tool.is_empty() || directories.iter().any(|directory| !directory.starts_with('/')) {
        return Err(malformed());
    }
    Ok((tool.to_string(), directories))
}

fn decide(input: &HookInput) -> HookDecision {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match read_grants(&args) {
        Ok(grants) => decide_with_grants(input, &grants),
        Err(cause) => deny_because_undecided(HOOK, &cause),
    }
}

fn decide_with_grants(input: &HookInput, grants: &ToolGrants) -> HookDecision {
    let tool = input.tool_name.as_str();
    let Some(allowed) = grants.get(tool) else {
        return HookDecision::Deny {
            category: "path".into(),
            event: format!("tool '{tool}' has no path grants"),
            reason: format!("Tool '{tool}' is not in the allowed tool list."),
        };
    };

    if let Some(under_home) = named_under_home(input) {
        return HookDecision::Deny {
            category: "path".into(),
            event: format!("blocked {tool} access to '{under_home}'"),
            reason: format!(
                "'{under_home}' starts with ~, a home directory. Name an absolute path under:\n{}",
                listed(allowed)
            ),
        };
    }

    let targets = input.targets();
    if targets.is_empty() {
        return HookDecision::Deny {
            category: "path".into(),
            event: format!("{tool} call names no path"),
            reason: format!("This {tool} call names no path. Name the path explicitly, under:\n{}", listed(allowed)),
        };
    }

    match targets.iter().find(|target| !allowed.iter().any(|directory| is_under(target, directory))) {
        None => HookDecision::Allow,
        Some(outside) => HookDecision::Deny {
            category: "path".into(),
            event: format!("blocked {tool} access to '{outside}'"),
            reason: format!(
                "'{outside}' is outside {tool}'s allowed paths.\nYou may only access:\n{}\n\
                 Adjust your path to use one of these locations.",
                listed(allowed)
            ),
        },
    }
}

/// A path the call names that starts with `~`: a tool may expand it to a home
/// directory, which no grant here covers. Grep's `pattern` is the text it
/// searches for, so it is read as a path only for Glob.
fn named_under_home(input: &HookInput) -> Option<&str> {
    let search_filter = match input.tool_name.as_str() {
        "Glob" => "pattern",
        _ => "glob",
    };
    ["file_path", "path", "notebook_path", search_filter]
        .iter()
        .filter_map(|key| input.tool_input.get(key).and_then(|value| value.as_str()))
        .find(|named| named.starts_with('~'))
}

fn listed(directories: &[String]) -> String {
    directories.iter().map(|directory| format!("  {directory}")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hook_decision_core::fixtures::{mismatch, mismatch_report, parse_fixture_set, FixtureCall};

    static FIXTURES_TOML: &str = include_str!("../fixtures.toml");

    fn grants() -> ToolGrants {
        let args: Vec<String> = ["Read=/schemas/,/docs/", "Write=/output/", "Grep=/schemas/", "Glob=/schemas"]
            .iter()
            .map(|arg| arg.to_string())
            .collect();
        read_grants(&args).unwrap()
    }

    fn call(tool: &str, cwd: Option<&str>, tool_input: serde_json::Value) -> HookInput {
        HookInput { tool_name: tool.to_string(), tool_input, cwd: cwd.map(str::to_string) }
    }

    fn allowed(input: &HookInput) -> bool {
        matches!(decide_with_grants(input, &grants()), HookDecision::Allow)
    }

    #[test]
    fn every_fixture_gets_its_verdict() {
        let set = parse_fixture_set(FIXTURES_TOML).unwrap();
        let grants = read_grants(&set.arguments).unwrap();
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
                mismatch(fixture, &decide_with_grants(&input, &grants))
            })
            .collect();
        assert!(mismatches.is_empty(), "{}", mismatch_report(&mismatches, set.fixtures.len()));
    }

    // ── read_grants ──────────────────────────────────────────────

    #[test]
    fn grants_are_read_and_merged() {
        let args = vec!["Read=/a/".to_string(), "Read=/b/../c".to_string()];
        assert_eq!(read_grants(&args).unwrap().get("Read"), Some(&vec!["/a/".to_string(), "/c".to_string()]));
    }

    #[test]
    fn no_grants_is_refused() {
        assert!(read_grants(&[]).is_err());
    }

    #[test]
    fn a_malformed_grant_is_refused() {
        for arg in ["Read", "Read=", "=/a", "Read=relative", "Read=/a,,/b", "Read=/a,b"] {
            assert!(read_grants(&[arg.to_string()]).is_err(), "{arg}");
        }
    }

    // ── Calls a fixture file cannot state ────────────────────────

    #[test]
    fn a_path_that_is_null_names_no_path() {
        assert!(!allowed(&call("Read", Some("/schemas"), serde_json::json!({ "file_path": null }))));
    }

    #[test]
    fn a_search_with_no_path_and_no_working_directory_is_denied() {
        assert!(!allowed(&call("Grep", None, serde_json::json!({ "pattern": "x" }))));
    }
}
