//! Reading a shell command as text: what in it is command structure, and which
//! paths it names.
//!
//! Lexical, like `paths`: nothing is run and nothing is expanded. These
//! functions answer what a rule needs to ask of a command line; they are not a
//! shell parser.

use std::sync::LazyLock;

use regex::Regex;

use crate::paths::{absolute_from, normalize};

/// A here-document opener: `<<WORD`, `<<-WORD`, `<<'WORD'`, `<<"WORD"`.
static HEREDOC_OPENER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<<(-?)\s*(?:'([A-Za-z_][A-Za-z0-9_]*)'|"([A-Za-z_][A-Za-z0-9_]*)"|([A-Za-z_][A-Za-z0-9_]*))"#)
        .expect("the here-document opener pattern is a valid regex")
});

/// The characters that structure a command line: separators and redirections.
const OPERATORS: [char; 5] = ['&', '|', ';', '<', '>'];

/// The command's structure: the command with every here-document body removed
/// and every operator inside quotes replaced by `_`.
///
/// A rule about how commands are joined or where output is sent is matched
/// against this, so text a command merely carries — a commit message, a file
/// body — is never read as structure. Quoted words keep their letters, so a
/// quoted file name is still a file name.
pub fn structure(command: &str) -> String {
    quoted_operators_replaced(&heredoc_bodies_removed(command))
}

/// The delimiters of the here-documents opened on one line, in order.
fn heredoc_delimiters(line: &str) -> Vec<String> {
    HEREDOC_OPENER
        .captures_iter(line)
        .filter(|opener| !is_here_string(line, opener.get(0).map_or(0, |whole| whole.start())))
        .filter_map(|opener| opener.get(2).or(opener.get(3)).or(opener.get(4)))
        .map(|delimiter| delimiter.as_str().to_string())
        .collect()
}

/// `<<<` opens a here-string, whose text is on the same line.
fn is_here_string(line: &str, opener_start: usize) -> bool {
    line[..opener_start].ends_with('<') || line[opener_start..].starts_with("<<<")
}

pub(crate) fn heredoc_bodies_removed(command: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut awaited: Vec<String> = Vec::new();
    for line in command.lines() {
        match awaited.first() {
            Some(delimiter) if line.trim() == delimiter => {
                awaited.remove(0);
            }
            Some(_) => {}
            None => {
                kept.push(line);
                awaited = heredoc_delimiters(line);
            }
        }
    }
    kept.join("\n")
}

#[derive(Clone, Copy, PartialEq)]
enum Quoting {
    Outside,
    Single,
    Double,
}

fn quoted_operators_replaced(command: &str) -> String {
    let mut shown = String::with_capacity(command.len());
    let mut quoting = Quoting::Outside;
    let mut escaped = false;
    for character in command.chars() {
        let (next_quoting, next_escaped) = quoting_after(quoting, escaped, character);
        let inside = quoting != Quoting::Outside && next_quoting != Quoting::Outside;
        shown.push(if inside && OPERATORS.contains(&character) { '_' } else { character });
        quoting = next_quoting;
        escaped = next_escaped;
    }
    shown
}

/// The quoting state after one character, and whether the next is escaped.
fn quoting_after(quoting: Quoting, escaped: bool, character: char) -> (Quoting, bool) {
    if escaped {
        return (quoting, false);
    }
    match (quoting, character) {
        (Quoting::Single, '\'') => (Quoting::Outside, false),
        (Quoting::Single, _) => (Quoting::Single, false),
        (_, '\\') => (quoting, true),
        (Quoting::Outside, '\'') => (Quoting::Single, false),
        (Quoting::Outside, '"') => (Quoting::Double, false),
        (Quoting::Double, '"') => (Quoting::Outside, false),
        _ => (quoting, false),
    }
}

/// Characters that end a word a path could be part of.
const WORD_BREAKS: [char; 16] = [';', '|', '&', '<', '>', '(', ')', '"', '\'', '=', '$', '`', '{', '}', ',', ':'];

/// Every path the command names, absolute and normalised.
///
/// A word is taken as a path when it holds a `/`. A word starting with `~/` is
/// under `home`; any other relative word is under `working_directory`. The
/// whole command is read, here-document bodies and quoted text included: a
/// path named anywhere in it is a path the command can reach.
///
/// The command is read one simple command at a time, in order. A `cd` moves
/// the working directory for the simple commands that follow it, and a
/// command's own `-C DIRECTORY` is where its relative paths are, so a relative
/// path is resolved from where the command would be when it reaches it.
pub fn named_paths(command: &str, working_directory: &str, home: &str) -> Vec<String> {
    let mut named = Vec::new();
    let mut current_directory = normalize(working_directory);
    for simple_command in command.split(|character: char| COMMAND_SEPARATORS.contains(&character)) {
        let words = words_of(simple_command);
        let stated = stated_directory(&words).map(|directory| resolved(directory, &current_directory, home));
        let base = stated.as_deref().unwrap_or(&current_directory);
        named.extend(stated.iter().cloned());
        named.extend(
            words
                .iter()
                .filter(|word| word.contains('/') && Some(**word) != stated_directory(&words))
                .map(|word| resolved(word, base, home)),
        );
        if let ["cd", destination, ..] = words.as_slice() {
            current_directory = resolved(destination, &current_directory, home);
        }
    }
    named
}

/// The words of one simple command. A `:` inside a bracket class does not end a
/// word, so `~/[[:punct:]]ssh` stays the one pattern the shell expands it as.
fn words_of(simple_command: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut word_start = 0;
    let mut bracket_depth = 0usize;
    for (offset, character) in simple_command.char_indices() {
        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            ':' if bracket_depth > 0 => continue,
            _ => {}
        }
        if character.is_whitespace() || WORD_BREAKS.contains(&character) {
            words.push(&simple_command[word_start..offset]);
            word_start = offset + character.len_utf8();
            bracket_depth = 0;
        }
    }
    words.push(&simple_command[word_start..]);
    words.retain(|word| !word.is_empty());
    words
}

/// The directory a command states for itself with `-C DIRECTORY`, as `git`,
/// `make` and `tar` take it: the command's relative paths are under it.
fn stated_directory<'a>(words: &[&'a str]) -> Option<&'a str> {
    words.iter().position(|word| *word == "-C").and_then(|position| words.get(position + 1)).copied()
}

/// Characters that end one simple command and begin the next.
const COMMAND_SEPARATORS: [char; 4] = [';', '&', '|', '\n'];

/// A word as an absolute, normalised path: under `home` when it starts with
/// `~`, itself when absolute, else under `current_directory`.
fn resolved(word: &str, current_directory: &str, home: &str) -> String {
    match word.strip_prefix('~') {
        Some(under_home) if under_home.is_empty() || under_home.starts_with('/') => {
            normalize(&format!("{home}/{under_home}"))
        }
        _ => absolute_from(current_directory, word),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── structure ────────────────────────────────────────────────

    #[test]
    fn a_plain_command_is_its_own_structure() {
        assert_eq!(structure("cargo test -p x && ls"), "cargo test -p x && ls");
    }

    #[test]
    fn operators_inside_double_quotes_are_not_structure() {
        assert_eq!(structure(r#"git commit -m "a && b > c""#), r#"git commit -m "a __ b _ c""#);
    }

    #[test]
    fn operators_inside_single_quotes_are_not_structure() {
        assert_eq!(structure("echo 'x && y | z'"), "echo 'x __ y _ z'");
    }

    #[test]
    fn operators_after_a_closed_quote_are_structure() {
        assert_eq!(structure(r#"echo "a" && echo 'b' > out"#), r#"echo "a" && echo 'b' > out"#);
    }

    #[test]
    fn an_escaped_quote_does_not_close_a_double_quoted_text() {
        assert_eq!(structure(r#"echo "a \" && b""#), r#"echo "a \" __ b""#);
    }

    #[test]
    fn a_backslash_inside_single_quotes_escapes_nothing() {
        assert_eq!(structure(r"echo 'a\' && ls"), r"echo 'a\' && ls");
    }

    #[test]
    fn a_quoted_file_name_keeps_its_letters() {
        assert_eq!(structure(r#"cat > "my file.py""#), r#"cat > "my file.py""#);
    }

    #[test]
    fn a_heredoc_body_is_not_structure() {
        let command = "git commit -F - <<'EOF'\nfix: a && b\nrun cargo build > log\nEOF";
        assert_eq!(structure(command), "git commit -F - <<'EOF'");
    }

    #[test]
    fn what_follows_a_heredoc_is_structure() {
        let command = "cat <<EOF\nbody && body\nEOF\nls && pwd";
        assert_eq!(structure(command), "cat <<EOF\nls && pwd");
    }

    #[test]
    fn a_chain_on_the_opener_line_is_structure() {
        assert_eq!(structure("cat <<EOF && ls\nbody\nEOF"), "cat <<EOF && ls");
    }

    #[test]
    fn a_dash_heredoc_ends_at_its_indented_delimiter() {
        assert_eq!(structure("cat <<-END\n\tbody | x\n\tEND\nls"), "cat <<-END\nls");
    }

    #[test]
    fn two_heredocs_on_one_line_both_end() {
        let command = "cmd <<A <<B\nfirst && x\nA\nsecond && y\nB\nls";
        assert_eq!(structure(command), "cmd <<A <<B\nls");
    }

    #[test]
    fn a_body_with_an_apostrophe_does_not_reach_what_follows() {
        let command = "cat <<EOF\ndon't && stop\nEOF\necho ok && ls";
        assert_eq!(structure(command), "cat <<EOF\necho ok && ls");
    }

    #[test]
    fn a_here_string_opens_no_heredoc() {
        assert_eq!(structure("grep x <<<word\nls && pwd"), "grep x <<<word\nls && pwd");
    }

    #[test]
    fn an_unterminated_heredoc_takes_the_rest() {
        assert_eq!(structure("cat <<EOF\nbody && body"), "cat <<EOF");
    }

    // ── named_paths ──────────────────────────────────────────────

    fn paths(command: &str) -> Vec<String> {
        named_paths(command, "/work/project", "/home/user")
    }

    #[test]
    fn an_absolute_path_is_named() {
        assert_eq!(paths("cat /etc/hosts"), vec!["/etc/hosts"]);
    }

    #[test]
    fn a_path_under_home_is_resolved() {
        assert_eq!(paths("cat ~/.ssh/id_rsa"), vec!["/home/user/.ssh/id_rsa"]);
    }

    #[test]
    fn a_relative_path_is_under_the_working_directory() {
        assert_eq!(paths("cat src/../../other/x.rs"), vec!["/work/other/x.rs"]);
    }

    #[test]
    fn a_path_is_normalised() {
        assert_eq!(paths("cat /a/tools/../.ssh/id"), vec!["/a/.ssh/id"]);
    }

    #[test]
    fn a_quoted_path_is_named() {
        assert_eq!(paths(r#"cat "/home/user/.aws/credentials""#), vec!["/home/user/.aws/credentials"]);
    }

    #[test]
    fn a_path_after_an_assignment_or_a_redirect_is_named() {
        assert_eq!(paths("KEY=/home/user/.netrc cmd >/tmp/out"), vec!["/home/user/.netrc", "/tmp/out"]);
    }

    #[test]
    fn a_path_inside_a_heredoc_body_is_named() {
        assert_eq!(paths("bash <<EOF\ncat /home/user/.ssh/id\nEOF"), vec!["/home/user/.ssh/id"]);
    }

    #[test]
    fn a_path_through_a_variable_keeps_what_follows_it() {
        assert_eq!(paths("cat $HOME/.ssh/id"), vec!["/work/project/HOME/.ssh/id"]);
    }

    #[test]
    fn a_relative_path_after_cd_is_under_the_new_directory() {
        assert_eq!(paths("cd /n/nornir; git add core/x/src/lib.rs"), vec!["/n/nornir", "/n/nornir/core/x/src/lib.rs"]);
        assert_eq!(paths("cd /n/nornir && cat hooks/h/main.rs"), vec!["/n/nornir", "/n/nornir/hooks/h/main.rs"]);
    }

    #[test]
    fn cd_steps_are_followed_in_order() {
        assert_eq!(
            paths("cd sub\ncat a/b\ncd ..\ncat c/d"),
            vec!["/work/project/sub/a/b", "/work/project/c/d"]
        );
    }

    #[test]
    fn cd_to_home_is_followed() {
        assert_eq!(paths("cd ~; cat .config/x"), vec!["/home/user/.config/x"]);
        assert_eq!(paths("cd ~/code; cat a/b"), vec!["/home/user/code", "/home/user/code/a/b"]);
    }

    #[test]
    fn a_path_before_cd_is_under_the_starting_directory() {
        assert_eq!(paths("cat a/b; cd /elsewhere"), vec!["/work/project/a/b", "/elsewhere"]);
    }

    #[test]
    fn a_relative_path_after_a_stated_directory_is_under_it() {
        assert_eq!(
            paths("git -C /n/nornir add core/x/src/lib.rs hooks/h/main.rs"),
            vec!["/n/nornir", "/n/nornir/core/x/src/lib.rs", "/n/nornir/hooks/h/main.rs"]
        );
        assert_eq!(paths("make -C sub/dir all"), vec!["/work/project/sub/dir"]);
    }

    #[test]
    fn a_stated_directory_holds_for_its_own_command_only() {
        assert_eq!(
            paths("git -C /n add a/b; cat c/d"),
            vec!["/n", "/n/a/b", "/work/project/c/d"]
        );
    }

    #[test]
    fn a_word_that_only_starts_with_cd_moves_nothing() {
        assert_eq!(paths("cdrecord x; cat a/b"), vec!["/work/project/a/b"]);
    }

    #[test]
    fn words_without_a_separator_are_not_paths() {
        assert!(paths("cargo test -p gleipnir_core --quiet").is_empty());
    }
}
