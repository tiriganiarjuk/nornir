//! Suppression attribute checks for Rust source files.
//!
//! Check: no_suppression_comments

use crate::parsing::{find_nodes_by_type, node_line, node_text};
use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

use super::prohibited::path_is_test_file;

fn violation(line: usize, message: String) -> Violation {
    Violation {
        line,
        check_name: String::new(),
        severity: Severity::Error,
        message,
        detail: String::new(),
        signal: String::new(),
        direction: String::new(),
        canary: String::new(),
    }
}

// -------------------------------------------------------------------------
// no_suppression_comments
// -------------------------------------------------------------------------

/// Forbidden #[allow(...)] targets that LLMs use to silence compiler feedback.
const FORBIDDEN_ALLOW_TARGETS: &[&str] = &[
    "dead_code",
    "unused",
    "unused_variables",
    "unused_imports",
    "unused_mut",
    "unused_assignments",
    "unreachable_code",
    "non_snake_case",
    "non_camel_case_types",
    "clippy::",
];

/// Detect #[allow(...)] attributes that suppress compiler/clippy warnings.
///
/// Catches:
/// - #[allow(dead_code)]
/// - #[allow(unused_variables)]
/// - #[allow(unused_imports)]
/// - #[allow(unused)]
/// - #[allow(clippy::...)]
/// - #[cfg_attr(not(test), allow(...))]
///
/// Skips:
/// - Attributes inside #[cfg(test)] modules or #[test] functions
/// - #[allow(private_interfaces)] — this is a Rust design pattern, not suppression
pub fn check_no_suppression_comments(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "attribute_item") {
        let text = node_text(node, source.source_bytes);

        // Skip if not an allow attribute
        if !text.contains("allow(") {
            continue;
        }

        // Skip #[allow(private_interfaces)] — legitimate Rust pattern
        if text.contains("private_interfaces") {
            continue;
        }

        // Skip if in test context
        if path_is_test_file(source.file_path) || in_test_context_attr(node, source.source_bytes) {
            continue;
        }

        // Check if any forbidden target is present
        for target in FORBIDDEN_ALLOW_TARGETS {
            if text.contains(target) {
                let desc = if target == &"clippy::" {
                    format!("#[allow(clippy::...)] suppresses clippy lint")
                } else {
                    format!("#[allow({})] suppresses compiler warning", target)
                };
                violations.push(violation(node_line(node), desc));
                break;
            }
        }
    }

    // Also check for line-level suppression comments (rarer in Rust but possible)
    for (line_num, line) in source.lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            // Check for SAFETY or other structured suppression patterns
            // that LLMs abuse
            if trimmed.contains("nolint") || trimmed.contains("no_lint") {
                violations.push(violation(
                    line_num + 1,
                    "suppression comment in Rust source".to_string(),
                ));
            }
        }
    }

    violations
}

/// Check if an attribute_item is inside a test context.
fn in_test_context_attr(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "function_item" => {
                if has_preceding_attribute(parent, source, "test") {
                    return true;
                }
            }
            "mod_item" => {
                if has_preceding_attribute(parent, source, "cfg(test)") {
                    return true;
                }
            }
            _ => {}
        }
        current = parent.parent();
    }
    false
}

/// Check if previous siblings are attribute_items containing target text.
fn has_preceding_attribute(node: tree_sitter::Node, source: &[u8], target: &str) -> bool {
    let mut sibling = node.prev_sibling();
    while let Some(sib) = sibling {
        if sib.kind() == "attribute_item" {
            let text = node_text(sib, source);
            if text.contains(target) {
                return true;
            }
        } else {
            break;
        }
        sibling = sib.prev_sibling();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::build_parsed_source_rust;
    use crate::structures::FileKind;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source_rust("/test/file.rs", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
    }

    #[test]
    fn allow_dead_code_caught() {
        let parsed = parse("#[allow(dead_code)]\nfn unused() {}");
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("dead_code"));
    }

    #[test]
    fn allow_unused_caught() {
        let parsed = parse("#[allow(unused)]\nfn foo() {}");
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("unused"));
    }

    #[test]
    fn allow_unused_variables_caught() {
        let parsed = parse("#[allow(unused_variables)]\nfn foo() { let x = 1; }");
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn allow_clippy_caught() {
        let parsed = parse("#[allow(clippy::needless_return)]\nfn foo() -> i32 { return 1; }");
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("clippy"));
    }

    #[test]
    fn allow_private_interfaces_ok() {
        let parsed = parse("#[allow(private_interfaces)]\npub trait Foo {}");
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn allow_in_test_module_ok() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                #[allow(dead_code)]
                fn helper() {}
            }
            "#,
        );
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn allow_in_test_fn_ok() {
        let parsed = parse(
            r#"
            #[test]
            fn it_works() {
                #[allow(unused_variables)]
                let x = 1;
            }
            "#,
        );
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn allow_outside_test_caught() {
        let parsed = parse(
            r#"
            #[allow(dead_code)]
            fn production() {}

            #[cfg(test)]
            mod tests {
                #[allow(dead_code)]
                fn helper() {}
            }
            "#,
        );
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn derive_attribute_ok() {
        let parsed = parse("#[derive(Debug, Clone)]\nstruct Foo { x: i32 }");
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn no_attributes_ok() {
        let parsed = parse("fn main() { let x = 1; }");
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
