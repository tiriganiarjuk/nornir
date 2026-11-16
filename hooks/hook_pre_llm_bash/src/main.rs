//! PreToolUse hook: what the Bash commands of an interactive session may do.
//!
//! Two sources of rules:
//!   rules.toml (this crate) — command patterns, by category
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

use std::process::ExitCode;

use hook_decision_core::flags::{parse_category_flags, CategorySeverities};
use hook_decision_core::paths::{is_under, normalize};
use hook_decision_core::rules::{
    command_subject, decide_category, first_match, parse_rule_set, rules_of, PatternKind, RuleSet,
};
use hook_decision_core::shell::{named_paths, structure};
use hook_decision_core::{ask_because_undecided, make_decision, DecisionInput, HookDecision, Severity};
use hook_io::{HookInput, WhenUndecided, FLOOR, PROBING};

const HOOK: &str = "hook_pre_llm_bash";

static RULES_TOML: &str = include_str!("../rules.toml");

/// Which text of a command a category's patterns are matched against.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Reads {
    /// The whole command: text a shell could run is examined wherever it sits,
    /// inside quotes and here-document bodies too.
    Whole,
    /// The command's structure: how its commands are joined and where their
    /// output goes. Text a command merely carries is not examined.
    Structure,
}

/// The categories of rules.toml, in the order they are examined: data loss
/// first, then how commands are joined, then the rest.
const CATEGORIES: [(&str, Reads); 7] = [
    ("destruction", Reads::Whole),
    ("chaining", Reads::Structure),
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

fn parse_rules(toml_str: &str) -> Result<RuleSet, String> {
    let categories: Vec<(&str, PatternKind)> =
        CATEGORIES.iter().map(|(category, _)| (*category, PatternKind::Regex)).collect();
    parse_rule_set(toml_str, &categories)
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
    CATEGORIES.iter().map(|(category, _)| *category).chain([PROBING]).collect()
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

/// The decision about one command: the floor, then each category of
/// rules.toml in order, then probing.
fn decide_inner(input: &HookInput, config: &Config) -> HookDecision {
    let command = input.command();
    if command.trim().is_empty() {
        return HookDecision::Allow;
    }
    let named = named_paths(command, input.cwd.as_deref().unwrap_or("/"), &config.home);

    floor_decision(command, &named, &config.protected)
        .or_else(|| pattern_decision(command, config))
        .or_else(|| probing_decision(command, &named, config))
        .unwrap_or(HookDecision::Allow)
}

fn floor_decision(command: &str, named: &[String], protected: &RuleSet) -> Option<HookDecision> {
    let floor = rules_of(protected, FLOOR);
    named.iter().find_map(|path| first_match(floor, path)).map(|rule| {
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

fn pattern_decision(command: &str, config: &Config) -> Option<HookDecision> {
    let command_structure = structure(command);
    CATEGORIES.iter().find_map(|(category, reads)| {
        let severity = *config.severities.get(*category)?;
        let examined = match reads {
            Reads::Whole => command,
            Reads::Structure => command_structure.as_str(),
        };
        decide_category(rules_of(&config.rules, category), &command_subject(examined), category, severity)
    })
}

fn probing_decision(command: &str, named: &[String], config: &Config) -> Option<HookDecision> {
    let severity = *config.severities.get(PROBING)?;
    let probing = rules_of(&config.protected, PROBING);
    named
        .iter()
        .filter(|path| !config.allow_paths.iter().any(|allowed| is_under(path, allowed)))
        .find_map(|path| first_match(probing, path))
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

    // Every category at block, so a decision's kind shows a rule's own severity.
    fn config_with(flags: &[&str], allow_paths: &str) -> Config {
        let args: Vec<String> = flags.iter().map(|flag| flag.to_string()).collect();
        read_config(&args, allow_paths, "/home/user").unwrap()
    }

    fn all_block() -> Config {
        config_with(
            &[
                "--subversion", "block", "--truncation", "block", "--evasion", "block", "--destruction", "block",
                "--revert", "block", "--workflow", "block", "--chaining", "block", "--probing", "block",
            ],
            "",
        )
    }

    fn bash(command: &str) -> HookInput {
        HookInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({ "command": command }),
            cwd: Some("/work/project".to_string()),
        }
    }

    fn category_of(decision: &HookDecision) -> &str {
        match decision {
            HookDecision::Deny { category, .. }
            | HookDecision::Ask { category, .. }
            | HookDecision::Warn { category, .. } => category,
            _ => "",
        }
    }

    fn decided(command: &str) -> HookDecision {
        decide_inner(&bash(command), &all_block())
    }

    /// Every command in `fires` is decided in `category`; none in `passes` is.
    fn assert_category(category: &str, fires: &[&str], passes: &[&str]) {
        for command in fires {
            assert_eq!(category_of(&decided(command)), category, "must be {category}: {command}");
        }
        for command in passes {
            assert_ne!(category_of(&decided(command)), category, "must not be {category}: {command}");
        }
    }

    // ── The rules file and the command line ──────────────────────

    #[test]
    fn the_embedded_rules_read_and_every_category_has_rules() {
        let rules = parse_rules(RULES_TOML).unwrap();
        for (category, _) in CATEGORIES {
            assert!(!rules_of(&rules, category).is_empty(), "{category} has no rules");
        }
    }

    #[test]
    fn a_rules_file_with_a_bad_pattern_is_refused() {
        assert!(parse_rules("[[subversion]]\npattern = \"(\"\ndescription = \"x\"\n").is_err());
    }

    #[test]
    fn a_rules_file_with_an_unknown_category_is_refused() {
        assert!(parse_rules("[[gaming]]\npattern = \"x\"\ndescription = \"x\"\n").is_err());
    }

    #[test]
    fn the_settings_command_line_is_read() {
        let args: Vec<String> = [
            "--subversion", "block", "--truncation", "warn", "--evasion", "warn", "--destruction", "block",
            "--revert", "ask", "--workflow", "ask", "--chaining", "block", "--probing", "warn",
        ]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
        assert!(read_config(&args, "", "/home/user").is_ok());
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
        let only_chaining = config_with(&["--chaining", "block"], "");
        assert!(matches!(decide_inner(&bash("chflags noschg x"), &only_chaining), HookDecision::Allow));
    }

    #[test]
    fn an_empty_command_is_allowed() {
        assert!(matches!(decided("   "), HookDecision::Allow));
    }

    // ── A command that matches a rule is decided whatever it is made of ──

    #[test]
    fn a_rule_match_padded_with_multibyte_characters_is_denied() {
        // The shape that used to fail the hook: a multi-byte character across
        // the point where the shown command is cut.
        for width in 40..70 {
            let command = format!("chflags noschg {}{}", "a".repeat(width), "\u{1F512}".repeat(20));
            assert!(matches!(decided(&command), HookDecision::Deny { .. }), "padding width {width}");
        }
    }

    // ── Floor ────────────────────────────────────────────────────

    #[test]
    fn a_command_that_names_a_credential_store_is_denied_with_no_flag() {
        let no_flags = config_with(&[], "");
        let commands = [
            "cat ~/.ssh/id_rsa",
            "cp /home/user/.aws/credentials /tmp/x",
            "ls /home/user/project/../.gnupg/",
            "bash -c 'cat ~/.netrc'",
            "cat <<EOF | sh\ncat ~/.kube/config\nEOF",
            "grep auth \"/home/user/.docker/config.json\"",
        ];
        for command in commands {
            let decision = decide_inner(&bash(command), &no_flags);
            assert!(matches!(decision, HookDecision::Deny { .. }), "{command}");
            assert_eq!(category_of(&decision), FLOOR, "{command}");
        }
    }

    #[test]
    fn a_grant_never_exempts_the_floor() {
        let granted = config_with(&["--probing", "block"], "/home/user/");
        assert_eq!(category_of(&decide_inner(&bash("cat ~/.ssh/id_rsa"), &granted)), FLOOR);
    }

    #[test]
    fn a_command_that_only_mentions_a_tool_by_name_is_not_on_the_floor() {
        for command in ["ssh example.org uptime", "aws s3 ls", "docker ps", "git push origin master"] {
            assert_ne!(category_of(&decided(command)), FLOOR, "{command}");
        }
    }

    // ── Probing ──────────────────────────────────────────────────

    #[test]
    fn a_command_that_names_the_security_infrastructure_is_probing() {
        assert_category(
            PROBING,
            &[
                "cat /n/hooks/hook_pre_llm_bash/src/main.rs",
                "less hooks/hook_pre_subagent_bash/src/main.rs",
                "git diff -- /n/cli/cc_launch/src/permissions.rs",
                "wc -l /n/capability/hook_io/protected_paths.toml",
            ],
            &["cargo test -p gleipnir_core", "ls /work/project/src", "cat README.md"],
        );
    }

    #[test]
    fn the_settings_rule_asks() {
        assert!(matches!(decided("cat ~/.claude/settings.json"), HookDecision::Ask { .. }));
    }

    #[test]
    fn a_granted_directory_is_exempt_from_probing() {
        let granted = config_with(&["--probing", "block"], "/n/");
        let decision = decide_inner(&bash("cat /n/hooks/hook_pre_llm_bash/src/main.rs"), &granted);
        assert!(matches!(decision, HookDecision::Allow));
    }

    #[test]
    fn a_path_that_climbs_out_of_a_granted_directory_is_not_exempt() {
        let granted = config_with(&["--probing", "block"], "/a/tools/");
        let decision = decide_inner(&bash("cat /a/tools/../n/core/gleipnir_core/src/lib.rs"), &granted);
        assert_eq!(category_of(&decision), PROBING);
    }

    // ── Subversion ───────────────────────────────────────────────

    #[test]
    fn subversion_fires_on_the_act_and_not_on_its_neighbours() {
        assert_category(
            "subversion",
            &[
                "rm /tmp/build.lock",
                "sudo rm -f x.lock",
                "ls; rm a.lock",
                "flock --unlock /tmp/x",
                "sudo chflags noschg rules.toml",
                "chflags -R noschg /a/b",
                "chmod 644 hooks/x/rules.toml",
                "chmod u+w /n/capability/hook_io/protected_paths.toml",
                "xattr -d com.apple.quarantine rules.toml",
                "export HOOK_LLM_ALLOW_PATHS=/",
                "env HOOK_LLM_ALLOW_PATHS=/ bash",
                "HOOK_LLM_ALLOW_PATHS=/ cat x",
            ],
            &[
                "git log -- Cargo.lock",
                "git diff Cargo.lock",
                "cargo update --locked",
                "perform check x.lock",
                "ls *.lock",
                "flock /tmp/x cmd",
                "chflags schg rules.toml",
                "chmod +x tools/run.sh",
                "xattr -l rules.toml",
                "echo $HOOK_LLM_ALLOW_PATHS",
                "printenv",
            ],
        );
    }

    // ── Truncation ───────────────────────────────────────────────

    #[test]
    fn truncation_fires_on_partial_reads_of_constraint_files() {
        assert_category(
            "truncation",
            &[
                "head -20 CLAUDE.md",
                "tail -n 5 /p/CLAUDE.md",
                "sed -n 1,10p CLAUDE.md",
                "awk 'NR<5' CLAUDE.md",
                "grep rule CLAUDE.md",
                "cat guardrails.md | tail -3",
                "head guardrails.toml",
            ],
            &[
                "cat CLAUDE.md",
                "wc -l CLAUDE.md",
                "head -20 README.md",
                "tail -f build.log",
                "grep rule README.md",
                "overhead --report CLAUDE.md",
                "ls CLAUDE.md",
            ],
        );
    }

    // ── Evasion ──────────────────────────────────────────────────

    #[test]
    fn evasion_fires_on_replacing_constraints() {
        assert_category(
            "evasion",
            &[
                "git checkout HEAD~1 -- CLAUDE.md",
                "git restore CLAUDE.md",
                "mv CLAUDE.md CLAUDE.md.bak",
                "cp /tmp/new.md CLAUDE.md",
                "git config core.hooksPath /tmp/hooks",
                "git -c core.hooksPath=/dev/null commit -m x",
            ],
            &[
                "scp host:notes.md .",
                "git add docs/config-hooks.md",
                "git log -- hooks/config.rs",
                "git config user.name x",
                "git status",
                "cat CLAUDE.md",
            ],
        );
    }

    // ── Destruction ──────────────────────────────────────────────

    #[test]
    fn destruction_fires_on_discarding_work() {
        assert_category(
            "destruction",
            &[
                "git reset --hard",
                "git reset --hard HEAD~3",
                "git checkout .",
                "git checkout -- .",
                "git restore .",
                "git clean -fd",
                "git clean -f",
                "rm -rf build",
                "rm build -r",
                "rm --recursive build",
                "find . -name '*.pyc' -delete",
            ],
            &[
                "git reset --soft HEAD~1",
                "git checkout feature-branch",
                "git clean -n",
                "rm notes.txt",
                "rm my-file.txt",
                "find . -name '*.pyc'",
            ],
        );
    }

    #[test]
    fn a_recursive_rm_asks_at_any_category_severity() {
        assert!(matches!(decided("rm -rf build"), HookDecision::Ask { .. }));
        assert!(matches!(decided("git reset --hard"), HookDecision::Deny { .. }));
    }

    // ── Revert ───────────────────────────────────────────────────

    #[test]
    fn revert_fires_on_file_reverts_and_stashes() {
        assert_category(
            "revert",
            &[
                "git checkout -- src/main.rs",
                "git checkout HEAD -- src/main.rs",
                "git restore src/main.rs",
                "git stash",
                "git stash push -m wip",
            ],
            &[
                "git restore --staged src/main.rs",
                "git stash list",
                "git stash pop",
                "git stash show",
                "git checkout main",
            ],
        );
    }

    // ── Workflow ─────────────────────────────────────────────────

    #[test]
    fn workflow_fires_on_builds_that_are_run() {
        assert_category(
            "workflow",
            &[
                "cargo build --release",
                "cargo build -p x --release",
                "cargo install --path . --release",
                "cargo build",
                "cd x; cargo build",
                "RUSTFLAGS=-g cargo build",
                "maturin build",
            ],
            &[
                "cargo test",
                "cargo check",
                "cargo clippy",
                "grep -rn \"cargo build\" docs",
                "git commit -m \"ran cargo build --release\"",
                "git commit -F - <<'EOF'\ncargo build --release\nEOF",
                "echo maturin build",
            ],
        );
    }

    #[test]
    fn a_debug_build_warns_and_a_release_build_asks() {
        assert!(matches!(decided("cargo build"), HookDecision::Warn { .. }));
        assert!(matches!(decided("cargo build --release"), HookDecision::Ask { .. }));
    }

    #[test]
    fn a_shell_write_to_a_source_file_is_flagged() {
        assert_category(
            "workflow",
            &[
                "echo 'x = 1' > src/config.py",
                "printf 'fn main() {}' >> src/main.rs",
                "cat > \"src/my page.svelte\" <<'EOF'\n<p>hi</p>\nEOF",
                "cat notes | tee src/lib.rs",
                "tee -a src/app.py < in",
                "sed -i '' 's/a/b/' src/main.rs",
                "perl -pi -e 's/a/b/' src/x.py",
            ],
            &[
                "cargo test > test.log",
                "echo done >> notes.md",
                "cat src/main.rs",
                "sed -n 1,5p src/main.rs",
                "grep -c '>' src/main.rs",
                "git commit -m \"wrote > main.rs\"",
                "python3 tools/run.py > out.json",
                "ls > files.pyc",
            ],
        );
    }

    #[test]
    fn a_shell_write_warns_and_names_the_write_tool() {
        match decided("echo x > src/a.py") {
            HookDecision::Warn { llm_context, .. } => assert!(llm_context.contains("Write"), "{llm_context}"),
            other => panic!("must warn, got {other:?}"),
        }
    }

    // ── Chaining ─────────────────────────────────────────────────

    #[test]
    fn chaining_fires_on_commands_joined_with_and() {
        assert_category(
            "chaining",
            &["cd /tmp && ls", "cargo test -p x && cargo test -p y", "a && b && c", "cat <<EOF && ls\nbody\nEOF"],
            &[
                "ls -la",
                "cat x | grep y",
                "cmd 2>&1",
                "test $((a&&b))",
                "git commit -m \"build && deploy\"",
                "echo 'a && b'",
                "git commit -F - <<'EOF'\nfirst && second\nEOF",
            ],
        );
    }

    // ── Benign commands ──────────────────────────────────────────

    #[test]
    fn ordinary_commands_are_allowed() {
        for command in ["ls -la", "cat file.txt", "git status", "git diff", "cargo test -p hook_io", "pwd"] {
            assert!(matches!(decided(command), HookDecision::Allow), "{command}");
        }
    }
}
