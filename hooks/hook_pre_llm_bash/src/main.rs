//! PreToolUse hook: what the Bash commands of an interactive session may do.
//!
//! Two sources of rules:
//!   rules.toml (this crate) — command patterns by category, and the texts for
//!   each compound command shape
//!   protected_paths.toml (hook_io) — the paths a command may not name (floor),
//!   and those whose naming is noticed (probing)
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_pre_llm_bash --subversion block --truncation warn --evasion warn \
//!         --destruction block --revert ask --workflow ask --chaining block --probing warn
//!
//! A category whose flag is absent is not examined. The floor needs no flag.
//!
//! Env var (set by cc_launch at launch, persona-bound):
//!     HOOK_LLM_ALLOW_PATHS=/path1:/path2  — directories exempt from probing
//!
//! What this hook does with a command is pinned by fixtures.toml beside
//! rules.toml; the test at the end of this file judges every fixture.

use std::process::ExitCode;

use hook_decision_core::flags::{parse_category_flags, CategorySeverities};
use hook_decision_core::paths::{for_matching, is_under, normalize};
use hook_decision_core::rules::{
    command_subject, decide_category, first_match, parse_rule_set, rules_of, PatternKind, RuleSet,
};
use hook_decision_core::shape::{command_shape, CommandShape, CompoundKind};
use hook_decision_core::shell::{named_paths, structure};
use hook_decision_core::{ask_because_undecided, make_decision, DecisionInput, HookDecision, Severity};
use hook_io::{HookInput, WhenUndecided, FLOOR, PROBING};

const HOOK: &str = "hook_pre_llm_bash";
const CHAINING: &str = "chaining";

static RULES_TOML: &str = include_str!("../rules.toml");

/// Which text of a command a category's patterns are matched against.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Reads {
    /// The whole command: text a shell could run is examined wherever it sits,
    /// inside quotes and here-document bodies too.
    Whole,
    /// The command's structure: where its output goes and what it starts.
    /// Text a command merely carries is not examined.
    Structure,
}

/// The pattern categories of rules.toml, in the order they are examined:
/// data loss first.
const PATTERN_CATEGORIES: [(&str, Reads); 6] = [
    ("destruction", Reads::Whole),
    ("subversion", Reads::Whole),
    ("truncation", Reads::Whole),
    ("evasion", Reads::Whole),
    ("revert", Reads::Whole),
    ("workflow", Reads::Structure),
];

fn main() -> ExitCode {
    hook_io::run_pre_hook(WhenUndecided::AskUser, decide)
}

// ── Rules ──────────────────────────────────────────────────────────

/// The rules file: its pattern categories as regexes, and the chaining
/// category, whose rules name a command shape. Every shape must have its rule.
fn parse_rules(toml_str: &str) -> Result<RuleSet, String> {
    let categories: Vec<(&str, PatternKind)> = PATTERN_CATEGORIES
        .iter()
        .map(|(category, _)| (*category, PatternKind::Regex))
        .chain([(CHAINING, PatternKind::Literal)])
        .collect();
    let rules = parse_rule_set(toml_str, &categories)?;
    let chaining = rules_of(&rules, CHAINING);
    if let Some(unknown) = chaining.iter().find(|rule| CompoundKind::ALL.iter().all(|kind| kind.key() != rule.pattern)) {
        return Err(format!("[[chaining]] names a shape this hook does not read: {}", unknown.pattern));
    }
    match CompoundKind::ALL.iter().find(|kind| chaining.iter().all(|rule| rule.pattern != kind.key())) {
        Some(missing) => Err(format!("[[chaining]] has no rule for the shape {}", missing.key())),
        None => Ok(rules),
    }
}

// ── Config ─────────────────────────────────────────────────────────

/// Everything a decision depends on besides the command itself.
struct Config {
    severities: CategorySeverities,
    /// Directories exempt from probing, normalised.
    allow_paths: Vec<String>,
    home: String,
    rules: RuleSet,
    protected: RuleSet,
}

fn flag_categories() -> Vec<&'static str> {
    PATTERN_CATEGORIES.iter().map(|(category, _)| *category).chain([CHAINING, PROBING]).collect()
}

fn read_config(args: &[String], allow_paths_env: &str, home: &str) -> Result<Config, String> {
    Ok(Config {
        severities: parse_category_flags(args, &flag_categories())?,
        allow_paths: allow_paths_env.split(':').filter(|path| !path.is_empty()).map(normalize).collect(),
        home: home.to_string(),
        rules: parse_rules(RULES_TOML)?,
        protected: hook_io::protected_paths()?,
    })
}

// ── Decision logic ─────────────────────────────────────────────────

fn decide(input: &HookInput) -> HookDecision {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let allow_paths_env = std::env::var("HOOK_LLM_ALLOW_PATHS").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    match read_config(&args, &allow_paths_env, &home) {
        Ok(config) => decide_inner(input, &config),
        Err(cause) => ask_because_undecided(HOOK, &cause),
    }
}

/// The decision about one command, in order: the floor; a compound command
/// refused; the pattern categories; a compound command warned of; probing.
fn decide_inner(input: &HookInput, config: &Config) -> HookDecision {
    let command = input.command();
    if command.trim().is_empty() {
        return HookDecision::Allow;
    }
    let named = named_paths(command, input.cwd.as_deref().unwrap_or("/"), &config.home);
    let shape = command_shape(command);
    let chaining = chaining_decision(command, &shape, config);
    let (refusal, warning) = match chaining {
        Some(warned @ HookDecision::Warn { .. }) => (None, Some(warned)),
        other => (other, None),
    };

    floor_decision(command, &named, &config.protected)
        .or(refusal)
        .or_else(|| pattern_decision(command, &shape, config))
        .or(warning)
        .or_else(|| probing_decision(command, &named, config))
        .unwrap_or(HookDecision::Allow)
}

fn floor_decision(command: &str, named: &[String], protected: &RuleSet) -> Option<HookDecision> {
    let floor = rules_of(protected, FLOOR);
    named.iter().find_map(|path| first_match(floor, &for_matching(path))).map(|rule| {
        make_decision(&DecisionInput {
            severity: Severity::Block,
            category: FLOOR,
            description: &rule.description,
            subject: command,
            verb_past: "ran",
            verb_present: "run",
            max_subject_chars: Some(60),
        })
    })
}

/// The decision about a compound command, at the severity of the chaining
/// flag or of the shape's own rule. A compound command is never put to the
/// user to approve: where the severity is ask, it is refused.
fn chaining_decision(command: &str, shape: &CommandShape, config: &Config) -> Option<HookDecision> {
    let CommandShape::Compound(kind) = shape else {
        return None;
    };
    let category_severity = *config.severities.get(CHAINING)?;
    let rule = rules_of(&config.rules, CHAINING).iter().find(|rule| rule.pattern == kind.key())?;
    let severity = match rule.severity.unwrap_or(category_severity) {
        Severity::Ask | Severity::Block => Severity::Block,
        Severity::Warn => Severity::Warn,
    };
    Some(make_decision(&DecisionInput {
        severity,
        category: CHAINING,
        description: &rule.description,
        subject: command,
        verb_past: "ran",
        verb_present: "run",
        max_subject_chars: Some(60),
    }))
}

/// The decision of the pattern categories. They read the command itself: for
/// a command in a directory, the command after its `cd`.
fn pattern_decision(command: &str, shape: &CommandShape, config: &Config) -> Option<HookDecision> {
    let examined = match shape {
        CommandShape::InDirectory { command: inner, .. } => inner.as_str(),
        CommandShape::Single | CommandShape::Compound(_) => command,
    };
    let examined_structure = structure(examined);
    let decision = PATTERN_CATEGORIES.iter().find_map(|(category, reads)| {
        let severity = *config.severities.get(*category)?;
        let read = match reads {
            Reads::Whole => examined,
            Reads::Structure => examined_structure.as_str(),
        };
        decide_category(rules_of(&config.rules, category), &command_subject(read), category, severity)
    })?;
    Some(match shape {
        CommandShape::InDirectory { directory, .. } => in_directory(decision, directory),
        CommandShape::Compound(_) => refused_where_asked(decision),
        CommandShape::Single => decision,
    })
}

/// The decision with the directory the command runs in stated beside it, so
/// what the user reads is the command and where, never a line to take apart.
fn in_directory(decision: HookDecision, directory: &str) -> HookDecision {
    let stated = |text: String| format!("{text}\nIn directory: {directory}");
    match decision {
        HookDecision::Warn { category, event, user_reason, llm_context } => {
            HookDecision::Warn { category, event, user_reason: stated(user_reason), llm_context }
        }
        HookDecision::Ask { category, event, reason, llm_context } => {
            HookDecision::Ask { category, event, reason: stated(reason), llm_context }
        }
        HookDecision::Deny { category, event, reason } => HookDecision::Deny { category, event, reason: stated(reason) },
        allowed => allowed,
    }
}

/// A compound command that a rule would put to the user is refused instead.
fn refused_where_asked(decision: HookDecision) -> HookDecision {
    match decision {
        HookDecision::Ask { category, event, reason, .. } => HookDecision::Deny {
            category,
            event,
            reason: format!(
                "{reason}\nThis call joins several commands, so it is refused and not put up for approval. \
                 Run the command as its own call."
            ),
        },
        other => other,
    }
}

fn probing_decision(command: &str, named: &[String], config: &Config) -> Option<HookDecision> {
    let severity = *config.severities.get(PROBING)?;
    let probing = rules_of(&config.protected, PROBING);
    named
        .iter()
        .filter(|path| !config.allow_paths.iter().any(|allowed| is_under(path, allowed)))
        .find_map(|path| first_match(probing, &for_matching(path)))
        .map(|rule| {
            make_decision(&DecisionInput {
                severity: rule.severity.unwrap_or(severity),
                category: PROBING,
                description: &rule.description,
                subject: command,
                verb_past: "ran",
                verb_present: "run",
                max_subject_chars: Some(60),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hook_decision_core::fixtures::{mismatch, mismatch_report, parse_fixture_set, FixtureCall};

    static FIXTURES_TOML: &str = include_str!("../fixtures.toml");

    fn config_with(flags: &[&str], allow_paths: &str) -> Config {
        let args: Vec<String> = flags.iter().map(|flag| flag.to_string()).collect();
        read_config(&args, allow_paths, "/home/user").unwrap()
    }

    fn bash(command: &str) -> HookInput {
        HookInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({ "command": command }),
            cwd: Some("/work/project".to_string()),
        }
    }

    // ── Every fixture gets its verdict ───────────────────────────

    #[test]
    fn every_fixture_gets_its_verdict() {
        let set = parse_fixture_set(FIXTURES_TOML).unwrap();
        let config = read_config(&set.arguments, &set.allow_paths, &set.home).unwrap();
        let mismatches: Vec<String> = set
            .fixtures
            .iter()
            .filter_map(|fixture| {
                let FixtureCall::Command(command) = &fixture.call else {
                    return Some(format!("{}\n    is not a Bash command", fixture.name));
                };
                let input = HookInput {
                    tool_name: "Bash".to_string(),
                    tool_input: serde_json::json!({ "command": command }),
                    cwd: Some(fixture.working_directory.clone()),
                };
                mismatch(fixture, &decide_inner(&input, &config))
            })
            .collect();
        assert!(mismatches.is_empty(), "{}", mismatch_report(&mismatches, set.fixtures.len()));
    }

    // ── The rules file and the command line ──────────────────────

    #[test]
    fn the_embedded_rules_read_and_every_category_has_rules() {
        let rules = parse_rules(RULES_TOML).unwrap();
        for (category, _) in PATTERN_CATEGORIES {
            assert!(!rules_of(&rules, category).is_empty(), "{category} has no rules");
        }
        assert_eq!(rules_of(&rules, CHAINING).len(), CompoundKind::ALL.len());
    }

    #[test]
    fn a_rules_file_with_a_bad_pattern_or_an_unknown_category_is_refused() {
        assert!(parse_rules("[[subversion]]\npattern = \"(\"\ndescription = \"x\"\n").is_err());
        assert!(parse_rules("[[gaming]]\npattern = \"x\"\ndescription = \"x\"\n").is_err());
    }

    #[test]
    fn a_rules_file_must_give_every_shape_its_rule_and_no_other() {
        let only_one = "[[chaining]]\npattern = \"chained\"\ndescription = \"x\"\n";
        assert!(parse_rules(only_one).unwrap_err().contains("no rule for the shape"));
        let unknown_shape = RULES_TOML.to_string() + "\n[[chaining]]\npattern = \"sideways\"\ndescription = \"x\"\n";
        assert!(parse_rules(&unknown_shape).unwrap_err().contains("sideways"));
    }

    #[test]
    fn a_misspelled_flag_or_severity_is_refused() {
        let misspelled_flag = vec!["--subverison".to_string(), "block".to_string()];
        let misspelled_severity = vec!["--subversion".to_string(), "blok".to_string()];
        assert!(read_config(&misspelled_flag, "", "/home/user").is_err());
        assert!(read_config(&misspelled_severity, "", "/home/user").is_err());
    }

    #[test]
    fn a_category_without_its_flag_is_not_examined() {
        let only_probing = config_with(&["--probing", "block"], "");
        assert!(matches!(decide_inner(&bash("chflags noschg x"), &only_probing), HookDecision::Allow));
        assert!(matches!(decide_inner(&bash("echo a; echo b"), &only_probing), HookDecision::Allow));
    }

    // ── A command that matches a rule is decided whatever it is made of ──

    #[test]
    fn a_rule_match_padded_with_multibyte_characters_is_denied() {
        let blocking = config_with(&["--subversion", "block"], "");
        for width in 40..70 {
            let command = format!("chflags noschg {}{}", "a".repeat(width), "\u{1F512}".repeat(20));
            assert!(matches!(decide_inner(&bash(&command), &blocking), HookDecision::Deny { .. }), "width {width}");
        }
    }

    // ── Compound commands ────────────────────────────────────────

    #[test]
    fn a_compound_command_is_refused_and_told_the_way_through() {
        let blocking = config_with(&["--chaining", "block"], "");
        match decide_inner(&bash("echo a; echo b"), &blocking) {
            HookDecision::Deny { category, reason, .. } => {
                assert_eq!(category, CHAINING);
                assert!(reason.contains("its own call"), "{reason}");
                assert!(reason.contains("cd DIR && command"), "{reason}");
            }
            other => panic!("must deny, got {other:?}"),
        }
    }

    #[test]
    fn a_compound_command_is_never_put_to_the_user() {
        let asking = config_with(&["--chaining", "ask"], "");
        assert!(matches!(decide_inner(&bash("echo a; echo b"), &asking), HookDecision::Deny { .. }));

        // Chaining only warns, and the chain holds a command a rule asks about.
        let warning = config_with(&["--chaining", "warn", "--destruction", "block"], "");
        match decide_inner(&bash("ls; rm -rf build"), &warning) {
            HookDecision::Deny { category, reason, .. } => {
                assert_eq!(category, "destruction");
                assert!(reason.contains("not put up for approval"), "{reason}");
            }
            other => panic!("must deny, got {other:?}"),
        }
    }

    #[test]
    fn a_warned_compound_command_still_gets_the_other_categories() {
        let warning = config_with(&["--chaining", "warn", "--subversion", "block"], "");
        assert!(matches!(decide_inner(&bash("ls; chflags noschg x"), &warning), HookDecision::Deny { .. }));
        match decide_inner(&bash("ls; pwd"), &warning) {
            HookDecision::Warn { category, .. } => assert_eq!(category, CHAINING),
            other => panic!("must warn, got {other:?}"),
        }
    }

    // ── A command in a directory ─────────────────────────────────

    #[test]
    fn a_command_in_a_directory_is_asked_about_as_itself_with_its_directory() {
        let asking = config_with(&["--revert", "ask", "--chaining", "block"], "");
        match decide_inner(&bash("cd /repo && git stash"), &asking) {
            HookDecision::Ask { reason, .. } => {
                assert!(reason.contains("'git stash'"), "{reason}");
                assert!(reason.contains("In directory: /repo"), "{reason}");
                assert!(!reason.contains("cd /repo"), "{reason}");
            }
            other => panic!("must ask, got {other:?}"),
        }
    }

    #[test]
    fn a_plain_command_in_a_directory_is_allowed() {
        let blocking = config_with(&["--chaining", "block", "--subversion", "block"], "");
        assert!(matches!(decide_inner(&bash("cd /repo && cargo test -p x"), &blocking), HookDecision::Allow));
    }

    #[test]
    fn a_path_named_after_cd_is_judged_from_that_directory() {
        let granted = config_with(&["--probing", "block"], "/n/");
        let inside = decide_inner(&bash("cd /n && cat hooks/hook_pre_llm_bash/src/main.rs"), &granted);
        assert!(matches!(inside, HookDecision::Allow));
        let outside = decide_inner(&bash("cd /elsewhere && cat hooks/hook_pre_llm_bash/src/main.rs"), &granted);
        assert!(matches!(outside, HookDecision::Deny { .. }));
    }

    #[test]
    fn a_shell_write_warns_and_names_the_write_tool() {
        let warning = config_with(&["--workflow", "ask"], "");
        match decide_inner(&bash("echo x > src/a.py"), &warning) {
            HookDecision::Warn { llm_context, .. } => assert!(llm_context.contains("Write"), "{llm_context}"),
            other => panic!("must warn, got {other:?}"),
        }
    }
}
