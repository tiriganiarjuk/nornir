//! Type system enforcement checks.
//!
//! Checks: no_object, no_json_value, no_any_types, no_any_type_aliases,
//! no_bare_collections, union_member_count, no_callable_params,
//! no_implicit_type_aliases.

use crate::parsing::{
    annotation_contains_name, bare_names_in_annotation, collect_union_leaves,
    count_union_members, find_nodes_by_type, find_type_annotations, names_for, node_field, node_line,
    node_text, walk_tree,
};
use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

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

/// The modules `object` is imported from under another name.
const OBJECT_MODULES: &[&str] = &["builtins"];
/// The modules `JsonValue` is imported from.
const JSON_VALUE_MODULES: &[&str] = &["pydantic", "pydantic.types"];

pub fn check_no_object(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let root = source.tree.root_node();
    let object_names = names_for(root, source.source_bytes, "object", OBJECT_MODULES);
    let mut violations = Vec::new();

    for type_node in find_type_annotations(root, source.source_bytes) {
        if contains_any_of(type_node, &object_names, source.source_bytes) {
            let text = node_text(type_node, source.source_bytes);
            violations.push(violation(
                node_line(type_node),
                format!("object in type annotation: {text}"),
            ));
        }
    }
    violations
}

pub fn check_no_json_value(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let root = source.tree.root_node();
    let json_value_names = names_for(root, source.source_bytes, "JsonValue", JSON_VALUE_MODULES);
    let mut violations = Vec::new();

    for type_node in find_type_annotations(root, source.source_bytes) {
        if contains_any_of(type_node, &json_value_names, source.source_bytes) {
            let text = node_text(type_node, source.source_bytes);
            violations.push(violation(
                node_line(type_node),
                format!("JsonValue in type annotation: {text}"),
            ));
        }
    }
    violations
}

/// The modules `Any` is imported from.
const ANY_MODULES: &[&str] = &["typing", "typing_extensions"];
/// The modules `Callable` is imported from.
const CALLABLE_MODULES: &[&str] = &["typing", "typing_extensions", "collections.abc"];

/// Whether a type node holds any of the names a file uses for one type.
fn contains_any_of(type_node: tree_sitter::Node, names: &[String], source: &[u8]) -> bool {
    names.iter().any(|name| annotation_contains_name(type_node, name, source))
}

pub fn check_no_any_types(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let root = source.tree.root_node();
    let any_names = names_for(root, source.source_bytes, "Any", ANY_MODULES);
    let mut violations = Vec::new();

    for type_node in find_type_annotations(root, source.source_bytes) {
        if contains_any_of(type_node, &any_names, source.source_bytes) {
            let text = node_text(type_node, source.source_bytes);
            violations.push(violation(
                node_line(type_node),
                format!("Any in type annotation: {text}"),
            ));
        }
    }
    violations
}

/// Collection types that say nothing about their contents when written bare: the
/// builtins, and the typing and collections.abc names for the same collections.
static BARE_COLLECTION_NAMES: &[&str] = &[
    "dict", "list", "set", "frozenset", "tuple", "Dict", "List", "Set", "FrozenSet", "Tuple", "Mapping", "Sequence",
];

pub fn check_no_bare_collections(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        for (line, name) in
            bare_names_in_annotation(type_node, BARE_COLLECTION_NAMES, source.source_bytes)
        {
            violations.push(violation(line, format!("bare {name} in type annotation")));
        }
    }
    violations
}

/// Python's builtin type names: the one list for both checks that ask whether a
/// name is a builtin type. `union_member_count` counts a union member with one of
/// these names as "simple", bare or parameterized (list[int], dict[str, float]);
/// `no_implicit_type_aliases` takes one as a type-shaped operand of a `|` alias.
const BUILTIN_TYPE_NAMES: &[&str] = &[
    "int", "str", "float", "bool", "bytes", "complex", "bytearray", "memoryview",
    "list", "dict", "tuple", "set", "frozenset", "object", "type",
];

/// Classify whether a union leaf node is a simple (builtin) type.
///
/// Returns true for bare builtins (`int`) and parameterized builtins (`list[int]`).
/// None is handled separately by the caller, not here.
fn is_simple_type(node: tree_sitter::Node, source: &[u8]) -> bool {
    match node.kind() {
        "identifier" => {
            let text = node_text(node, source);
            BUILTIN_TYPE_NAMES.contains(&text.as_ref())
        }
        // tree-sitter uses "generic_type" for parameterized types in annotations (list[int])
        // and "subscript" in expression context
        "generic_type" | "subscript" => {
            // Check if the base identifier is a simple/builtin name
            // generic_type: first named child is the identifier
            // subscript: "value" field is the identifier
            let base = node.named_child(0)
                .or_else(|| node.child_by_field_name("value"));
            if let Some(base_node) = base {
                if base_node.kind() == "identifier" {
                    let text = node_text(base_node, source);
                    return BUILTIN_TYPE_NAMES.contains(&text.as_ref());
                }
            }
            false
        }
        _ => false,
    }
}

/// Check if a union leaf node is None.
fn is_none_type(node: tree_sitter::Node, source: &[u8]) -> bool {
    node.kind() == "none" || (node.kind() == "identifier" && node_text(node, source) == "None")
}

/// Check if a node is the root of a union (binary_operator with | OR union_type).
fn is_union_root(node: tree_sitter::Node) -> bool {
    match node.kind() {
        "union_type" => {
            // Skip nested union_type (shouldn't happen but be safe)
            if let Some(parent) = node.parent() {
                if parent.kind() == "union_type" {
                    return false;
                }
            }
            true
        }
        "binary_operator" => {
            // Skip if parent is also binary_operator (not the outermost)
            if let Some(parent) = node.parent() {
                if parent.kind() == "binary_operator" {
                    return false;
                }
            }
            count_union_members(node) > 1
        }
        _ => false,
    }
}

/// Find top-level union nodes within an annotation subtree.
/// Returns (line, simple_count, named_count) for each union root.
///
/// Handles both binary_operator chains (simple type unions) and
/// union_type nodes (tree-sitter uses this for generic/parameterized unions).
fn find_union_roots(
    type_node: tree_sitter::Node,
    source: &[u8],
) -> Vec<(usize, usize, usize)> {
    let mut results = Vec::new();

    for node in walk_tree(type_node) {
        if !is_union_root(node) {
            continue;
        }
        let leaves = collect_union_leaves(node);
        let mut simple_count = 0usize;
        let mut named_count = 0usize;
        for leaf in &leaves {
            if is_none_type(*leaf, source) {
                continue; // None is always free
            } else if is_simple_type(*leaf, source) {
                simple_count += 1;
            } else {
                named_count += 1;
            }
        }
        if simple_count + named_count > 1 {
            results.push((node_line(node), simple_count, named_count));
        }
    }
    results
}

pub fn check_union_member_count(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        for (line, simple_count, named_count) in find_union_roots(type_node, source.source_bytes) {
            if simple_count > config.max_simple_union_members {
                violations.push(violation(
                    line,
                    "union has too many simple types".to_string(),
                ));
            }
            if named_count > config.max_named_union_members {
                violations.push(violation(
                    line,
                    "union has too many named types".to_string(),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_callable_params
// -------------------------------------------------------------------------

/// Detect Callable in function parameter type annotations.
///
/// Callable parameters launder runtime dependencies through function arguments,
/// bypassing the static import graph that gleipnir enforces. If a function needs
/// to call another function, it must import it directly.
pub fn check_no_callable_params(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let root = source.tree.root_node();
    let callable_names = names_for(root, source.source_bytes, "Callable", CALLABLE_MODULES);
    let mut violations = Vec::new();

    for func in find_nodes_by_type(root, "function_definition") {
        let params = match node_field(func, "parameters") {
            Some(p) => p,
            None => continue,
        };
        let mut cursor = params.walk();
        for param in params.named_children(&mut cursor) {
            // typed_parameter, typed_default_parameter
            let type_node = node_field(param, "type");
            let type_node = match type_node {
                Some(t) => t,
                None => continue,
            };
            if contains_any_of(type_node, &callable_names, source.source_bytes) {
                let param_name = node_field(param, "name")
                    .map(|n| node_text(n, source.source_bytes))
                    .unwrap_or_else(|| node_text(param, source.source_bytes));
                violations.push(violation(
                    node_line(param),
                    format!("parameter '{param_name}' has Callable type annotation"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_callable_type_aliases
// -------------------------------------------------------------------------

/// Detect type aliases that wrap Callable, laundering callable-passing behind
/// a clean name that `no_callable_params` cannot see through.
pub fn check_no_callable_type_aliases(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let root = source.tree.root_node();
    let callable_names = names_for(root, source.source_bytes, "Callable", CALLABLE_MODULES);
    let mut violations = Vec::new();

    for node in find_nodes_by_type(root, "type_alias_statement") {
        let mut cursor = node.walk();
        let type_children: Vec<_> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "type")
            .collect();

        if type_children.len() < 2 {
            continue;
        }

        let name_node = type_children[0];
        let value_node = match type_children.last() {
            Some(node) => *node,
            None => continue,
        };

        let alias_name = name_node
            .named_child(0)
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");

        if contains_any_of(value_node, &callable_names, source.source_bytes) {
            violations.push(callable_alias_violation(node, alias_name));
        }
    }
    for assignment in module_level_assignments(root) {
        let (Some(left), Some(right)) = (node_field(assignment, "left"), node_field(assignment, "right")) else {
            continue;
        };
        if left.kind() == "identifier" && contains_any_of(right, &callable_names, source.source_bytes) {
            violations.push(callable_alias_violation(assignment, node_text(left, source.source_bytes)));
        }
    }
    violations
}

fn callable_alias_violation(node: tree_sitter::Node, alias_name: &str) -> Violation {
    violation(
        node_line(node),
        format!("type alias '{alias_name}' wraps Callable — callables must not be passed as arguments"),
    )
}

/// The assignments a module makes at its top level: `Name = value` and `Name: T = value`.
fn module_level_assignments(root: tree_sitter::Node) -> Vec<tree_sitter::Node> {
    let mut cursor = root.walk();
    let assignments = root.named_children(&mut cursor).filter_map(statement_assignment).collect();
    assignments
}

// -------------------------------------------------------------------------
// no_any_type_aliases
// -------------------------------------------------------------------------

pub fn check_no_any_type_aliases(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let root = source.tree.root_node();
    let any_names = names_for(root, source.source_bytes, "Any", ANY_MODULES);
    let mut violations = Vec::new();

    for node in find_nodes_by_type(root, "type_alias_statement") {
        // Extract alias name (second named child of kind "type" wrapping an identifier)
        // and value (last named child of kind "type")
        let mut cursor = node.walk();
        let type_children: Vec<_> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "type")
            .collect();

        // Need at least 2: name wrapper and value wrapper
        if type_children.len() < 2 {
            continue;
        }

        let name_node = type_children[0];
        let value_node = match type_children.last() {
            Some(node) => *node,
            None => continue,
        };

        let alias_name = name_node
            .named_child(0)
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");

        if contains_any_of(value_node, &any_names, source.source_bytes) {
            violations.push(violation(
                node_line(node),
                format!("type alias '{alias_name}' launders Any — use Any directly so type holes cluster visibly"),
            ));
        }
    }
    violations
}

/// Classify a node as a type alias value.
/// Returns alias type name or empty string.
fn is_type_alias_value(node: tree_sitter::Node, source: &[u8]) -> &'static str {
    if node.kind() == "subscript" {
        if let Some(value) = node.child_by_field_name("value") {
            if value.kind() == "identifier" {
                match node_text(value, source) {
                    "Literal" => return "Literal",
                    "dict" => return "dict",
                    "list" => return "list",
                    "set" => return "set",
                    "tuple" => return "tuple",
                    _ => {}
                }
            }
        }
    }
    if is_type_union(node, source) {
        return "Union";
    }
    ""
}

/// CapWords: starts uppercase and contains at least one lowercase letter.
fn is_cap_words(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
        && name.contains(|c: char| c.is_ascii_lowercase())
}

/// Check if a binary_operator node's operator is `|`.
fn has_pipe_operator(node: tree_sitter::Node) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|child| !child.is_named() && child.kind() == "|");
    found
}

/// Check if an operand of `|` names a type: None, a builtin type, a subscript
/// (`list[int]`), a CapWords name, an attribute ending in CapWords, or a
/// nested `|` of such operands.
fn is_type_shaped_operand(node: tree_sitter::Node, source: &[u8]) -> bool {
    match node.kind() {
        "none" | "subscript" => true,
        "identifier" => {
            let name = node_text(node, source);
            name == "None" || BUILTIN_TYPE_NAMES.contains(&name) || is_cap_words(name)
        }
        "attribute" => node
            .child_by_field_name("attribute")
            .map(|segment| is_cap_words(node_text(segment, source)))
            .unwrap_or(false),
        "binary_operator" => is_type_union(node, source),
        _ => false,
    }
}

/// Check if a node is a `|` expression whose every operand is type-shaped.
/// A `|` between values (sets, dicts, flags, constants) is an operation, not an alias.
fn is_type_union(node: tree_sitter::Node, source: &[u8]) -> bool {
    if node.kind() != "binary_operator" || !has_pipe_operator(node) {
        return false;
    }
    let left = node.child_by_field_name("left");
    let right = node.child_by_field_name("right");
    match (left, right) {
        (Some(left_operand), Some(right_operand)) => {
            is_type_shaped_operand(left_operand, source)
                && is_type_shaped_operand(right_operand, source)
        }
        _ => false,
    }
}

pub fn check_no_implicit_type_aliases(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for stmt in root.named_children(&mut cursor) {
        let assign = if stmt.kind() == "expression_statement" {
            let mut inner_cursor = stmt.walk();
            let first_child = stmt.named_children(&mut inner_cursor).next();
            match first_child {
                Some(child) if child.kind() == "assignment" => Some(child),
                _ => None,
            }
        } else if stmt.kind() == "assignment" {
            Some(stmt)
        } else {
            None
        };

        let assign = match assign {
            Some(a) => a,
            None => continue,
        };

        let left = match node_field(assign, "left") {
            Some(n) if n.kind() == "identifier" => n,
            _ => continue,
        };
        let right = match node_field(assign, "right") {
            Some(n) => n,
            None => continue,
        };

        let alias_type = is_type_alias_value(right, source.source_bytes);
        if !alias_type.is_empty() {
            let name = node_text(left, source.source_bytes);
            violations.push(violation(
                node_line(assign),
                format!("implicit type alias {name} = {alias_type}[...]"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_string_annotations — string forward references in type positions
// -------------------------------------------------------------------------

/// The name a generic is written on: `Literal` of `Literal["a"]`, `Annotated` of
/// `typing.Annotated[str, ...]`.
fn generic_base_name<'a>(generic: tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    let base = match generic.kind() {
        "generic_type" => generic.named_child(0)?,
        "subscript" => generic.child_by_field_name("value")?,
        _ => return None,
    };
    match base.kind() {
        "identifier" => Some(node_text(base, source)),
        "attribute" => base.child_by_field_name("attribute").map(|segment| node_text(segment, source)),
        _ => None,
    }
}

/// Whether a string inside an annotation is a value rather than a type: a member of
/// `Literal[...]`, or a note in `Annotated[T, ...]` after its first argument.
fn is_value_string(string: tree_sitter::Node, annotation: tree_sitter::Node, source: &[u8]) -> bool {
    let mut previous = string;
    let mut current = string.parent();
    while let Some(node) = current {
        let (generic, first_argument) = match node.kind() {
            "type_parameter" => (node.parent().filter(|parent| parent.kind() == "generic_type"), node.named_child(0)),
            "subscript" => (Some(node), node.child_by_field_name("subscript")),
            _ => (None, None),
        };
        match generic.and_then(|generic| generic_base_name(generic, source)) {
            Some("Literal") => return true,
            Some("Annotated") if first_argument.map(|first| first.id()) != Some(previous.id()) => return true,
            _ => {}
        }
        if node.id() == annotation.id() {
            break;
        }
        previous = node;
        current = node.parent();
    }
    false
}

/// The first string anywhere in an annotation that stands for a type (a forward
/// reference), if there is one.
fn is_string_type_node<'a>(node: tree_sitter::Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    find_nodes_by_type(node, "string")
        .into_iter()
        .find(|string| !is_value_string(*string, node, source))
        .map(|string| node_text(string, source))
}

/// Detect string annotations used as forward references.
///
/// `-> "SomeType"` or `param: "SomeType"` — the type is a string literal
/// instead of an actual type reference. This hides the real dependency
/// and exists because the import was deferred or missing.
pub fn check_no_string_annotations(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    // Return type annotations: def foo() -> "Bar"
    for func in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        if let Some(ret) = node_field(func, "return_type") {
            if let Some(text) = is_string_type_node(ret, source.source_bytes) {
                violations.push(violation(
                    node_line(func),
                    format!("string annotation {text} — use a real type reference"),
                ));
            }
        }
    }

    // Parameter type annotations: def foo(x: "Bar")
    for kind in &["typed_parameter", "typed_default_parameter"] {
        for param in find_nodes_by_type(source.tree.root_node(), kind) {
            if let Some(type_node) = node_field(param, "type") {
                if let Some(text) = is_string_type_node(type_node, source.source_bytes) {
                    violations.push(violation(
                        node_line(param),
                        format!("string annotation {text} — use a real type reference"),
                    ));
                }
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_open_models — a model that accepts undeclared keys, or declares none
// -------------------------------------------------------------------------

/// The content of a plain string literal, without quotes or prefix. None for
/// an f-string and for a node that is not a string.
fn plain_string_content<'a>(node: tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    if node.kind() != "string" {
        return None;
    }
    let mut content = "";
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "interpolation" => return None,
            "string_content" => content = node_text(child, source),
            _ => {}
        }
    }
    Some(content)
}

/// Whether a value switches undeclared keys on: the literal `"allow"`, or a
/// name that ends in `.allow`.
fn allows_undeclared_keys(value: tree_sitter::Node, source: &[u8]) -> bool {
    plain_string_content(value, source) == Some("allow") || node_text(value, source).ends_with(".allow")
}

/// Whether an argument is the keyword `extra=` with a value that allows.
fn is_extra_allow_keyword(argument: tree_sitter::Node, source: &[u8]) -> bool {
    if argument.kind() != "keyword_argument" {
        return false;
    }
    let named_extra = node_field(argument, "name").is_some_and(|name| node_text(name, source) == "extra");
    named_extra && node_field(argument, "value").is_some_and(|value| allows_undeclared_keys(value, source))
}

/// Whether a dict pair is `"extra":` with a value that allows.
fn is_extra_allow_pair(pair: tree_sitter::Node, source: &[u8]) -> bool {
    let keyed_extra = node_field(pair, "key").is_some_and(|key| plain_string_content(key, source) == Some("extra"));
    keyed_extra && node_field(pair, "value").is_some_and(|value| allows_undeclared_keys(value, source))
}

/// The assignment a statement holds, when it holds one.
fn statement_assignment(statement: tree_sitter::Node) -> Option<tree_sitter::Node> {
    if statement.kind() != "expression_statement" {
        return None;
    }
    statement.named_child(0).filter(|inner| inner.kind() == "assignment")
}

/// Whether an assignment's target is the given name.
fn assigns_to(assignment: tree_sitter::Node, target: &str, source: &[u8]) -> bool {
    node_field(assignment, "left").is_some_and(|left| node_text(left, source) == target)
}

/// `class Model(BaseModel, extra="allow")` — the switch in the class line.
fn opened_in_class_line(class_node: tree_sitter::Node, source: &[u8]) -> bool {
    let Some(bases) = node_field(class_node, "superclasses") else {
        return false;
    };
    let mut cursor = bases.walk();
    let found = bases.named_children(&mut cursor).any(|argument| is_extra_allow_keyword(argument, source));
    found
}

/// `model_config = ConfigDict(extra="allow")` or `model_config = {"extra": "allow"}`.
fn opened_in_model_config(assignment: tree_sitter::Node, source: &[u8]) -> bool {
    if !assigns_to(assignment, "model_config", source) {
        return false;
    }
    let Some(right) = node_field(assignment, "right") else {
        return false;
    };
    let by_keyword = find_nodes_by_type(right, "keyword_argument")
        .into_iter()
        .any(|argument| is_extra_allow_keyword(argument, source));
    let by_pair = find_nodes_by_type(right, "pair").into_iter().any(|pair| is_extra_allow_pair(pair, source));
    by_keyword || by_pair
}

/// A nested `class Config:` holding `extra = "allow"`.
fn opened_in_config_class(inner_class: tree_sitter::Node, source: &[u8]) -> bool {
    let is_config = node_field(inner_class, "name").is_some_and(|name| node_text(name, source) == "Config");
    let Some(body) = node_field(inner_class, "body") else {
        return false;
    };
    let mut cursor = body.walk();
    let found = body.named_children(&mut cursor).filter_map(statement_assignment).any(|assignment| {
        assigns_to(assignment, "extra", source)
            && node_field(assignment, "right").is_some_and(|right| allows_undeclared_keys(right, source))
    });
    is_config && found
}

/// Whether a class body switches undeclared keys on.
fn body_opens_model(body: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = body.walk();
    let found = body.named_children(&mut cursor).any(|statement| match statement.kind() {
        "class_definition" => opened_in_config_class(statement, source),
        _ => statement_assignment(statement).is_some_and(|assignment| opened_in_model_config(assignment, source)),
    });
    found
}

/// Whether a class body declares a field: an annotated name other than `model_config`.
fn body_declares_field(body: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = body.walk();
    let found = body.named_children(&mut cursor).filter_map(statement_assignment).any(|assignment| {
        node_field(assignment, "type").is_some() && !assigns_to(assignment, "model_config", source)
    });
    found
}

/// Detect a model that accepts keys it does not declare, and a model written
/// directly on `BaseModel` that declares no field.
///
/// Either one is `dict[str, Any]` with a base class: what it holds is whatever
/// arrived.
pub fn check_no_open_models(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let bytes = source.source_bytes;
    let mut violations = Vec::new();

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let Some(body) = node_field(class_node, "body") else {
            continue;
        };
        let open = opened_in_class_line(class_node, bytes) || body_opens_model(body, bytes);
        let on_base_model = node_field(class_node, "superclasses")
            .is_some_and(|bases| annotation_contains_name(bases, "BaseModel", bytes));
        let fieldless = on_base_model && !body_declares_field(body, bytes);
        let finding = match (open, fieldless) {
            (true, true) => "accepts undeclared keys and declares no fields",
            (true, false) => "accepts undeclared keys",
            (false, true) => "declares no fields",
            (false, false) => continue,
        };
        let class_name = node_field(class_node, "name").map(|name| node_text(name, bytes)).unwrap_or("<unknown>");
        violations.push(violation(node_line(class_node), format!("model '{class_name}' {finding}")));
    }
    violations
}

// -------------------------------------------------------------------------
// no_base_type_annotations — a base type where a shape belongs
// -------------------------------------------------------------------------

/// Base types that accept every model, or every enumeration.
const SHAPELESS_BASE_TYPES: &[&str] = &["BaseModel", "RootModel", "Enum", "StrEnum", "IntEnum"];

/// Detect a base type used as a type annotation.
///
/// `outcome: BaseModel` accepts every model there is: it names no shape, as
/// `object` names none. A class's own base list is not an annotation.
pub fn check_no_base_type_annotations(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let bytes = source.source_bytes;
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), bytes) {
        let named_base = SHAPELESS_BASE_TYPES.iter().find(|base| annotation_contains_name(type_node, base, bytes));
        if let Some(base) = named_base {
            let text = node_text(type_node, bytes);
            violations.push(violation(node_line(type_node), format!("{base} in type annotation: {text}")));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_builtin_records — a record or a table with no model naming it
// -------------------------------------------------------------------------

/// The typing and collections.abc names for the builtin collections, each with the
/// builtin it stands for: a record spelled `Dict[str, str]` is still `dict[str, str]`.
const COLLECTION_SPELLINGS: &[(&str, &str)] = &[
    ("Dict", "dict"),
    ("Mapping", "dict"),
    ("List", "list"),
    ("Sequence", "list"),
    ("Set", "set"),
    ("FrozenSet", "frozenset"),
    ("Tuple", "tuple"),
];

/// A collection's builtin name, whichever of its names it is written under.
fn builtin_spelling(name: &str) -> &str {
    COLLECTION_SPELLINGS.iter().find(|(spelling, _)| *spelling == name).map_or(name, |(_, builtin)| *builtin)
}

/// The base name and the type arguments of a parameterized type, the base by its
/// builtin name: `dict[str, int]` and `Dict[str, int]` → `("dict", [str, int])`.
/// None for any other node.
fn parameterized_type<'tree, 'text>(
    node: tree_sitter::Node<'tree>,
    source: &'text [u8],
) -> Option<(&'text str, Vec<tree_sitter::Node<'tree>>)> {
    let mut cursor = node.walk();
    match node.kind() {
        "generic_type" => {
            let base = node.named_child(0).filter(|first| first.kind() == "identifier")?;
            let parameters = node.named_children(&mut cursor).find(|child| child.kind() == "type_parameter")?;
            let mut parameter_cursor = parameters.walk();
            let arguments = parameters.named_children(&mut parameter_cursor).collect();
            Some((builtin_spelling(node_text(base, source)), arguments))
        }
        "subscript" => {
            let base = node.child_by_field_name("value").filter(|value| value.kind() == "identifier")?;
            let arguments = node.children_by_field_name("subscript", &mut cursor).collect();
            Some((builtin_spelling(node_text(base, source)), arguments))
        }
        _ => None,
    }
}

/// A type with its `type` wrapper taken off.
fn unwrapped_type(node: tree_sitter::Node) -> tree_sitter::Node {
    if node.kind() == "type" {
        return node.named_child(0).unwrap_or(node);
    }
    node
}

/// Whether one member of a type names a builtin: `str`, `None`, `...`, or a
/// builtin collection of builtins.
fn is_builtin_leaf(leaf: tree_sitter::Node, source: &[u8]) -> bool {
    if is_none_type(leaf, source) || leaf.kind() == "ellipsis" {
        return true;
    }
    if leaf.kind() == "identifier" {
        return BUILTIN_TYPE_NAMES.contains(&node_text(leaf, source));
    }
    match parameterized_type(leaf, source) {
        Some((base, arguments)) => {
            BUILTIN_TYPE_NAMES.contains(&base)
                && arguments.into_iter().all(|argument| names_builtins_only(argument, source))
        }
        None => false,
    }
}

/// Whether a type names builtin types and nothing else: `str`, `list[str]`, `int | None`.
fn names_builtins_only(node: tree_sitter::Node, source: &[u8]) -> bool {
    let leaves = collect_union_leaves(node);
    !leaves.is_empty() && leaves.into_iter().all(|leaf| is_builtin_leaf(leaf, source))
}

/// Whether a type is a `tuple[...]`.
fn is_tuple_type(node: tree_sitter::Node, source: &[u8]) -> bool {
    parameterized_type(unwrapped_type(node), source).is_some_and(|(base, _)| base == "tuple")
}

/// What a parameterized type holds in place of a model, if it does: a dict
/// whose values are builtins, or a collection of tuples.
fn builtin_record_finding(node: tree_sitter::Node, source: &[u8]) -> Option<&'static str> {
    let (base, arguments) = parameterized_type(node, source)?;
    match (base, arguments.as_slice()) {
        ("dict", [_, value]) if names_builtins_only(*value, source) => Some("dict of builtin values"),
        ("list" | "set" | "frozenset", [item]) if is_tuple_type(*item, source) => Some("collection of tuples"),
        _ => None,
    }
}

/// The outermost builtin records inside one annotation, each with its line.
fn builtin_records_in(type_node: tree_sitter::Node, source: &[u8]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut reported_until = 0;
    for node in walk_tree(type_node) {
        if node.start_byte() < reported_until {
            continue;
        }
        if let Some(finding) = builtin_record_finding(node, source) {
            reported_until = node.end_byte();
            violations.push(violation(node_line(node), format!("{finding}: {}", node_text(node, source))));
        }
    }
    violations
}

/// A function's return type when it is a tuple of named places, such as
/// `tuple[Report, list[str]]`. `tuple[int, ...]` is a sequence, and is not one.
fn tuple_return<'tree>(function: tree_sitter::Node<'tree>, source: &[u8]) -> Option<tree_sitter::Node<'tree>> {
    let returned = unwrapped_type(node_field(function, "return_type")?);
    let (base, arguments) = parameterized_type(returned, source)?;
    let is_sequence = arguments.iter().any(|argument| unwrapped_type(*argument).kind() == "ellipsis");
    (base == "tuple" && !is_sequence).then_some(returned)
}

/// Detect data carried in builtin containers where a model belongs: a dict
/// whose values are builtin types, a collection of tuples, a tuple return.
///
/// Each is several facts about one thing, side by side, with nothing naming
/// them. `list[str]` and `dict[str, Model]` are not: the first is a list of
/// one kind of thing, the second a table of named records.
pub fn check_no_builtin_records(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let bytes = source.source_bytes;
    let root = source.tree.root_node();
    let mut violations = Vec::new();

    for type_node in find_type_annotations(root, bytes) {
        violations.extend(builtin_records_in(type_node, bytes));
    }
    for function in find_nodes_by_type(root, "function_definition") {
        if let Some(returned) = tuple_return(function, bytes) {
            let function_name = node_field(function, "name").map(|name| node_text(name, bytes)).unwrap_or("<unknown>");
            let text = node_text(returned, bytes);
            violations.push(violation(node_line(function), format!("{function_name}() returns a tuple: {text}")));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_string_value_sets — a string literal as one of a fixed set of values
// -------------------------------------------------------------------------

/// A plain string literal that says something. The empty string is an absence,
/// and an f-string carries data.
fn is_value_literal(node: tree_sitter::Node, source: &[u8]) -> bool {
    plain_string_content(node, source).is_some_and(|content| !content.is_empty())
}

/// A tuple, list or set written out as value literals and nothing else.
fn is_literal_collection(node: tree_sitter::Node, source: &[u8]) -> bool {
    if !matches!(node.kind(), "tuple" | "list" | "set") {
        return false;
    }
    let mut cursor = node.walk();
    let members: Vec<_> = node.named_children(&mut cursor).collect();
    !members.is_empty() && members.into_iter().all(|member| is_value_literal(member, source))
}

/// Whether one test of a comparison holds a value against a string literal.
/// A lone literal on either side of `in` may be a search inside a text
/// (`"**" in line`, `needle in "haystack"`), so `in` counts only against a
/// collection written out as literals.
fn is_literal_test(left: tree_sitter::Node, operator: &str, right: tree_sitter::Node, source: &[u8]) -> bool {
    match operator {
        "==" | "!=" => is_value_literal(left, source) || is_value_literal(right, source),
        "in" | "not in" => is_literal_collection(right, source),
        _ => false,
    }
}

/// Whether a comparison holds a value against a string literal. The module's
/// own `__name__` test names no value of the program's.
fn compares_with_literal(comparison: tree_sitter::Node, source: &[u8]) -> bool {
    if node_text(comparison, source).contains("__name__") {
        return false;
    }
    let mut cursor = comparison.walk();
    let parts: Vec<_> = comparison.children(&mut cursor).collect();
    parts.windows(3).any(|window| is_literal_test(window[0], window[1].kind(), window[2], source))
}

/// Whether a `case` matches on a string literal.
fn case_matches_literal(clause: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = clause.walk();
    let found = clause.named_children(&mut cursor).filter(|part| part.kind() == "case_pattern").any(|pattern| {
        find_nodes_by_type(pattern, "string").into_iter().any(|literal| is_value_literal(literal, source))
    });
    found
}

/// How many value literals one `return` hands back: the literal itself, or
/// each literal branch of a returned conditional.
fn returned_literals(statement: tree_sitter::Node, source: &[u8]) -> usize {
    let Some(value) = statement.named_child(0) else {
        return 0;
    };
    if value.kind() != "conditional_expression" {
        return usize::from(is_value_literal(value, source));
    }
    let mut cursor = value.walk();
    let branches = value.named_children(&mut cursor).filter(|branch| is_value_literal(*branch, source)).count();
    branches
}

/// How many value literals a function body hands back across its returns. A
/// nested function, class or lambda answers for its own.
fn literal_return_count(node: tree_sitter::Node, source: &[u8]) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        count += match child.kind() {
            "function_definition" | "class_definition" | "lambda" => 0,
            "return_statement" => returned_literals(child, source),
            _ => literal_return_count(child, source),
        };
    }
    count
}

/// Detect a string literal used as one of a fixed set of values: a value
/// compared with a literal, a `case` matching one, and a function that hands
/// back one of several.
///
/// A value that can only be one of a few things is a type; written as strings
/// the set has no home and no list of its members.
pub fn check_no_string_value_sets(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let bytes = source.source_bytes;
    let root = source.tree.root_node();
    let mut violations = Vec::new();

    for comparison in find_nodes_by_type(root, "comparison_operator") {
        if compares_with_literal(comparison, bytes) {
            let text = node_text(comparison, bytes);
            violations.push(violation(node_line(comparison), format!("value compared with a string literal: {text}")));
        }
    }
    for clause in find_nodes_by_type(root, "case_clause") {
        if case_matches_literal(clause, bytes) {
            violations.push(violation(node_line(clause), "case matches a string literal".to_string()));
        }
    }
    for function in find_nodes_by_type(root, "function_definition") {
        let alternatives = node_field(function, "body").map_or(0, |body| literal_return_count(body, bytes));
        if alternatives > 1 {
            let function_name = node_field(function, "name").map(|name| node_text(name, bytes)).unwrap_or("<unknown>");
            violations.push(violation(
                node_line(function),
                format!("{function_name}() returns one of several string literals"),
            ));
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::build_parsed_source;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source("/test/file.py", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(crate::structures::FileKind::Outside, &crate::STATISTICS)
    }

    // -- no_object --

    #[test]
    fn object_in_annotation() {
        let parsed = parse("def foo(x: object) -> None:\n    pass\n");
        let violations = check_no_object(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("object"));
    }

    #[test]
    fn no_object_clean() {
        let parsed = parse("def foo(x: str) -> str:\n    return x\n");
        let violations = check_no_object(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_json_value --

    #[test]
    fn json_value_in_annotation() {
        let parsed = parse("x: JsonValue = {}\n");
        let violations = check_no_json_value(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("JsonValue"));
    }

    #[test]
    fn no_json_value_clean() {
        let parsed = parse("x: dict[str, int] = {}\n");
        let violations = check_no_json_value(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn json_value_and_object_under_another_name_caught() {
        // The edge: Any and Callable were followed through a renamed import; these were not.
        for code in [
            "from pydantic import JsonValue as Payload\ndef load(data: Payload) -> int:\n    return 1\n",
            "from pydantic.types import JsonValue as Payload\nrows: list[Payload] = []\n",
        ] {
            assert_eq!(check_no_json_value(&parse(code), &default_config()).len(), 1, "{code}");
        }
        let code = "from builtins import object as Thing\ndef load(data: Thing) -> int:\n    return 1\n";
        assert_eq!(check_no_object(&parse(code), &default_config()).len(), 1);
    }

    #[test]
    fn a_name_that_is_not_json_value_or_object_is_not_one() {
        let code = "from mypkg.models import Payload, Thing\ndef load(data: Payload, other: Thing) -> int:\n    return 1\n";
        assert!(check_no_json_value(&parse(code), &default_config()).is_empty());
        assert!(check_no_object(&parse(code), &default_config()).is_empty());
    }

    // -- no_any_types --

    #[test]
    fn any_in_annotation() {
        let parsed = parse("def foo(x: Any) -> Any:\n    return x\n");
        let violations = check_no_any_types(&parsed, &default_config());
        assert!(!violations.is_empty());
    }

    #[test]
    fn no_any_clean() {
        let parsed = parse("def foo(x: str) -> int:\n    return 1\n");
        let violations = check_no_any_types(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn any_in_type_alias_caught() {
        let parsed = parse("type JsonNode = dict[str, Any]\n");
        let violations = check_no_any_types(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Any"));
    }

    #[test]
    fn any_under_another_name_caught() {
        // The canary: Any imported under a respectable name.
        for code in [
            "from typing import Any as Unknown\ndef load(value: Unknown) -> int:\n    return 1\n",
            "from typing_extensions import Any as Payload\nrows: list[Payload] = []\n",
        ] {
            assert_eq!(check_no_any_types(&parse(code), &default_config()).len(), 1, "{code}");
        }
    }

    #[test]
    fn any_alias_through_a_renamed_import_caught() {
        let parsed = parse("from typing import Any as Unknown\ntype JsonNode = dict[str, Unknown]\n");
        assert_eq!(check_no_any_type_aliases(&parsed, &default_config()).len(), 1);
    }

    #[test]
    fn a_name_that_is_not_any_is_not_any() {
        let parsed = parse("from mypkg.models import Unknown\ndef load(value: Unknown) -> int:\n    return 1\n");
        assert!(check_no_any_types(&parsed, &default_config()).is_empty());
    }

    // -- no_bare_collections --

    #[test]
    fn bare_dict_caught() {
        let parsed = parse("x: dict\n");
        let violations = check_no_bare_collections(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("dict"));
    }

    #[test]
    fn subscripted_dict_ok() {
        let parsed = parse("x: dict[str, int]\n");
        let violations = check_no_bare_collections(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn bare_typing_collections_and_bare_frozenset_caught() {
        // The edge: only dict, list, set and tuple were looked for.
        for name in ["frozenset", "Dict", "List", "Set", "FrozenSet", "Tuple", "Mapping", "Sequence"] {
            let parsed = parse(&format!("def load(items: {name}) -> int:\n    return 1\n"));
            let violations = check_no_bare_collections(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{name}");
            assert!(violations[0].message.contains(name), "{name}");
        }
    }

    #[test]
    fn subscripted_typing_collections_ok() {
        let parsed = parse("def load(items: Mapping[str, Page], rows: Sequence[Page]) -> FrozenSet[int]:\n    return frozenset()\n");
        assert!(check_no_bare_collections(&parsed, &default_config()).is_empty());
        let parsed = parse("def load(items: typing.Dict[str, int], rows: collections.abc.Sequence[Page]) -> int:\n    return 1\n");
        assert!(check_no_bare_collections(&parsed, &default_config()).is_empty());
        let parsed = parse("def load(items: typing.Dict) -> int:\n    return 1\n");
        assert_eq!(check_no_bare_collections(&parsed, &default_config()).len(), 1);
    }

    #[test]
    fn bare_list_caught() {
        let parsed = parse("def foo(items: list) -> None:\n    pass\n");
        let violations = check_no_bare_collections(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("list"));
    }

    // -- union_member_count --

    #[test]
    fn small_simple_union_ok() {
        let parsed = parse("x: int | str\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 2 simple, cap 4
    }

    #[test]
    fn simple_at_cap_ok() {
        let parsed = parse("x: int | str | float | bool\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 4 simple, cap 4
    }

    #[test]
    fn simple_over_cap_caught() {
        let parsed = parse("x: int | str | float | bool | bytes\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1); // 5 simple, cap 4
        assert!(violations[0].message.contains("simple types"));
    }

    #[test]
    fn none_not_counted() {
        let parsed = parse("x: int | str | float | bool | None\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 4 simple + None free = ok
    }

    #[test]
    fn named_at_cap_ok() {
        let parsed = parse(
            "x: ModelA | ModelB | ModelC | ModelD | ModelE | ModelF | ModelG | ModelH\n",
        );
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 8 named, cap 8
    }

    #[test]
    fn named_over_cap_caught() {
        let parsed = parse(
            "x: ModelA | ModelB | ModelC | ModelD | ModelE | ModelF | ModelG | ModelH | ModelI\n",
        );
        let violations = check_union_member_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1); // 9 named, cap 8
        assert!(violations[0].message.contains("named types"));
    }

    #[test]
    fn named_with_none_ok() {
        let parsed = parse(
            "x: ModelA | ModelB | ModelC | ModelD | ModelE | ModelF | ModelG | ModelH | None\n",
        );
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 8 named + None free
    }

    #[test]
    fn parameterized_builtin_is_simple() {
        let parsed = parse("x: list[int] | dict[str, float] | tuple[bool, ...] | set[bytes] | frozenset[int]\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1); // 5 simple (parameterized), cap 4
        assert!(violations[0].message.contains("simple types"));
    }

    #[test]
    fn mixed_both_under_caps_ok() {
        let parsed = parse("x: int | str | ModelA | ModelB | ModelC | ModelD | ModelE | ModelF\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 2 simple (cap 4), 6 named (cap 8)
    }

    #[test]
    fn mixed_simple_over_cap() {
        let parsed = parse(
            "x: int | str | float | bool | bytes | ModelA | ModelB\n",
        );
        let violations = check_union_member_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1); // 5 simple over cap
        assert!(violations[0].message.contains("simple types"));
    }

    /// The four builtin type names that pin the one list in both of its uses.
    const PINNED_BUILTIN_TYPE_NAMES: [&str; 4] = ["type", "complex", "bytearray", "memoryview"];

    #[test]
    fn each_pinned_builtin_name_is_a_simple_union_member() {
        for name in PINNED_BUILTIN_TYPE_NAMES {
            let parsed = parse(&format!("x: int | str | float | bool | {name}\n"));
            let violations = check_union_member_count(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{name}"); // 5 simple, cap 4
            assert!(violations[0].message.contains("simple types"), "{name}");
        }
    }

    #[test]
    fn a_name_that_only_resembles_a_pinned_builtin_is_a_named_union_member() {
        for name in PINNED_BUILTIN_TYPE_NAMES {
            let parsed = parse(&format!("x: int | str | float | bool | {name}_like\n"));
            let violations = check_union_member_count(&parsed, &default_config());
            assert!(violations.is_empty(), "{name}_like"); // 4 simple, 1 named
        }
    }

    #[test]
    fn union_member_count_messages_carry_no_numbers() {
        let simple = parse("x: int | str | float | bool | bytes\n");
        let named = parse(
            "x: ModelA | ModelB | ModelC | ModelD | ModelE | ModelF | ModelG | ModelH | ModelI\n",
        );
        let mut violations = check_union_member_count(&simple, &default_config());
        violations.extend(check_union_member_count(&named, &default_config()));
        assert_eq!(violations.len(), 2);
        for viol in &violations {
            assert!(!viol.message.contains(|c: char| c.is_ascii_digit()), "{}", viol.message);
        }
    }

    // -- no_callable_params --

    #[test]
    fn callable_param_caught() {
        let parsed = parse("def run(fn: Callable[[int], str]) -> None:\n    pass\n");
        let violations = check_no_callable_params(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Callable"));
    }

    #[test]
    fn bare_callable_param_caught() {
        let parsed = parse("def run(fn: Callable) -> None:\n    pass\n");
        let violations = check_no_callable_params(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn callable_in_union_param_caught() {
        let parsed = parse("def run(fn: Callable | None) -> None:\n    pass\n");
        let violations = check_no_callable_params(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_callable_param_ok() {
        let parsed = parse("def run(x: int, y: str) -> None:\n    pass\n");
        let violations = check_no_callable_params(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn callable_under_another_name_caught() {
        for code in [
            "from typing import Callable as Handler\ndef run(on_done: Handler[[int], str]) -> None:\n    pass\n",
            "from collections.abc import Callable as Hook\ndef run(on_done: Hook) -> None:\n    pass\n",
        ] {
            assert_eq!(check_no_callable_params(&parse(code), &default_config()).len(), 1, "{code}");
        }
        let alias = parse("from typing import Callable as Handler\ntype Step = Handler[[int], int]\n");
        assert_eq!(check_no_callable_type_aliases(&alias, &default_config()).len(), 1);
    }

    #[test]
    fn callable_alias_written_as_a_plain_assignment_caught() {
        // The edge: an alias by assignment, not by `type`, gives the parameter a name
        // no_callable_params cannot see through.
        for code in [
            "from typing import Callable\nStep = Callable[[int], int]\ndef run(step: Step) -> int:\n    return 1\n",
            "from typing import Callable, TypeAlias\nStep: TypeAlias = Callable[[int], int]\n",
            "from collections.abc import Callable as Hook\nStep = Hook[[int], int]\n",
            "import typing\nStep = typing.Callable[[int], int]\n",
        ] {
            assert_eq!(check_no_callable_type_aliases(&parse(code), &default_config()).len(), 1, "{code}");
        }
    }

    #[test]
    fn a_callable_built_inside_a_function_is_not_an_alias() {
        let code = "from typing import Callable\ndef run() -> int:\n    step = Callable[[int], int]\n    return 1\n";
        assert!(check_no_callable_type_aliases(&parse(code), &default_config()).is_empty());
    }

    #[test]
    fn callable_return_type_ok() {
        let parsed = parse("def factory() -> Callable[[int], str]:\n    pass\n");
        let violations = check_no_callable_params(&parsed, &default_config());
        assert!(violations.is_empty()); // return type, not param
    }

    // -- no_implicit_type_aliases --

    #[test]
    fn literal_alias_caught() {
        let parsed = parse("Status = Literal['active', 'inactive']\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Literal"));
    }

    #[test]
    fn union_alias_caught() {
        let parsed = parse("Result = int | str\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Union"));
    }

    #[test]
    fn union_alias_of_type_shaped_operands_caught() {
        for code in [
            "Maybe = Foo | None\n",
            "Handler = list[int] | Mapping\n",
            "Number = int | float | Decimal\n",
            "Shape = shapes.Circle | shapes.Square\n",
        ] {
            let parsed = parse(code);
            let violations = check_no_implicit_type_aliases(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{code}");
            assert!(violations[0].message.contains("Union"), "{code}");
        }
    }

    #[test]
    fn each_pinned_builtin_name_is_a_type_shaped_alias_operand() {
        for name in PINNED_BUILTIN_TYPE_NAMES {
            let parsed = parse(&format!("Maybe = {name} | None\n"));
            let violations = check_no_implicit_type_aliases(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{name}");
            assert!(violations[0].message.contains("Union"), "{name}");
        }
    }

    #[test]
    fn a_name_that_only_resembles_a_pinned_builtin_is_not_a_type_shaped_alias_operand() {
        for name in PINNED_BUILTIN_TYPE_NAMES {
            let parsed = parse(&format!("merged = {name}_like | None\n"));
            let violations = check_no_implicit_type_aliases(&parsed, &default_config());
            assert!(violations.is_empty(), "{name}_like");
        }
    }

    #[test]
    fn pipe_between_values_is_not_an_alias() {
        for code in [
            "ALL = FIRST_SET | SECOND_SET\n",
            "merged = defaults | overrides\n",
            "FLAGS = re.I | re.M\n",
            "X = {1} | {2}\n",
            "Combined = build() | Foo\n",
            "Mask = Foo | 4\n",
            "Mixed = int | str | SOME_CONSTANT\n",
            "Total = Foo + Bar\n",
        ] {
            let parsed = parse(code);
            let violations = check_no_implicit_type_aliases(&parsed, &default_config());
            assert!(violations.is_empty(), "{code}");
        }
    }

    #[test]
    fn normal_assignment_ok() {
        let parsed = parse("x = 42\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn dict_alias_caught() {
        let parsed = parse("Config = dict[str, int]\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_any_type_aliases --

    #[test]
    fn any_type_alias_caught() {
        let parsed = parse("type JsonNode = dict[str, Any]\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("JsonNode"));
        assert!(violations[0].message.contains("launders Any"));
    }

    #[test]
    fn nested_any_type_alias_caught() {
        let parsed = parse("type Payload = list[dict[str, Any]]\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Payload"));
    }

    #[test]
    fn clean_type_alias_ok() {
        let parsed = parse("type UserId = str\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn type_alias_no_any_ok() {
        let parsed = parse("type Config = dict[str, int]\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_string_annotations --

    #[test]
    fn string_return_annotation_caught() {
        let parsed = parse("def assemble() -> \"SectionBuffer\":\n    pass\n");
        let violations = check_no_string_annotations(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("string annotation"));
    }

    #[test]
    fn string_param_annotation_caught() {
        let parsed = parse("def process(data: \"InputModel\") -> None:\n    pass\n");
        let violations = check_no_string_annotations(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn a_string_annotation_inside_a_generic_is_caught() {
        // The edge: only a whole annotation that is a string was looked for.
        for code in [
            "def load(pages: list[\"Page\"]) -> int:\n    return 1\n",
            "def load(page: Page) -> dict[str, \"Report\"]:\n    return {}\n",
            "def load(page: \"Page\" | None) -> int:\n    return 1\n",
        ] {
            let violations = check_no_string_annotations(&parse(code), &default_config());
            assert_eq!(violations.len(), 1, "{code}");
        }
    }

    #[test]
    fn strings_that_are_values_in_an_annotation_are_ok() {
        for code in [
            "def load(kind: Literal[\"entity\", \"source\"]) -> int:\n    return 1\n",
            "def load(name: Annotated[str, \"a note\"]) -> int:\n    return 1\n",
            "def load(kind: Literal[\"entity\"] | None) -> int:\n    return 1\n",
        ] {
            assert!(check_no_string_annotations(&parse(code), &default_config()).is_empty(), "{code}");
        }
    }

    #[test]
    fn real_type_annotations_ok() {
        let parsed = parse("def add(a: int, b: int) -> int:\n    return a + b\n");
        let violations = check_no_string_annotations(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn multiple_string_annotations_all_caught() {
        let parsed = parse("def transform(data: \"Input\", config: \"Config\") -> \"Output\":\n    pass\n");
        let violations = check_no_string_annotations(&parsed, &default_config());
        assert_eq!(violations.len(), 3);
    }

    // -- no_open_models --

    fn open_models(code: &str) -> Vec<Violation> {
        check_no_open_models(&parse(code), &default_config())
    }

    #[test]
    fn a_model_opened_in_its_class_line_is_caught() {
        let found = open_models("class Frontmatter(BaseModel, frozen=True, extra=\"allow\"):\n    title: str\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "model 'Frontmatter' accepts undeclared keys");
    }

    #[test]
    fn a_model_with_no_fields_is_caught() {
        let found = open_models("class RunStop(BaseModel, frozen=True):\n    \"\"\"A marker.\"\"\"\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "model 'RunStop' declares no fields");
    }

    #[test]
    fn an_open_model_with_no_fields_is_one_finding() {
        // The specimen: a key file's whole frontmatter, no key declared.
        let found = open_models(
            "class KeyFileFrontmatter(BaseModel, frozen=True, extra=\"allow\"):\n    \"\"\"Every key rides through.\"\"\"\n",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "model 'KeyFileFrontmatter' accepts undeclared keys and declares no fields");
        assert_eq!(found[0].line, 1);
    }

    #[test]
    fn a_model_opened_through_model_config_is_caught() {
        for config in ["ConfigDict(extra=\"allow\")", "ConfigDict(frozen=True, extra='allow')", "{\"extra\": \"allow\"}"] {
            let found = open_models(&format!("class Page(BaseModel):\n    model_config = {config}\n    title: str\n"));
            assert_eq!(found.len(), 1, "{config}");
            assert!(found[0].message.contains("accepts undeclared keys"), "{config}");
        }
    }

    #[test]
    fn a_model_opened_through_a_nested_config_class_is_caught() {
        for value in ["\"allow\"", "Extra.allow"] {
            let found = open_models(&format!(
                "class Page(BaseModel):\n    title: str\n\n    class Config:\n        extra = {value}\n"
            ));
            assert_eq!(found.len(), 1, "{value}");
            assert_eq!(found[0].message, "model 'Page' accepts undeclared keys", "{value}");
        }
    }

    #[test]
    fn an_annotated_model_config_is_not_a_field() {
        let found = open_models("class Page(BaseModel):\n    model_config: ClassVar[ConfigDict] = ConfigDict(frozen=True)\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "model 'Page' declares no fields");
    }

    #[test]
    fn a_closed_model_with_fields_is_ok() {
        for code in [
            "class Page(BaseModel, frozen=True):\n    title: str\n",
            "class Page(BaseModel, extra=\"forbid\"):\n    title: str\n",
            "class Page(BaseModel, extra=\"ignore\"):\n    title: str\n",
            "class Page(BaseModel):\n    model_config = ConfigDict(extra=\"forbid\")\n    title: str = \"\"\n",
        ] {
            assert!(open_models(code).is_empty(), "{code}");
        }
    }

    #[test]
    fn a_class_that_is_not_written_on_base_model_may_have_no_fields() {
        for code in [
            "class Vocabulary(RootModel[dict[str, VocabularyEntry]], frozen=True):\n    \"\"\"A member set.\"\"\"\n",
            "class Colour(Enum):\n    red = 1\n",
            "class MissingPartError(ValueError):\n    pass\n",
            "class ReadFailures(RunStop, frozen=True):\n    failures: list[DocumentFailure]\n",
        ] {
            assert!(open_models(code).is_empty(), "{code}");
        }
    }

    #[test]
    fn a_call_that_only_resembles_the_switch_is_ok() {
        let code = "class Page(BaseModel):\n    title: str\n    other = build(extra=\"allow\")\n\ndef make(extra: str = \"allow\") -> Page:\n    return Page(title=extra)\n";
        assert!(open_models(code).is_empty());
    }

    // -- no_base_type_annotations --

    fn base_annotations(code: &str) -> Vec<Violation> {
        check_no_base_type_annotations(&parse(code), &default_config())
    }

    #[test]
    fn a_base_model_parameter_is_caught() {
        // The specimen: a nine-member union widened until its warning went away.
        let found = base_annotations("def exit_code(outcome: BaseModel) -> int:\n    return 0\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "BaseModel in type annotation: BaseModel");
    }

    #[test]
    fn a_base_type_is_caught_wherever_it_annotates() {
        for (base, code) in [
            ("BaseModel", "def load() -> BaseModel:\n    return build()\n"),
            ("BaseModel", "class Report(Outcome):\n    cause: BaseModel\n"),
            ("BaseModel", "def render(outcome: Report | BaseModel) -> str:\n    return \"\"\n"),
            ("BaseModel", "def render(outcomes: list[BaseModel]) -> str:\n    return \"\"\n"),
            ("BaseModel", "def render(outcome: pydantic.BaseModel) -> str:\n    return \"\"\n"),
            ("BaseModel", "type Outcome = BaseModel\n"),
            ("RootModel", "def render(registry: RootModel) -> str:\n    return \"\"\n"),
            ("Enum", "def label(kind: Enum) -> str:\n    return kind.name\n"),
            ("StrEnum", "def label(kind: StrEnum) -> str:\n    return kind.value\n"),
        ] {
            let found = base_annotations(code);
            assert_eq!(found.len(), 1, "{code}");
            assert!(found[0].message.starts_with(&format!("{base} in type annotation")), "{code}");
        }
    }

    #[test]
    fn a_base_type_in_a_class_s_base_list_is_ok() {
        for code in [
            "class Page(BaseModel, frozen=True):\n    title: str\n",
            "class Vocabulary(RootModel[dict[str, VocabularyEntry]], frozen=True):\n    \"\"\"A member set.\"\"\"\n",
            "class Colour(Enum):\n    red = 1\n",
        ] {
            assert!(base_annotations(code).is_empty(), "{code}");
        }
    }

    #[test]
    fn a_named_shape_is_ok() {
        for code in [
            "def exit_code(outcome: ReadFailures | WriteFailures) -> int:\n    return 1\n",
            "def label(kind: RecordKind) -> str:\n    return kind.value\n",
            "def render(outcome: BaseModelReport, kind: MyEnum) -> str:\n    return \"\"\n",
        ] {
            assert!(base_annotations(code).is_empty(), "{code}");
        }
    }

    // -- no_builtin_records --

    fn builtin_records(code: &str) -> Vec<Violation> {
        check_no_builtin_records(&parse(code), &default_config())
    }

    #[test]
    fn a_dict_of_builtin_values_is_caught() {
        // The specimens: one record per tag, split into columns keyed by its name.
        for (annotation, code) in [
            ("dict[str, int]", "class TagCensus(BaseModel):\n    counts: dict[str, int]\n"),
            ("dict[str, list[str]]", "class TagCensus(BaseModel):\n    carriers: dict[str, list[str]]\n"),
            ("dict[str, str]", "def classify(census: TagCensus) -> dict[str, str]:\n    return {}\n"),
            ("dict[str, str]", "def render(classes: dict[str, str]) -> str:\n    return \"\"\n"),
            ("dict[str, int | None]", "limits: dict[str, int | None] = {}\n"),
            ("dict[str, str]", "def find(names: dict[str, str] | None) -> str:\n    return \"\"\n"),
        ] {
            let found = builtin_records(code);
            assert_eq!(found.len(), 1, "{code}");
            assert_eq!(found[0].message, format!("dict of builtin values: {annotation}"), "{code}");
        }
    }

    #[test]
    fn a_builtin_record_spelled_with_typing_names_is_caught() {
        // The edge: Dict, Mapping, List and Tuple are the same records under other names.
        for (finding, code) in [
            ("dict of builtin values", "counts: Dict[str, int] = {}\n"),
            ("dict of builtin values", "def render(classes: Mapping[str, str]) -> str:\n    return \"\"\n"),
            ("dict of builtin values", "carriers: dict[str, List[str]] = {}\n"),
            ("collection of tuples", "pairs: List[Tuple[str, str]] = []\n"),
            ("collection of tuples", "pairs: Sequence[tuple[str, int]] = []\n"),
        ] {
            let found = builtin_records(code);
            assert_eq!(found.len(), 1, "{code}");
            assert!(found[0].message.starts_with(finding), "{code}: {}", found[0].message);
        }
        let found = builtin_records("def pick(page: Page) -> Tuple[Page, List[str]]:\n    return page, []\n");
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("returns a tuple"), "{}", found[0].message);
    }

    #[test]
    fn a_typing_table_of_models_is_ok() {
        for code in ["registry: Mapping[str, TagMeaning] = {}\n", "tags: List[str] = []\n", "sizes: Tuple[int, ...] = ()\n"] {
            assert!(builtin_records(code).is_empty(), "{code}");
        }
    }

    #[test]
    fn a_nested_builtin_record_is_one_finding_at_the_outermost() {
        let found = builtin_records("lookup: dict[str, dict[str, str]] = {}\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "dict of builtin values: dict[str, dict[str, str]]");

        let found = builtin_records("rows: list[dict[str, str]] = []\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "dict of builtin values: dict[str, str]");
    }

    #[test]
    fn a_collection_of_tuples_is_caught() {
        for (annotation, code) in [
            ("list[tuple[str, str]]", "pairs: list[tuple[str, str]] = []\n"),
            ("set[tuple[str, int]]", "def seen(pairs: set[tuple[str, int]]) -> int:\n    return 0\n"),
            ("list[tuple[Page, Report]]", "def join(pages: list[Page]) -> list[tuple[Page, Report]]:\n    return []\n"),
        ] {
            let found = builtin_records(code);
            assert_eq!(found.len(), 1, "{code}");
            assert_eq!(found[0].message, format!("collection of tuples: {annotation}"), "{code}");
        }
    }

    #[test]
    fn a_tuple_return_is_caught() {
        // The specimen: a result and its defects handed back side by side.
        let found = builtin_records(
            "def with_entity_aliases(meanings: Meanings) -> tuple[dict[str, TagMeaning], list[str]]:\n    return {}, []\n",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].message,
            "with_entity_aliases() returns a tuple: tuple[dict[str, TagMeaning], list[str]]"
        );
    }

    #[test]
    fn a_tuple_that_is_a_sequence_is_not_a_tuple_return() {
        assert!(builtin_records("def sizes(page: Page) -> tuple[int, ...]:\n    return ()\n").is_empty());
    }

    #[test]
    fn a_collection_of_one_kind_and_a_table_of_models_are_ok() {
        for code in [
            "class Page(BaseModel):\n    tags: list[str]\n    seen: set[str]\n    sizes: tuple[int, ...]\n",
            "class Registry(BaseModel):\n    meanings: dict[str, TagMeaning]\n",
            "def group(surveys: list[DirectorySurvey]) -> dict[str, list[DirectorySurvey]]:\n    return {}\n",
            "DISPATCH: dict[type[Page], Callable[[Page], Report]] = {}\n",
            "def load(data: dict[str, Any]) -> Page:\n    return build(data)\n",
            "def pick(first: str, second: int) -> Page:\n    return build(first, second)\n",
        ] {
            assert!(builtin_records(code).is_empty(), "{code}");
        }
    }

    // -- no_string_value_sets --

    fn string_value_sets(code: &str) -> Vec<Violation> {
        check_no_string_value_sets(&parse(code), &default_config())
    }

    #[test]
    fn a_value_compared_with_a_string_literal_is_caught() {
        for code in [
            "pending = [name for name in names if classes[name] == \"pending\"]\n",
            "if tag_class != 'entity':\n    skip()\n",
            "if kind in (\"entity\", \"source\"):\n    read(kind)\n",
            "if kind not in [\"entity\", \"source\"]:\n    skip()\n",
            "if first < second == \"pending\":\n    skip()\n",
        ] {
            let found = string_value_sets(code);
            assert_eq!(found.len(), 1, "{code}");
            assert!(found[0].message.starts_with("value compared with a string literal: "), "{code}");
        }
    }

    #[test]
    fn a_case_matching_a_string_literal_is_caught() {
        let found = string_value_sets(
            "def read(kind: str) -> int:\n    match kind:\n        case \"entity\" | \"source\":\n            return 1\n        case _:\n            return 0\n",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "case matches a string literal");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn a_function_returning_one_of_several_string_literals_is_caught() {
        // The specimen: a census class handed back as a word, one of them carrying data.
        let code = "def classification(tag_name: str, stems: set[str], owners: Owners, is_entity: bool) -> str:\n    if tag_name in stems:\n        return \"canonical+entity\" if is_entity else \"canonical\"\n    if tag_name in owners:\n        return f\"alias-of:{owners[tag_name]}\"\n    if is_entity:\n        return \"entity\"\n    return \"pending\"\n";
        let found = string_value_sets(code);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "classification() returns one of several string literals");
        assert_eq!(found[0].line, 1);
    }

    #[test]
    fn a_returned_conditional_of_two_string_literals_is_caught() {
        let found = string_value_sets(
            "def subtype_field(record_kind: RecordKind) -> str:\n    return \"entity_type\" if record_kind is RecordKind.ENTITY else \"media_type\"\n",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "subtype_field() returns one of several string literals");
    }

    #[test]
    fn a_nested_function_answers_for_its_own_returns() {
        let code = "def outer() -> str:\n    def inner() -> str:\n        return \"inner\"\n    return \"outer\"\n";
        assert!(string_value_sets(code).is_empty());
    }

    #[test]
    fn text_handled_as_text_is_ok() {
        for code in [
            "if __name__ == \"__main__\":\n    main()\n",
            "if name == other:\n    skip()\n",
            "if text == \"\":\n    skip()\n",
            "if needle in \"haystack\":\n    skip()\n",
            // The specimens: a marker looked for inside a line of text.
            "if \"**\" in cells[0]:\n    skip()\n",
            "if \"V1 Classifications\" in line:\n    start = index\n",
            "if \"draft\" not in flags:\n    publish()\n",
            "if status is MemberStatus.CANONICAL:\n    keep()\n",
            "if value.startswith(\"[[\"):\n    strip()\n",
            "def label() -> str:\n    return \"name\"\n",
            "def line(name: str, count: int) -> str:\n    if count:\n        return f\"{name} ({count})\"\n    return f\"{name}\"\n",
            "class Page(BaseModel):\n    record_kind: Literal[\"entity\"]\n",
            "title = fields[\"title\"]\n",
        ] {
            assert!(string_value_sets(code).is_empty(), "{code}");
        }
    }
}
