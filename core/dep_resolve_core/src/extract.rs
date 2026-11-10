//! Symbol extraction from Python tree-sitter ASTs.
//!
//! Given a parsed Python file and a list of top-level names, extracts their
//! definitions as either signatures or full bodies.

use tree_sitter::{Node, Tree};

use crate::ts::{node_field, node_text};
use crate::{ResolveMode, SymbolKind};

/// A top-level definition, rendered.
#[derive(Debug, Clone)]
pub struct Definition {
    pub name: String,
    pub kind: SymbolKind,
    pub rendered: String,
}

/// Shared context for symbol extraction — avoids threading 4+ params through every call.
struct ExtractContext<'a> {
    source: &'a [u8],
    mode: ResolveMode,
    names: &'a [String],
    want_all: bool,
}

impl<'a> ExtractContext<'a> {
    fn matches(&self, name: &str) -> bool {
        self.want_all || self.names.iter().any(|n| n == name)
    }
}

/// Extract requested symbols from a parsed Python module.
///
/// If `names` contains `"*"`, extracts all top-level definitions.
pub fn extract_symbols(
    source: &[u8],
    tree: &Tree,
    names: &[String],
    mode: ResolveMode,
) -> Vec<Definition> {
    let exctx = ExtractContext {
        source,
        mode,
        names,
        want_all: names.iter().any(|n| n == "*"),
    };

    let root = tree.root_node();
    let mut results = Vec::new();
    let mut cursor = root.walk();

    for node in root.children(&mut cursor) {
        let (decorators, inner) = unwrap_decorated(node, source);

        let sym = match inner.kind() {
            "function_definition" => {
                try_extract_function(node, inner, &decorators, &exctx)
            }
            "class_definition" => {
                try_extract_class(node, inner, &decorators, &exctx)
            }
            "expression_statement" => {
                try_extract_assignment(inner, &exctx)
            }
            "type_alias_statement" => {
                try_extract_type_alias(inner, &exctx)
            }
            _ => None,
        };

        if let Some(resolved) = sym {
            results.push(resolved);
        }
    }

    results
}

/// Extract all public top-level names from a Python module, in file order.
pub fn extract_all_top_level_names(source: &[u8], tree: &Tree) -> Vec<String> {
    let root = tree.root_node();
    let mut names = Vec::new();
    let mut cursor = root.walk();

    for node in root.children(&mut cursor) {
        let (_, inner) = unwrap_decorated(node, source);
        let name = top_level_name(inner, source);
        if let Some(name) = name {
            if !name.starts_with('_') {
                names.push(name);
            }
        }
    }

    names
}

/// The top-level statement that defines `name`: a function, class, constant
/// or type alias. For a decorated definition the outer node is returned, so
/// the decorators are part of it. Imports are not definitions.
pub fn find_top_level<'a>(tree: &'a Tree, source: &[u8], name: &str) -> Option<Node<'a>> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    let found = root.children(&mut cursor).find(|node| {
        let (_, inner) = unwrap_decorated(*node, source);
        top_level_name(inner, source).as_deref() == Some(name)
    });
    found
}

// ── Node unwrapping ─────────────────────────────────────────────

/// If the node is a decorated_definition, return (decorator_text, inner_def).
/// Otherwise return (empty, node).
fn unwrap_decorated<'a>(node: Node<'a>, source: &[u8]) -> (String, Node<'a>) {
    if node.kind() != "decorated_definition" {
        return (String::new(), node);
    }
    let deco_text = extract_decorators(node, source);
    let inner = node
        .named_children(&mut node.walk())
        .find(|c| is_definition(c.kind()))
        .unwrap_or(node);
    (deco_text, inner)
}

fn is_definition(kind: &str) -> bool {
    matches!(kind, "function_definition" | "class_definition")
}

/// Check if a function_definition node has an `async` keyword child.
fn has_async_keyword(node: Node) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|child| child.kind() == "async");
    found
}

/// Extract the name from a top-level node, if it defines one.
fn top_level_name(node: Node, source: &[u8]) -> Option<String> {
    match node.kind() {
        "function_definition" | "class_definition" => {
            node_field(node, "name").map(|n| node_text(n, source).to_string())
        }
        "expression_statement" => assignment_name(node, source),
        "type_alias_statement" => type_alias_name(node, source),
        _ => None,
    }
}

fn assignment_name(expr_stmt: Node, source: &[u8]) -> Option<String> {
    let mut cursor = expr_stmt.walk();
    for child in expr_stmt.named_children(&mut cursor) {
        if child.kind() != "assignment" {
            continue;
        }
        let left = node_field(child, "left")?;
        if left.kind() == "identifier" {
            return Some(node_text(left, source).to_string());
        }
    }
    None
}

fn type_alias_name(node: Node, source: &[u8]) -> Option<String> {
    let name_node = node.named_children(&mut node.walk()).next()?;
    Some(node_text(name_node, source).to_string())
}

// ── Per-kind extraction ─────────────────────────────────────────

fn try_extract_function(
    outer: Node,
    inner: Node,
    decorators: &str,
    exctx: &ExtractContext,
) -> Option<Definition> {
    let name = node_field(inner, "name")
        .map(|n| node_text(n, exctx.source).to_string())?;
    if !exctx.matches(&name) {
        return None;
    }

    let rendered = match exctx.mode {
        ResolveMode::Signatures => {
            render_bare_signature(inner, exctx.source, decorators)
        }
        ResolveMode::Hybrid | ResolveMode::Full => {
            let full_node = if decorators.is_empty() { inner } else { outer };
            strip_body(full_node, exctx.source)
        }
    };

    Some(Definition {
        name,
        kind: SymbolKind::Function,
        rendered,
    })
}

fn try_extract_class(
    outer: Node,
    inner: Node,
    decorators: &str,
    exctx: &ExtractContext,
) -> Option<Definition> {
    let name = node_field(inner, "name")
        .map(|n| node_text(n, exctx.source).to_string())?;
    if !exctx.matches(&name) {
        return None;
    }

    let rendered = match exctx.mode {
        ResolveMode::Signatures => {
            let mut out = String::new();
            if !decorators.is_empty() {
                out.push_str(decorators);
                out.push('\n');
            }
            out.push_str(&render_class_signature(inner, exctx.source));
            out
        }
        ResolveMode::Hybrid | ResolveMode::Full => {
            let full_node = if decorators.is_empty() { inner } else { outer };
            strip_body(full_node, exctx.source)
        }
    };

    Some(Definition {
        name,
        kind: SymbolKind::Class,
        rendered,
    })
}

fn try_extract_assignment(
    expr_stmt: Node,
    exctx: &ExtractContext,
) -> Option<Definition> {
    let mut cursor = expr_stmt.walk();
    for child in expr_stmt.named_children(&mut cursor) {
        if child.kind() != "assignment" {
            continue;
        }
        let left = node_field(child, "left")?;
        if left.kind() != "identifier" {
            continue;
        }
        let name = node_text(left, exctx.source).to_string();
        if !exctx.matches(&name) {
            continue;
        }
        return Some(Definition {
            name,
            kind: SymbolKind::Constant,
            rendered: node_text(child, exctx.source).to_string(),
        });
    }
    None
}

fn try_extract_type_alias(
    node: Node,
    exctx: &ExtractContext,
) -> Option<Definition> {
    let name = type_alias_name(node, exctx.source)?;
    if !exctx.matches(&name) {
        return None;
    }
    Some(Definition {
        name,
        kind: SymbolKind::TypeAlias,
        rendered: node_text(node, exctx.source).to_string(),
    })
}

// ── Signature rendering ─────────────────────────────────────────

fn render_function_signature(node: Node, source: &[u8]) -> String {
    let is_async = has_async_keyword(node);
    let prefix = if is_async { "async " } else { "" };

    let name = node_field(node, "name")
        .map(|n| node_text(n, source))
        .unwrap_or("?");

    let params = node_field(node, "parameters")
        .map(|n| node_text(n, source))
        .unwrap_or("()");

    let ret = node_field(node, "return_type")
        .map(|n| format!(" -> {}", node_text(n, source)))
        .unwrap_or_default();

    format!("{prefix}def {name}{params}{ret}")
}

fn render_bare_signature(node: Node, source: &[u8], decorators: &str) -> String {
    let sig = render_function_signature(node, source);
    let mut out = String::new();
    if !decorators.is_empty() {
        out.push_str(decorators);
        out.push('\n');
    }
    out.push_str(&sig);
    out
}

/// Strip docstrings and comment lines from a definition body.
///
/// Preserves decorators, the def/class line, and all logic lines.
/// Removes triple-quoted docstrings and lines that are only comments.
fn strip_body(node: Node, source: &[u8]) -> String {
    let raw = node_text(node, source);
    let mut lines: Vec<&str> = Vec::new();
    let mut in_docstring = false;
    let mut docstring_delim = "";

    for line in raw.lines() {
        let trimmed = line.trim();

        // Track triple-quoted docstring boundaries
        if in_docstring {
            if trimmed.ends_with(docstring_delim) || trimmed == docstring_delim {
                in_docstring = false;
            }
            continue;
        }

        if is_docstring_start(trimmed) {
            // Single-line docstring: """text""" or '''text'''
            let delim = &trimmed[..3];
            if trimmed.len() > 6 && trimmed[3..].contains(delim) {
                continue; // whole docstring on one line
            }
            // Multi-line docstring starts here
            in_docstring = true;
            docstring_delim = delim;
            continue;
        }

        // Skip pure comment lines (but keep inline comments on code lines)
        if trimmed.starts_with('#') {
            continue;
        }

        lines.push(line);
    }

    lines.join("\n")
}

fn is_docstring_start(trimmed: &str) -> bool {
    trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''")
}

// ── Class rendering ──────────────────────────────────────────────

fn render_class_signature(node: Node, source: &[u8]) -> String {
    let name = node_field(node, "name")
        .map(|n| node_text(n, source))
        .unwrap_or("?");

    let bases = node_field(node, "superclasses")
        .map(|n| node_text(n, source).to_string())
        .unwrap_or_default();

    let mut lines = vec![format!("class {name}{bases}:")];

    if let Some(body) = node_field(node, "body") {
        collect_class_members(body, source, &mut lines);
    }

    lines.join("\n")
}

fn collect_class_members(body: Node, source: &[u8], lines: &mut Vec<String>) {
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        match child.kind() {
            "expression_statement" => {
                collect_class_field(child, source, lines);
            }
            "type_alias_statement" => {
                lines.push(format!("    {}", node_text(child, source)));
            }
            "function_definition" => {
                let sig = render_function_signature(child, source);
                lines.push(format!("    {sig}"));
            }
            "decorated_definition" => {
                collect_decorated_method(child, source, lines);
            }
            _ => {}
        }
    }
}

fn collect_class_field(expr_stmt: Node, source: &[u8], lines: &mut Vec<String>) {
    let mut cursor = expr_stmt.walk();
    for expr in expr_stmt.named_children(&mut cursor) {
        if expr.kind() == "assignment" {
            lines.push(format!("    {}", node_text(expr, source)));
        }
    }
}

fn collect_decorated_method(decorated: Node, source: &[u8], lines: &mut Vec<String>) {
    let func = decorated
        .named_children(&mut decorated.walk())
        .find(|c| is_definition(c.kind()));
    if let Some(func) = func {
        let sig = render_function_signature(func, source);
        lines.push(format!("    {sig}"));
    }
}

// ── Decorator extraction ────────────────────────────────────────

fn extract_decorators(decorated_node: Node, source: &[u8]) -> String {
    let mut deco_lines = Vec::new();
    let mut cursor = decorated_node.walk();
    for child in decorated_node.children(&mut cursor) {
        if child.kind() == "decorator" {
            deco_lines.push(node_text(child, source).to_string());
        }
    }
    deco_lines.join("\n")
}
