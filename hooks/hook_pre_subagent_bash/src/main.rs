//! PreToolUse hook: the Bash sandbox of a subagent.
//!
//! A subagent may run exactly two kinds of command, both named on this hook's
//! command line; every other command is denied.
//!
//! Usage (in agent frontmatter):
//!     hook_pre_subagent_bash --writer tool1 tool2 --inspect cmd1=/path/ cmd2=/path/
//!
//!   --writer   a nornir writer, fed one record on stdin in one of two forms:
//!                  cat <<'RECORD' | writer [name]        (the record on the lines
//!                  {...}                                  between, the delimiter
//!                  RECORD                                 alone on the last line)
//!                  echo '{...}' | writer [name]          (one line)
//!   --inspect  a read-only command (ls, find, tree, ...) confined to one
//!              directory: one line, no shell operators, every path it names
//!              under that directory, and no option that runs a command or
//!              writes or deletes a file.
//!
//! A command is allowed only when the whole of it is one of these forms.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::sync::LazyLock;

use hook_decision_core::paths::{absolute_from, is_under, normalize};
use hook_decision_core::{deny_because_undecided, HookDecision};
use hook_io::{HookInput, WhenUndecided};
use regex::Regex;

const HOOK: &str = "hook_pre_subagent_bash";

/// `cat <<'DELIM' | writer [name]` — the whole first line of the heredoc form.
static HEREDOC_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^cat[ \t]+<<'([A-Z_]+)'[ \t]*\|[ \t]*([A-Za-z0-9_]+)(?:[ \t]+([A-Za-z0-9][A-Za-z0-9._-]*))?[ \t]*$")
        .expect("the heredoc header pattern is a valid regex")
});

/// `echo '...' | writer [name]` — the whole command of the echo form.
static ECHO_PIPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^echo[ \t]+'[^'\n\r]*'[ \t]*\|[ \t]*([A-Za-z0-9_]+)(?:[ \t]+([A-Za-z0-9][A-Za-z0-9._-]*))?[ \t]*$")
        .expect("the echo pipe pattern is a valid regex")
});

/// Characters that give a command line structure beyond one command and its
/// arguments. An inspect command holds none of them, quoted or not.
const SHELL_OPERATORS: [char; 13] = [';', '|', '&', '<', '>', '(', ')', '$', '`', '{', '}', '\\', '\n'];

/// Options of `find` that run a command, or write or delete.
const FIND_ACTING_OPTIONS: [&str; 9] =
    ["-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf", "-fls"];

/// Short options of `tree` that write files: `-o FILE`, and `-R`, which
/// writes a file into every directory it crosses.
const TREE_ACTING_LETTERS: [char; 2] = ['o', 'R'];

/// The option among the words that makes this inspect command act, if any.
/// Each command's options are its own: `-o` writes for `tree`, and is the
/// "or" of two tests for `find`.
fn acting_option<'a>(inspect_command: &str, words: &'a [String]) -> Option<&'a str> {
    let acts = |word: &&String| match inspect_command {
        "find" => FIND_ACTING_OPTIONS.contains(&word.as_str()),
        "tree" => {
            let is_short_cluster = word.starts_with('-') && !word.starts_with("--");
            word.starts_with("--output") || (is_short_cluster && word.contains(TREE_ACTING_LETTERS))
        }
        _ => false,
    };
    words.iter().find(acts).map(String::as_str)
}

fn main() -> ExitCode {
    hook_io::run_pre_hook(WhenUndecided::Deny, decide)
}

// ── Config ─────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct Config {
    writers: Vec<String>,
    /// Inspect command → the directory it is confined to, normalised.
    inspect: BTreeMap<String, String>,
}

/// The command line, read strictly: every argument belongs to a section, and
/// every inspect entry is `command=/directory`.
fn read_config(args: &[String]) -> Result<Config, String> {
    let mut config = Config::default();
    let mut section: Option<&str> = None;
    for arg in args {
        match (arg.as_str(), section) {
            ("--writer", _) => section = Some("writer"),
            ("--inspect", _) => section = Some("inspect"),
            (name, Some("writer")) => config.writers.push(name.to_string()),
            (entry, Some("inspect")) => {
                let (command, directory) = inspect_entry(entry)?;
                config.inspect.insert(command, directory);
            }
            (other, _) => return Err(format!("{other} comes before --writer or --inspect")),
        }
    }
    Ok(config)
}

fn inspect_entry(entry: &str) -> Result<(String, String), String> {
    match entry.split_once('=') {
        Some((command, directory)) if !command.is_empty() && directory.starts_with('/') => {
            Ok((command.to_string(), normalize(directory)))
        }
        _ => Err(format!("{entry} is not command=/directory")),
    }
}

// ── Decision logic ─────────────────────────────────────────────────

fn decide(input: &HookInput) -> HookDecision {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match read_config(&args) {
        Ok(config) => decide_inner(input, &config),
        Err(cause) => deny_because_undecided(HOOK, &cause),
    }
}

fn decide_inner(input: &HookInput, config: &Config) -> HookDecision {
    let command = input.command();
    let Some(first_word) = command.split_whitespace().next() else {
        return deny("bash", "empty command".into(), "Empty command.".into());
    };

    if first_word == "cat" || first_word == "echo" {
        return validate_writer(command, &config.writers);
    }
    match config.inspect.get(first_word) {
        Some(directory) => validate_inspect(command, directory, input.cwd.as_deref().unwrap_or("/")),
        None => deny_not_allowed(first_word, config),
    }
}

fn deny(category: &str, event: String, reason: String) -> HookDecision {
    HookDecision::Deny { category: category.into(), event, reason }
}

fn deny_not_allowed(first_word: &str, config: &Config) -> HookDecision {
    let allowed: Vec<String> = config
        .writers
        .iter()
        .map(|writer| format!("  cat <<'RECORD' | {writer}\n  {{...json...}}\n  RECORD"))
        .chain(config.inspect.keys().map(|command| format!("  {command} <args>")))
        .collect();
    let help = if allowed.is_empty() { "  (none)".to_string() } else { allowed.join("\n") };
    deny(
        "bash",
        format!("'{first_word}' is not allowed"),
        format!(
            "'{first_word}' is not an allowed command.\nAllowed commands:\n{help}\n\
             All other Bash commands are blocked by the security boundary."
        ),
    )
}

// ── Writer validation ──────────────────────────────────────────────

/// The writer and optional name of a command that is, whole, one of the two
/// writer forms.
fn writer_invocation(command: &str) -> Option<(String, Option<String>)> {
    let lines: Vec<&str> = command.split('\n').collect();
    let header = match lines.as_slice() {
        [only] => ECHO_PIPE.captures(only)?,
        [first, body @ .., last] => {
            let header = HEREDOC_HEADER.captures(first)?;
            let delimiter = header.get(1)?.as_str();
            // The heredoc ends at the first line that is the delimiter. That
            // line must be the last: anything after it would run as commands.
            if *last != delimiter || body.iter().any(|line| *line == delimiter) {
                return None;
            }
            // In the heredoc form the delimiter is group 1; shift to the echo
            // form's numbering by reading the writer and name from 2 and 3.
            return Some((header.get(2)?.as_str().to_string(), header.get(3).map(|name| name.as_str().to_string())));
        }
        [] => return None,
    };
    Some((header.get(1)?.as_str().to_string(), header.get(2).map(|name| name.as_str().to_string())))
}

fn validate_writer(command: &str, writers: &[String]) -> HookDecision {
    match writer_invocation(command) {
        Some((writer, name)) => check_writer_and_name(&writer, name.as_deref(), writers),
        None => deny(
            "bash",
            "unrecognized writer invocation pattern".into(),
            "Writer commands must be exactly one of:\n  \
             cat <<'RECORD' | writer_name [name]\n  \
             {...json...}\n  \
             RECORD\n\
             or, on one line:\n  \
             echo '{...json...}' | writer_name [name]\n\
             The delimiter appears only on the last line, and nothing follows it."
                .into(),
        ),
    }
}

fn check_writer_and_name(writer: &str, name: Option<&str>, writers: &[String]) -> HookDecision {
    if !writers.iter().any(|allowed| allowed == writer) {
        let listed: Vec<String> = writers.iter().map(|allowed| format!("  {allowed}")).collect();
        return deny(
            "bash",
            format!("'{writer}' not in allowed writers"),
            format!("'{writer}' is not an allowed writer.\nAllowed writers:\n{}", listed.join("\n")),
        );
    }
    match name {
        Some(name) if path_core::validate_path_segment(name).is_err() => deny(
            "bash",
            format!("invalid name arg '{name}'"),
            format!(
                "Name argument '{name}' must be a plain filename stem (e.g., 'entry-123').\n\
                 No path separators, no leading dots, no leading dashes."
            ),
        ),
        _ => HookDecision::Allow,
    }
}

// ── Inspect validation ─────────────────────────────────────────────

fn validate_inspect(command: &str, directory: &str, working_directory: &str) -> HookDecision {
    if let Some(operator) = command.chars().find(|character| SHELL_OPERATORS.contains(character) || *character == '\r') {
        return deny_operator(command, operator);
    }

    let mut command_words = command.split_whitespace().map(unquoted);
    let inspect_command = command_words.next().unwrap_or_default();
    let words: Vec<String> = command_words.collect();
    if let Some(option) = acting_option(&inspect_command, &words) {
        return deny(
            "bash",
            format!("option '{option}' is not read-only"),
            format!("'{option}' runs a command, or writes or deletes files. This command may only read."),
        );
    }

    if let Some(under_home) = words.iter().find(|word| word.starts_with('~')) {
        return deny(
            "path",
            format!("path '{under_home}' is under a home directory"),
            format!("'{under_home}' starts with ~, which the shell expands to a home directory. Name absolute paths under '{directory}'."),
        );
    }

    if let Some(hidden_pattern) = words.iter().find(|word| matches_hidden_names(word)) {
        return deny(
            "path",
            format!("pattern '{hidden_pattern}' can match the parent directory"),
            format!("'{hidden_pattern}' has a pattern that starts with a dot, which can match '..'. Name the files, or the directory."),
        );
    }

    let reached = reached_paths(&words, working_directory);
    match reached.iter().find(|path| !is_under(path, directory)) {
        Some(outside) => deny(
            "path",
            format!("path '{outside}' outside allowed directory"),
            format!("'{outside}' is not under '{directory}'. Name only paths under that directory, as absolute paths."),
        ),
        None => HookDecision::Allow,
    }
}

/// A word as the shell hands it on: with every quote mark removed, wherever in
/// the word it stands.
fn unquoted(word: &str) -> String {
    word.replace(['\'', '"'], "")
}

/// Whether a word holds a path segment that is a pattern starting with a dot
/// (`.*`, `.?x`, `.[a-z]`): such a pattern can match `..`.
fn matches_hidden_names(word: &str) -> bool {
    word.split('/').any(|segment| segment.starts_with('.') && segment.contains(['*', '?', '[']))
}

/// The path an option carries in the same word, if it carries one:
/// `--option=PATH`, or a path written straight after a short option.
fn path_in_option(option: &str) -> Option<&str> {
    match option.split_once('=') {
        Some((_, value)) => looks_like_a_path(value).then_some(value),
        None => option.find('/').map(|path_start| &option[path_start..]),
    }
}

/// Every path the words reach, absolute and normalised.
///
/// A word after an option is that option's value unless it looks like a path
/// (it holds a `/`, or is `.` or `..`); every other word that is not
/// an option is a path, and so is a path an option carries in its own word.
/// A command that names no path reaches its working directory.
fn reached_paths(words: &[String], working_directory: &str) -> Vec<String> {
    let mut reached = Vec::new();
    let mut after_option = false;
    for word in words {
        let is_option = word.starts_with('-');
        if !is_option && (!after_option || looks_like_a_path(word)) {
            reached.push(absolute_from(working_directory, word));
        }
        if let Some(carried) = path_in_option(word).filter(|_| is_option) {
            reached.push(absolute_from(working_directory, carried));
        }
        after_option = is_option;
    }
    if reached.is_empty() {
        reached.push(normalize(working_directory));
    }
    reached
}

fn looks_like_a_path(word: &str) -> bool {
    word.contains('/') || word == "." || word == ".."
}

/// A shell operator in a sandboxed subagent's command is an early adversarial
/// signal: denied, and loud.
fn deny_operator(command: &str, operator: char) -> HookDecision {
    let first_word = command.split_whitespace().next().unwrap_or("?");
    deny(
        "chaining",
        format!("shell operator {operator:?} in '{first_word}' command — possible sandbox escape"),
        "A shell operator ( ; | & < > ( ) $ ` { } \\ or a line break) was found.\n\
         This subagent is sandboxed. Each command must be a single operation.\n\
         Attempts are logged and flagged to the user."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hook_decision_core::fixtures::{mismatch, mismatch_report, parse_fixture_set, FixtureCall};

    static FIXTURES_TOML: &str = include_str!("../fixtures.toml");

    fn config() -> Config {
        let args: Vec<String> =
            ["--writer", "append_record", "write_entry", "--inspect", "ls=/data/in/", "find=/data/in/", "tree=/data/in"]
                .iter()
                .map(|arg| arg.to_string())
                .collect();
        read_config(&args).unwrap()
    }

    fn bash(command: &str) -> HookInput {
        HookInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({ "command": command }),
            cwd: Some("/data/in/work".to_string()),
        }
    }

    #[test]
    fn every_fixture_gets_its_verdict() {
        let set = parse_fixture_set(FIXTURES_TOML).unwrap();
        let config = read_config(&set.arguments).unwrap();
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

    // ── read_config ──────────────────────────────────────────────

    #[test]
    fn the_command_line_is_read() {
        let config = config();
        assert_eq!(config.writers, vec!["append_record", "write_entry"]);
        assert_eq!(config.inspect.get("ls").map(String::as_str), Some("/data/in/"));
    }

    #[test]
    fn an_argument_before_any_section_is_refused() {
        assert!(read_config(&["append_record".to_string()]).is_err());
    }

    #[test]
    fn an_inspect_entry_without_a_directory_is_refused() {
        for entry in ["ls", "ls=", "=/data", "ls=relative/dir"] {
            assert!(read_config(&["--inspect".to_string(), entry.to_string()]).is_err(), "{entry}");
        }
    }

    #[test]
    fn an_empty_command_line_allows_nothing() {
        let nothing = read_config(&[]).unwrap();
        assert!(matches!(decide_inner(&bash("ls /data/in"), &nothing), HookDecision::Deny { .. }));
    }

    // ── A call that states no working directory ──────────────────

    #[test]
    fn a_command_with_no_working_directory_is_judged_from_the_root() {
        let unknown = HookInput::for_tool("Bash", serde_json::json!({ "command": "ls sub" }));
        assert!(matches!(decide_inner(&unknown, &config()), HookDecision::Deny { .. }));
    }
}
