//! The shape of a shell command: whether it is one command, one command in a
//! directory, or several.
//!
//! A call that can be taken in at a glance is one command — a single pipeline
//! with its redirections — optionally after one `cd` to a stated directory.
//! Everything that joins commands is compound: a separator, a group, inline
//! control flow, a shell started inside the command.
//!
//! Lexical, like `shell`: nothing is run and nothing is expanded. Quoted text
//! and here-document bodies are text; the command inside `$( … )` or backticks
//! is read by the same rule.

use crate::shell::heredoc_bodies_removed;

/// Why a command is more than one command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundKind {
    /// Commands joined by `;`, `&&`, `||` or a line break.
    Chained,
    /// A command sent to the background with `&`.
    Backgrounded,
    /// A subshell `( … )`, a brace group `{ …; }`, or a function definition.
    Grouped,
    /// `for`, `while`, `until`, `if`, `case` or `select` written inline.
    ControlFlow,
    /// A shell started inside the command: `bash -c`, `eval`, text fed to a shell.
    NestedShell,
}

impl CompoundKind {
    pub const ALL: [CompoundKind; 5] = [
        CompoundKind::Chained,
        CompoundKind::Backgrounded,
        CompoundKind::Grouped,
        CompoundKind::ControlFlow,
        CompoundKind::NestedShell,
    ];

    /// The name a rules file knows this kind by.
    pub fn key(self) -> &'static str {
        match self {
            CompoundKind::Chained => "chained",
            CompoundKind::Backgrounded => "backgrounded",
            CompoundKind::Grouped => "grouped",
            CompoundKind::ControlFlow => "control_flow",
            CompoundKind::NestedShell => "nested_shell",
        }
    }
}

/// What a command is, read as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandShape {
    /// One command: a single pipeline, with its redirections.
    Single,
    /// `cd DIRECTORY`, then one command.
    InDirectory { directory: String, command: String },
    /// More than one command.
    Compound(CompoundKind),
}

/// The shape of a command.
pub fn command_shape(command: &str) -> CommandShape {
    let joined_lines = heredoc_bodies_removed(command).replace("\\\n", " ");
    shape_of(&joined_lines.chars().collect::<Vec<char>>())
}

fn shape_of(source: &[char]) -> CommandShape {
    let outlined = outline(source);
    let chain_inside_substitution = outlined
        .substitutions
        .iter()
        .any(|inner| shape_of(inner) != CommandShape::Single);
    if chain_inside_substitution {
        return CommandShape::Compound(CompoundKind::Chained);
    }

    let split = simple_commands(&outlined.text);
    let special = split.commands.iter().find_map(|span| compound_kind_of(&outlined.text[span.start..span.end]));
    if let Some(kind) = special {
        return CommandShape::Compound(kind);
    }
    // Every parenthesis that is text or a substitution is blanked by now.
    if outlined.text.iter().any(|character| *character == '(' || *character == ')') {
        return CommandShape::Compound(CompoundKind::Grouped);
    }
    if split.separators.contains(&Separator::Background) {
        return CommandShape::Compound(CompoundKind::Backgrounded);
    }
    joined_shape(source, &outlined.text, &split)
}

/// The shape of commands that are each plain: one is single, a `cd` and one
/// more is a command in a directory, anything else is a chain.
fn joined_shape(source: &[char], outline_text: &[char], split: &Split) -> CommandShape {
    match split.commands.as_slice() {
        [] | [_] => CommandShape::Single,
        [first, second] if split.joins_in_order() => {
            let destination = cd_destination(&outline_text[first.start..first.end], &source[first.start..first.end]);
            let second_is_cd = cd_destination(&outline_text[second.start..second.end], &source[second.start..second.end]).is_some();
            match destination {
                Some(directory) if !second_is_cd => CommandShape::InDirectory {
                    directory,
                    command: source[second.start..second.end].iter().collect::<String>().trim().to_string(),
                },
                _ => CommandShape::Compound(CompoundKind::Chained),
            }
        }
        _ => CommandShape::Compound(CompoundKind::Chained),
    }
}

// ── The outline: a command with its text blanked ───────────────────

/// A command with everything that is text replaced by `_`, character for
/// character, so positions in it are positions in the command; and the
/// commands found inside `$( … )` and backticks.
struct Outline {
    text: Vec<char>,
    substitutions: Vec<Vec<char>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Quoted {
    No,
    Single,
    Double,
}

fn outline(source: &[char]) -> Outline {
    let mut outlined = Outline { text: Vec::with_capacity(source.len()), substitutions: Vec::new() };
    let mut quoted = Quoted::No;
    let mut position = 0;
    while position < source.len() {
        let character = source[position];
        let consumed = match (quoted, character) {
            (Quoted::Single, '\'') => close_or_open(&mut outlined, &mut quoted, Quoted::No, character),
            (Quoted::Single, _) => blank(&mut outlined, 1),
            (_, '\\') => blank(&mut outlined, 2.min(source.len() - position)),
            (_, '$') if source.get(position + 1) == Some(&'(') => substitution(&mut outlined, source, position),
            (_, '`') => backtick_substitution(&mut outlined, source, position),
            (Quoted::No, '\'') => close_or_open(&mut outlined, &mut quoted, Quoted::Single, character),
            (Quoted::No, '"') => close_or_open(&mut outlined, &mut quoted, Quoted::Double, character),
            (Quoted::Double, '"') => close_or_open(&mut outlined, &mut quoted, Quoted::No, character),
            (Quoted::Double, _) => blank(&mut outlined, 1),
            (Quoted::No, _) => keep(&mut outlined, character),
        };
        position += consumed;
    }
    outlined
}

fn keep(outlined: &mut Outline, character: char) -> usize {
    outlined.text.push(character);
    1
}

fn blank(outlined: &mut Outline, count: usize) -> usize {
    outlined.text.extend(std::iter::repeat_n('_', count));
    count
}

fn close_or_open(outlined: &mut Outline, quoted: &mut Quoted, next: Quoted, quote_mark: char) -> usize {
    *quoted = next;
    keep(outlined, quote_mark)
}

/// `$( … )` at `start`: the command inside is kept for its own reading, and
/// the whole span is blanked. `$(( … ))` is arithmetic and holds no command.
fn substitution(outlined: &mut Outline, source: &[char], start: usize) -> usize {
    let inside_start = start + 2;
    let close = matching_close(source, inside_start);
    let is_arithmetic = source.get(inside_start) == Some(&'(');
    if !is_arithmetic {
        outlined.substitutions.push(source[inside_start..close].to_vec());
    }
    blank(outlined, (close + 1).min(source.len()) - start)
}

/// A backtick substitution at `start`, read like `$( … )`.
fn backtick_substitution(outlined: &mut Outline, source: &[char], start: usize) -> usize {
    let inside_start = start + 1;
    let close = source[inside_start..]
        .iter()
        .position(|character| *character == '`')
        .map_or(source.len(), |offset| inside_start + offset);
    outlined.substitutions.push(source[inside_start..close].to_vec());
    blank(outlined, (close + 1).min(source.len()) - start)
}

/// The position of the `)` that closes a parenthesis opened just before
/// `inside_start`; the end of the text when none does. Parentheses inside
/// single quotes do not count.
fn matching_close(source: &[char], inside_start: usize) -> usize {
    let mut depth = 1usize;
    let mut in_single_quotes = false;
    let mut position = inside_start;
    while position < source.len() {
        match (in_single_quotes, source[position]) {
            (_, '\'') => in_single_quotes = !in_single_quotes,
            (false, '\\') => position += 1,
            (false, '(') => depth += 1,
            (false, ')') if depth == 1 => return position,
            (false, ')') => depth -= 1,
            _ => {}
        }
        position += 1;
    }
    source.len()
}

// ── Simple commands and what joins them ────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Separator {
    /// `;` or a line break.
    Sequence,
    /// `&&`
    And,
    /// `||`
    Or,
    /// `&`
    Background,
}

#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
}

struct Split {
    /// The simple commands that hold anything, in order.
    commands: Vec<Span>,
    separators: Vec<Separator>,
}

impl Split {
    /// Whether the commands are joined by exactly one `&&`, `;` or line break.
    fn joins_in_order(&self) -> bool {
        matches!(self.separators.as_slice(), [Separator::And] | [Separator::Sequence])
    }
}

/// The separator at `position` of an outline, and how many characters it is.
/// `&` beside a redirection (`2>&1`, `&>`, `<&`, `|&`) separates nothing.
fn separator_at(outline_text: &[char], position: usize) -> Option<(Separator, usize)> {
    let next = outline_text.get(position + 1).copied();
    let previous = position.checked_sub(1).and_then(|before| outline_text.get(before)).copied();
    match outline_text[position] {
        ';' | '\n' => Some((Separator::Sequence, 1)),
        '&' if next == Some('&') => Some((Separator::And, 2)),
        '&' if matches!(previous, Some('>' | '<' | '|')) || next == Some('>') => None,
        '&' => Some((Separator::Background, 1)),
        '|' if next == Some('|') => Some((Separator::Or, 2)),
        _ => None,
    }
}

fn simple_commands(outline_text: &[char]) -> Split {
    let mut split = Split { commands: Vec::new(), separators: Vec::new() };
    let mut command_start = 0;
    let mut position = 0;
    while position < outline_text.len() {
        match separator_at(outline_text, position) {
            Some((separator, width)) => {
                push_command(&mut split, outline_text, Span { start: command_start, end: position });
                // A line break or `;` after the last command, or before the first, joins nothing.
                split.separators.push(separator);
                position += width;
                command_start = position;
            }
            None => position += 1,
        }
    }
    push_command(&mut split, outline_text, Span { start: command_start, end: outline_text.len() });
    keep_joining_separators(split)
}

fn push_command(split: &mut Split, outline_text: &[char], span: Span) {
    let holds_something = outline_text[span.start..span.end].iter().any(|character| !character.is_whitespace());
    if holds_something {
        split.commands.push(span);
    } else if let Some(Separator::Sequence) = split.separators.last() {
        // An empty stretch between two sequence separators: the earlier one joined nothing.
        split.separators.pop();
    }
}

/// The separators that join two commands. A `;` or line break with no command
/// after it joins nothing; a `&` there still backgrounds the command before it.
fn keep_joining_separators(mut split: Split) -> Split {
    let joining = split.commands.len().saturating_sub(1);
    while split.separators.len() > joining && split.separators.last() == Some(&Separator::Sequence) {
        split.separators.pop();
    }
    split
}

// ── What one simple command is ─────────────────────────────────────

const SHELLS: [&str; 6] = ["bash", "sh", "zsh", "dash", "ksh", "fish"];
const CONTROL_WORDS: [&str; 7] = ["for", "while", "until", "if", "case", "select", "function"];
/// Words that come before a command and are not it.
const COMMAND_PREFIXES: [&str; 6] = ["sudo", "command", "exec", "time", "nohup", "env"];

/// The compound kind of one simple command, if it is one on its own.
fn compound_kind_of(command_outline: &[char]) -> Option<CompoundKind> {
    let text: String = command_outline.iter().collect();
    let segments: Vec<Vec<&str>> = text.split('|').map(command_words).collect();
    let first_word = segments.first().and_then(|words| words.first()).copied().unwrap_or("");
    if first_word == "{" {
        return Some(CompoundKind::Grouped);
    }
    if CONTROL_WORDS.contains(&first_word) {
        return Some(CompoundKind::ControlFlow);
    }
    if first_word == "ssh" {
        return None;
    }
    let starts_a_shell = segments.iter().enumerate().any(|(index, words)| segment_starts_a_shell(index, words));
    starts_a_shell.then_some(CompoundKind::NestedShell)
}

/// The words of one pipeline segment, from its command on: assignments
/// (`NAME=value`) and prefix words before the command are left out.
fn command_words(segment: &str) -> Vec<&str> {
    segment
        .split_whitespace()
        .skip_while(|word| COMMAND_PREFIXES.contains(word) || is_assignment(word))
        .collect()
}

fn is_assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) => !name.is_empty() && name.chars().all(|character| character.is_alphanumeric() || character == '_'),
        None => false,
    }
}

fn is_shell(word: &str) -> bool {
    SHELLS.contains(&word.rsplit('/').next().unwrap_or(word))
}

/// Whether a word is an option cluster that holds `c`, as in `-c` or `-lc`.
fn is_command_option(word: &str) -> bool {
    word.starts_with('-') && !word.starts_with("--") && word.contains('c')
}

/// Whether a pipeline segment starts a shell on text: `eval`, a shell given a
/// command (`-c`), a shell fed by a pipe or a here-document, or a shell given
/// a command by `xargs` or `find -exec`.
fn segment_starts_a_shell(index_in_pipeline: usize, words: &[&str]) -> bool {
    let Some(command) = words.first() else {
        return false;
    };
    let shell_position = words.iter().position(|word| is_shell(word));
    let given_a_command = shell_position
        .is_some_and(|position| words[position + 1..].iter().any(|word| is_command_option(word)));
    let fed_text = is_shell(command)
        && (index_in_pipeline > 0 || words.iter().any(|word| word.contains("<<")) || words.len() == 1);
    *command == "eval" || given_a_command || fed_text
}

/// The directory a `cd` command goes to, when it is `cd` and one stated path:
/// a path with no variable, substitution or pattern in it.
fn cd_destination(command_outline: &[char], command_source: &[char]) -> Option<String> {
    let outline_text: String = command_outline.iter().collect();
    let words: Vec<&str> = outline_text.split_whitespace().collect();
    let [cd, destination_outline] = words.as_slice() else {
        return None;
    };
    if *cd != "cd" || destination_outline.contains(['$', '*', '?', '[', '{']) {
        return None;
    }
    let as_written: String = command_source.iter().collect();
    let stated = as_written.trim().strip_prefix("cd")?.trim();
    let unquoted = stated.trim_matches(|character| character == '"' || character == '\'');
    (!unquoted.is_empty() && !unquoted.contains(['$', '`'])).then(|| unquoted.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_directory(directory: &str, command: &str) -> CommandShape {
        CommandShape::InDirectory { directory: directory.to_string(), command: command.to_string() }
    }

    fn assert_single(commands: &[&str]) {
        for command in commands {
            assert_eq!(command_shape(command), CommandShape::Single, "{command:?}");
        }
    }

    fn assert_compound(kind: CompoundKind, commands: &[&str]) {
        for command in commands {
            assert_eq!(command_shape(command), CommandShape::Compound(kind), "{command:?}");
        }
    }

    #[test]
    fn every_kind_has_its_own_key() {
        let mut keys: Vec<&str> = CompoundKind::ALL.iter().map(|kind| kind.key()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), CompoundKind::ALL.len());
    }

    // ── Single ───────────────────────────────────────────────────

    #[test]
    fn one_command_is_single() {
        assert_single(&["ls -la", "", "   ", "cargo test -p x", "cd /tmp", "VAR=1 OTHER=2 make", "sudo make install"]);
    }

    #[test]
    fn one_pipeline_is_single() {
        assert_single(&["cat x | grep y | wc -l", "make 2>&1 | tail -5", "cmd |& tee log"]);
    }

    #[test]
    fn redirections_are_not_separators() {
        assert_single(&["cmd 2>&1", "cmd > out 2>&1", "cmd &> out", "cmd &>> out", "cmd <&3", "cmd >&2", "cmd < in > out"]);
    }

    #[test]
    fn a_trailing_semicolon_or_line_break_joins_nothing() {
        assert_single(&["ls;", "ls\n", "ls ;\n", "\nls\n"]);
    }

    #[test]
    fn operators_inside_quotes_are_text() {
        assert_single(&[
            r#"git commit -m "build && deploy; done""#,
            "echo 'a && b || c; d & e'",
            r#"ssh host "cd /srv && make; echo done""#,
            "ssh host 'bash -c \"a; b\"'",
            r#"grep -E "a|b" file"#,
            r#"echo "(not a group)""#,
        ]);
    }

    #[test]
    fn an_escaped_operator_is_text() {
        assert_single(&[r"find . -name x -exec rm {} \;", r"echo a\;b", r"echo a \&\& b"]);
    }

    #[test]
    fn a_heredoc_body_is_text() {
        assert_single(&[
            "cat <<'EOF' | writer\na && b; c\nEOF",
            "git commit -F - <<EOF\nfirst; second && third\nEOF",
        ]);
    }

    #[test]
    fn a_continued_line_is_one_line() {
        assert_single(&["cargo test \\\n  -p x \\\n  --quiet"]);
    }

    #[test]
    fn one_command_inside_a_substitution_is_single() {
        assert_single(&[
            "echo $(date)",
            "git log $(git merge-base a b)..HEAD",
            "echo \"today is $(date +%F)\"",
            "echo `date`",
            "git commit -m \"$(cat <<'EOF'\nfix: a && b; c\nEOF\n)\"",
        ]);
    }

    #[test]
    fn arithmetic_is_not_a_command() {
        assert_single(&["echo $((a && b))", "test $((1 + (2 * 3))) -gt 0"]);
    }

    #[test]
    fn a_shell_running_a_script_file_is_single() {
        assert_single(&["bash tools/run.sh", "sh ./install.sh --yes", "/bin/zsh script.zsh"]);
    }

    // ── InDirectory ──────────────────────────────────────────────

    #[test]
    fn cd_then_one_command_is_a_command_in_a_directory() {
        assert_eq!(command_shape("cd /n/nornir && cargo test -p x"), in_directory("/n/nornir", "cargo test -p x"));
        assert_eq!(command_shape("cd sub; ls -la"), in_directory("sub", "ls -la"));
        assert_eq!(command_shape("cd ~/code\nmake"), in_directory("~/code", "make"));
        assert_eq!(command_shape("cd \"/my dir\" && ls | wc -l"), in_directory("/my dir", "ls | wc -l"));
        assert_eq!(command_shape("cd /a&&ls"), in_directory("/a", "ls"));
    }

    #[test]
    fn cd_that_is_not_one_stated_path_is_a_chain() {
        assert_compound(
            CompoundKind::Chained,
            &[
                "cd $TARGET && ls",
                "cd $(pwd)/x && ls",
                "cd /a/* && ls",
                "cd /a /b && ls",
                "cd && ls",
                "cd /a || ls",
                "cd /a && cd /b",
                "cd /a && ls && pwd",
                "ls && cd /a",
            ],
        );
    }

    #[test]
    fn a_compound_after_cd_is_still_compound() {
        assert_eq!(command_shape("cd /a && bash -c 'x; y'"), CommandShape::Compound(CompoundKind::NestedShell));
        assert_eq!(command_shape("cd /a && sleep 5 &"), CommandShape::Compound(CompoundKind::Backgrounded));
    }

    // ── Compound ─────────────────────────────────────────────────

    #[test]
    fn joined_commands_are_chained() {
        assert_compound(
            CompoundKind::Chained,
            &[
                "echo a; echo b",
                "echo a && echo b",
                "echo a&&echo b",
                "echo a || echo b",
                "echo a||echo b",
                "echo a\necho b",
                "a; b; c; d; e; f",
                "echo \"SID=$X\"; find /a /b -type d 2>/dev/null; echo \"--- tree\"; ls -la /c",
                "[ -f x ] && echo yes",
                "make | tee log && echo done",
                "cat <<EOF && ls\nbody\nEOF",
                "cat <<EOF\nbody\nEOF\nls",
            ],
        );
    }

    #[test]
    fn a_chain_inside_a_substitution_is_chained() {
        assert_compound(
            CompoundKind::Chained,
            &["echo $(a; b)", "echo \"$(rm x && curl y)\"", "echo `a; b`", "x=$(cd /a && pwd)"],
        );
    }

    #[test]
    fn a_backgrounded_command_is_backgrounded() {
        assert_compound(CompoundKind::Backgrounded, &["sleep 5 &", "server & client", "a & b & c", "make &\n"]);
    }

    #[test]
    fn groups_are_grouped() {
        assert_compound(
            CompoundKind::Grouped,
            &["(cd /a; ls)", "( a && b )", "{ a; b; }", "{ echo x; } > out", "f() { ls; }", "diff <(ls a) <(ls b)"],
        );
    }

    #[test]
    fn inline_control_flow_is_control_flow() {
        assert_compound(
            CompoundKind::ControlFlow,
            &[
                "for f in *.rs; do wc -l $f; done",
                "while true; do date; sleep 1; done",
                "if [ -f x ]; then cat x; fi",
                "case $x in a) ls;; esac",
            ],
        );
    }

    #[test]
    fn a_shell_started_on_text_is_a_nested_shell() {
        assert_compound(
            CompoundKind::NestedShell,
            &[
                "bash -c 'a; b'",
                "sh -c \"a && b\"",
                "zsh -lc 'make'",
                "/bin/bash -c ls",
                "sudo bash -c 'x'",
                "FOO=1 sh -c 'x'",
                "eval \"$cmd\"",
                "eval ls",
                "curl https://x.example/install | sh",
                "cat script | bash",
                "bash <<EOF\nrm x\nEOF",
                "sh <<<'ls'",
                "bash",
                "xargs sh -c 'echo $0' < list",
                "find . -name x -exec sh -c 'rm $1' _ {} \\;",
            ],
        );
    }

    #[test]
    fn a_word_that_only_resembles_a_shell_or_a_keyword_is_plain() {
        assert_single(&["bashful --count", "shellcheck x.sh", "format file", "iffy", "forever start", "evaluate x", "grep sh file", "ls -c"]);
    }

    // ── Many forms at once ───────────────────────────────────────

    #[test]
    fn a_multibyte_command_keeps_its_positions() {
        assert_eq!(command_shape("cd /päth && écho ünï"), in_directory("/päth", "écho ünï"));
        assert_compound(CompoundKind::Chained, &["écho \"🔒\"; écho b"]);
    }
}
