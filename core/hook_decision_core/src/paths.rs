//! Lexical path normalisation. Every path a hook matches is normalised first,
//! so a rule sees where a path leads and never how it was spelled.
//!
//! Lexical: `.` and `..` segments and repeated separators are resolved from the
//! text alone. No file system is consulted and no symbolic link is followed.

/// The path with `.`, `..` and empty segments resolved.
///
/// An absolute path stays absolute; `..` at the root stays at the root. A
/// relative path keeps the `..` segments that lead above its start. A trailing
/// separator is kept, because a rule may match a directory by it.
pub fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut kept: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => step_up(&mut kept, absolute),
            name => kept.push(name),
        }
    }
    let body = kept.join("/");
    let trailing = if path.ends_with('/') && !body.is_empty() { "/" } else { "" };
    let leading = if absolute { "/" } else { "" };
    format!("{leading}{body}{trailing}")
}

fn step_up<'a>(kept: &mut Vec<&'a str>, absolute: bool) {
    match kept.last() {
        Some(&last) if last != ".." => {
            kept.pop();
        }
        _ if absolute => {}
        _ => kept.push(".."),
    }
}

/// The path as an absolute, normalised path: itself when it is absolute, else
/// joined to `base`.
pub fn absolute_from(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        normalize(path)
    } else {
        normalize(&format!("{base}/{path}"))
    }
}

/// A normalised path as a protected-path rule is matched against it: in lower
/// case, because the volumes here do not tell capitals apart, and ending in a
/// separator, so a directory named without one is still that directory.
pub fn for_matching(path: &str) -> String {
    let lowered = path.to_lowercase();
    if lowered.ends_with('/') {
        lowered
    } else {
        format!("{lowered}/")
    }
}

/// Characters that make a path segment a pattern.
const WILDCARDS: [char; 3] = ['*', '?', '['];

/// A path that holds pattern segments, rewritten with a protected name where
/// its segments can match one: `/home/u/.s*/id_*` against `/.ssh/` gives
/// `/home/u/.ssh/id_*/`. `None` when no pattern segment of the path can match
/// the name.
///
/// Both are given as `for_matching` gives a path. A pattern segment counts
/// only when it also holds a character of its own: a bare `*` or `**` names
/// nothing, and is every listing of every directory.
pub fn with_pattern_as(path: &str, protected: &str) -> Option<String> {
    let path_segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
    let protected_segments: Vec<&str> = protected.split('/').filter(|segment| !segment.is_empty()).collect();
    let last_is_a_prefix = !protected.ends_with('/');
    let width = protected_segments.len();
    if width == 0 || width > path_segments.len() {
        return None;
    }
    (0..=path_segments.len() - width).find_map(|start| {
        let window = &path_segments[start..start + width];
        let names_a_pattern = window.iter().any(|segment| is_naming_pattern(segment));
        let every_segment_fits = window.iter().zip(&protected_segments).enumerate().all(|(index, (segment, name))| {
            segment_fits(segment, name, last_is_a_prefix && index + 1 == width)
        });
        (names_a_pattern && every_segment_fits).then(|| {
            let rewritten: Vec<&str> = path_segments[..start]
                .iter()
                .chain(&protected_segments)
                .chain(&path_segments[start + width..])
                .copied()
                .collect();
            format!("/{}/", rewritten.join("/"))
        })
    })
}

/// A pattern segment that holds at least one character of its own.
fn is_naming_pattern(segment: &str) -> bool {
    segment.contains(WILDCARDS) && segment.chars().any(|character| !WILDCARDS.contains(&character) && character != ']')
}

/// Whether a path segment is, or as a pattern can match, a protected name —
/// or, where the name is a prefix, a name that starts with it.
fn segment_fits(segment: &str, name: &str, name_is_a_prefix: bool) -> bool {
    if !segment.contains(WILDCARDS) {
        return segment == name || (name_is_a_prefix && segment.starts_with(name));
    }
    if !is_naming_pattern(segment) {
        return false;
    }
    let stated_start = segment.split(WILDCARDS).next().unwrap_or("");
    pattern_matches(segment, name) || (name_is_a_prefix && stated_start.starts_with(name))
}

/// Whether a pattern of `*` and `?` matches a name whole. A pattern with a
/// bracket class is taken to match: what a class holds is not read here.
fn pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern.contains('[') {
        return true;
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    matches_from(&pattern, &name)
}

fn matches_from(pattern: &[char], name: &[char]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((&'*', rest)) => (0..=name.len()).any(|taken| matches_from(rest, &name[taken..])),
        Some((&'?', rest)) => !name.is_empty() && matches_from(rest, &name[1..]),
        Some((stated, rest)) => name.first() == Some(stated) && matches_from(rest, &name[1..]),
    }
}

/// Whether a normalised path lies at or under a normalised directory.
pub fn is_under(path: &str, directory: &str) -> bool {
    let directory = directory.trim_end_matches('/');
    match path.strip_prefix(directory) {
        Some(rest) => rest.is_empty() || rest.starts_with('/'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_is_unchanged() {
        assert_eq!(normalize("/a/b/c.rs"), "/a/b/c.rs");
    }

    #[test]
    fn parent_segments_are_resolved() {
        assert_eq!(normalize("/a/tools/../.ssh/id"), "/a/.ssh/id");
        assert_eq!(normalize("/a/b/../../c"), "/c");
    }

    #[test]
    fn current_and_empty_segments_are_dropped() {
        assert_eq!(normalize("/a/./b//c"), "/a/b/c");
    }

    #[test]
    fn the_root_has_no_parent() {
        assert_eq!(normalize("/../../etc"), "/etc");
        assert_eq!(normalize("/.."), "/");
    }

    #[test]
    fn a_relative_path_keeps_what_leads_above_it() {
        assert_eq!(normalize("../../x"), "../../x");
        assert_eq!(normalize("a/../../x"), "../x");
        assert_eq!(normalize("a/b/../c"), "a/c");
    }

    #[test]
    fn a_trailing_separator_is_kept() {
        assert_eq!(normalize("/a/b/"), "/a/b/");
        assert_eq!(normalize("/a/b/../"), "/a/");
    }

    #[test]
    fn a_relative_path_is_joined_to_its_base() {
        assert_eq!(absolute_from("/work/project", "src/../../other/x"), "/work/other/x");
        assert_eq!(absolute_from("/work/project/", "x"), "/work/project/x");
    }

    #[test]
    fn an_absolute_path_ignores_the_base() {
        assert_eq!(absolute_from("/work", "/etc/../x"), "/x");
    }

    #[test]
    fn a_path_is_matched_in_lower_case_and_as_a_directory() {
        assert_eq!(for_matching("/home/U/.SSH"), "/home/u/.ssh/");
        assert_eq!(for_matching("/home/u/.ssh/"), "/home/u/.ssh/");
        assert_eq!(for_matching("/home/u/.netrc"), "/home/u/.netrc/");
    }

    #[test]
    fn a_pattern_that_can_match_a_protected_name_is_rewritten_as_it() {
        assert_eq!(with_pattern_as("/home/u/.s*/id_*/", "/.ssh/").as_deref(), Some("/home/u/.ssh/id_*/"));
        assert_eq!(with_pattern_as("/home/u/.ss?/", "/.ssh/").as_deref(), Some("/home/u/.ssh/"));
        assert_eq!(with_pattern_as("/home/u/*ssh/", "/.ssh/").as_deref(), Some("/home/u/.ssh/"));
        assert_eq!(with_pattern_as("/home/u/.[a-z]sh/x/", "/.ssh/").as_deref(), Some("/home/u/.ssh/x/"));
    }

    #[test]
    fn a_pattern_can_match_a_name_of_several_segments() {
        assert_eq!(with_pattern_as("/home/u/.k*/config/", "/.kube/config").as_deref(), Some("/home/u/.kube/config/"));
        assert_eq!(with_pattern_as("/home/u/.kube/con*/", "/.kube/config").as_deref(), Some("/home/u/.kube/config/"));
        assert_eq!(with_pattern_as("/home/u/.docker/c*.json/", "/.docker/config.json").as_deref(), Some("/home/u/.docker/config.json/"));
    }

    #[test]
    fn a_pattern_can_match_a_name_that_is_a_prefix() {
        assert!(with_pattern_as("/u/.claude/sett*/", "/.claude/settings").is_some());
        assert!(with_pattern_as("/u/.claude/settings.j*/", "/.claude/settings").is_some());
        assert!(with_pattern_as("/n/hooks/hook_pre_l*/src/", "/hook_pre_llm").is_some());
    }

    #[test]
    fn a_pattern_that_cannot_match_the_name_is_left() {
        assert!(with_pattern_as("/home/u/.a*/x/", "/.ssh/").is_none());
        assert!(with_pattern_as("/home/u/src/*.rs/", "/.ssh/").is_none());
        assert!(with_pattern_as("/home/u/.ssh?/x/", "/.ssh/").is_none());
        assert!(with_pattern_as("/home/u/.kube/*.yaml/", "/.kube/config").is_none());
    }

    #[test]
    fn a_bare_wildcard_names_nothing() {
        assert!(with_pattern_as("/work/project/*/", "/.ssh/").is_none());
        assert!(with_pattern_as("/work/project/**/*.rs/", "/.ssh/").is_none());
        assert!(with_pattern_as("/work/project/?/", "/.ssh/").is_none());
    }

    #[test]
    fn a_path_without_a_pattern_is_left() {
        assert!(with_pattern_as("/home/u/.ssh/id_rsa/", "/.ssh/").is_none());
        assert!(with_pattern_as("/home/u/x/", "/.ssh/").is_none());
        assert!(with_pattern_as("/", "/.ssh/").is_none());
    }

    #[test]
    fn under_means_at_or_below() {
        assert!(is_under("/a/b/c", "/a/b"));
        assert!(is_under("/a/b/c", "/a/b/"));
        assert!(is_under("/a/b", "/a/b/"));
    }

    #[test]
    fn a_sibling_with_the_same_start_is_not_under() {
        assert!(!is_under("/a/bc/d", "/a/b"));
        assert!(!is_under("/a", "/a/b"));
    }

    #[test]
    fn a_path_that_climbs_out_is_not_under_once_normalised() {
        let climbed = normalize("/a/tools/../../etc/passwd");
        assert!(!is_under(&climbed, "/a/tools/"));
    }
}
