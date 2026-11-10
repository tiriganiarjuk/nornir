//! Tree-sitter primitives for Python AST traversal.
//!
//! Minimal set of helpers needed for dependency resolution.
//! These are intentionally self-contained — no dependency on gleipnir_core.

use tree_sitter::{Node, Parser, Tree};

/// Parse Python source bytes into a tree-sitter Tree.
pub fn parse_python(source_bytes: &[u8]) -> Result<Tree, String> {
    let mut parser = Parser::new();
    let language = tree_sitter_python::LANGUAGE;
    parser
        .set_language(&language.into())
        .map_err(|e| format!("failed to set Python language: {e}"))?;
    parser
        .parse(source_bytes, None)
        .ok_or_else(|| "tree-sitter parse returned None".to_string())
}

/// Extract UTF-8 text from a node's byte range.
pub fn node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("")
}

/// Get a named field child from a node.
pub fn node_field<'a>(node: Node<'a>, field: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field)
}

/// Depth-first search for all nodes of a specific kind.
pub fn find_nodes_by_type<'a>(root: Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut results = Vec::new();
    collect_by_type(root, kind, &mut results);
    results
}

fn collect_by_type<'a>(node: Node<'a>, kind: &str, results: &mut Vec<Node<'a>>) {
    if node.kind() == kind {
        results.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_by_type(child, kind, results);
    }
}

/// Extract module path and relative import level from an import_from_statement.
///
/// Returns (module_path, level) where level > 0 means relative import.
pub fn extract_module_info(node: Node, source: &[u8]) -> (String, usize) {
    let module_node = match node.child_by_field_name("module_name") {
        Some(n) => n,
        None => return (String::new(), 0),
    };

    let full_text = node_text(module_node, source);

    if module_node.kind() == "relative_import" {
        let level = full_text.chars().take_while(|&c| c == '.').count();
        return (full_text[level..].to_string(), level);
    }

    (full_text.to_string(), 0)
}

/// A name bound by `from x import …`: the local name and the name in `x`.
///
/// `from x import a` binds `a` → `a`; `from x import a as b` binds `b` → `a`;
/// `from x import *` binds `*` → `*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    pub local: String,
    pub original: String,
}

/// The names bound by an import_from_statement, in source order.
pub fn extract_import_bindings(node: Node, source: &[u8]) -> Vec<ImportBinding> {
    let mut bindings = Vec::new();
    let mut past_import = false;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "import" {
            past_import = true;
            continue;
        }
        if !past_import {
            continue;
        }
        match child.kind() {
            "dotted_name" => {
                let name = node_text(child, source).to_string();
                bindings.push(ImportBinding { local: name.clone(), original: name });
            }
            "aliased_import" => {
                let original = child.child_by_field_name("name").map(|n| node_text(n, source));
                let alias = child.child_by_field_name("alias").map(|n| node_text(n, source));
                if let (Some(original), Some(alias)) = (original, alias) {
                    bindings.push(ImportBinding {
                        local: alias.to_string(),
                        original: original.to_string(),
                    });
                }
            }
            "wildcard_import" => {
                bindings.push(ImportBinding { local: "*".to_string(), original: "*".to_string() });
            }
            _ => {}
        }
    }
    bindings
}

/// The dotted module names of a plain `import a.b, c as d` statement.
pub fn extract_plain_import_modules(node: Node, source: &[u8]) -> Vec<String> {
    let mut modules = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => modules.push(node_text(child, source).to_string()),
            "aliased_import" => {
                if let Some(name) = child.child_by_field_name("name") {
                    modules.push(node_text(name, source).to_string());
                }
            }
            _ => {}
        }
    }
    modules
}
