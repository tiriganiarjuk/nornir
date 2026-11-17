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
