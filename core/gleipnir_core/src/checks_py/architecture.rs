//! Architecture enforcement checks.
//!
//! Checks: no_methods_in_classes, no_callable_protocol, pydantic_only,
//! god_classes, no_reexport_shims, hardcoded_config, import_count,
//! structures_no_functions, structures_import_boundary,
//! classes_only_in_structures, max_functions_outside_zones,
//! v2_structure_no_logic, v2_structure_bases, v2_structure_import_boundary,
//! v2_logic_no_constants, v2_dispatch_only_tables,
//! v2_classes_only_in_structure, no_inline_dispatch.

use crate::classify;
use crate::parsing::{
    extract_module_info, find_nodes_by_type, node_field, node_line, node_text,
};
use crate::structures::{CheckConfig, Level, ParsedSource, Severity, Violation, Zone};

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
// no_methods_in_classes
// -------------------------------------------------------------------------

pub fn check_no_methods_in_classes(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let class_name = node_field(class_node, "name")
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");

        let body = match node_field(class_node, "body") {
            Some(b) => b,
            None => continue,
        };

        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            // Bare method
            let func_node = if child.kind() == "function_definition" {
                Some(child)
            // Decorated method (@field_validator, @classmethod, etc.)
            } else if child.kind() == "decorated_definition" {
                node_field(child, "definition")
                    .filter(|n| n.kind() == "function_definition")
            } else {
                None
            };
            if let Some(func) = func_node {
                let method_name = node_field(func, "name")
                    .map(|n| node_text(n, source.source_bytes))
                    .unwrap_or("<unknown>");
                violations.push(violation(
                    node_line(child),
                    format!("Method '{method_name}' in class '{class_name}'"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_callable_protocol
// -------------------------------------------------------------------------

/// Detect Protocol classes with __call__ method.
///
/// A Protocol with __call__ is structurally equivalent to Callable — it creates
/// a type that accepts any object with a matching call signature. This is the
/// same dependency laundering as Callable parameters, just with extra steps.
pub fn check_no_callable_protocol(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        // Check if class inherits from Protocol
        let superclasses = match node_field(class_node, "superclasses") {
            Some(s) => s,
            None => continue,
        };
        let superclass_text = node_text(superclasses, source.source_bytes);
        if !superclass_text.contains("Protocol") {
            continue;
        }

        // Check if class has a __call__ method
        let body = match node_field(class_node, "body") {
            Some(b) => b,
            None => continue,
        };
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            let func = match child.kind() {
                "function_definition" => child,
                "decorated_definition" => {
                    match node_field(child, "definition")
                        .filter(|n| n.kind() == "function_definition")
                    {
                        Some(f) => f,
                        None => continue,
                    }
                }
                _ => continue,
            };
            let name = node_field(func, "name")
                .map(|n| node_text(n, source.source_bytes))
                .unwrap_or_default();
            if name == "__call__" {
                let class_name = node_field(class_node, "name")
                    .map(|n| node_text(n, source.source_bytes))
                    .unwrap_or("<unknown>");
                violations.push(violation(
                    node_line(class_node),
                    format!("Protocol '{class_name}' with __call__ is a Callable alias"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// pydantic_only
// -------------------------------------------------------------------------

// Reversed to avoid self-triggering
const REVERSED_PYDANTIC_FORBIDDEN: &[&str] = &[
    "tciDdepyT",
    "ssalcatad@",
    "elpuTdemaN",
    "sailAepyT",
    "enifed@",
    "nezorf@",
    "elbatum@",
    "srtta@",
    "tcurtS.cepsgsm",
    "tcurts.cepsgsm@",
];

fn pydantic_forbidden_patterns() -> Vec<String> {
    REVERSED_PYDANTIC_FORBIDDEN
        .iter()
        .map(|s| s.chars().rev().collect())
        .collect()
}

pub fn check_pydantic_only(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let patterns = pydantic_forbidden_patterns();
    let mut violations = Vec::new();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        for pattern in &patterns {
            if line.contains(pattern.as_str()) {
                violations.push(violation(line_num, format!("{pattern} found")));
                break;
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// god_classes
// -------------------------------------------------------------------------

const GOD_CLASS_ALLOWED_METHODS: &[&str] = &[
    "__init__", "__repr__", "__str__", "__eq__", "__hash__",
    "__lt__", "__le__", "__gt__", "__ge__", "__bool__",
    "__len__", "__iter__", "__getitem__", "__setitem__",
    "__contains__", "__post_init__",
    "model_validate", "model_json_schema",
    "model_copy", "model_post_init", "model_rebuild",
];

const GOD_CLASS_ALLOWED_DECORATORS: &[&str] = &[
    "property", "cached_property", "staticmethod", "classmethod",
    "validator", "field_validator", "model_validator",
    "computed_field", "root_validator",
];

fn extract_decorator_name_arch(decorator: tree_sitter::Node, source: &[u8]) -> String {
    let mut cursor = decorator.walk();
    let children: Vec<_> = decorator.named_children(&mut cursor).collect();
    if children.is_empty() {
        return String::new();
    }
    let expr = children[0];
    match expr.kind() {
        "identifier" => node_text(expr, source).to_string(),
        "attribute" => expr
            .child_by_field_name("attribute")
            .map(|a| node_text(a, source).to_string())
            .unwrap_or_default(),
        "call" => {
            let func = match expr.child_by_field_name("function") {
                Some(f) => f,
                None => return String::new(),
            };
            match func.kind() {
                "identifier" => node_text(func, source).to_string(),
                "attribute" => func
                    .child_by_field_name("attribute")
                    .map(|a| node_text(a, source).to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

fn unwrap_decorated<'a>(
    node: tree_sitter::Node<'a>,
) -> Option<(tree_sitter::Node<'a>, Vec<tree_sitter::Node<'a>>)> {
    match node.kind() {
        "function_definition" => Some((node, vec![])),
        "decorated_definition" => {
            let mut cursor = node.walk();
            let mut func = None;
            let mut decorators = Vec::new();
            for child in node.named_children(&mut cursor) {
                match child.kind() {
                    "function_definition" => func = Some(child),
                    "decorator" => decorators.push(child),
                    _ => {}
                }
            }
            func.map(|f| (f, decorators))
        }
        _ => None,
    }
}

fn classify_class_methods(body: tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let mut behavior = Vec::new();
    let mut cursor = body.walk();

    for child in body.named_children(&mut cursor) {
        let (func_node, decorators) = match unwrap_decorated(child) {
            Some(pair) => pair,
            None => continue,
        };

        let method_name = node_field(func_node, "name")
            .map(|n| node_text(n, source))
            .unwrap_or("");

        if GOD_CLASS_ALLOWED_METHODS.contains(&method_name) || method_name.starts_with('_') {
            continue;
        }

        let has_allowed_dec = decorators.iter().any(|dec| {
            let name = extract_decorator_name_arch(*dec, source);
            GOD_CLASS_ALLOWED_DECORATORS.contains(&name.as_str())
        });

        if !has_allowed_dec {
            behavior.push(method_name.to_string());
        }
    }
    behavior
}

pub fn check_god_classes(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let class_name = node_field(class_node, "name")
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("");

        if class_name.starts_with("Test") || class_name.ends_with("Error") {
            continue;
        }

        let body = match node_field(class_node, "body") {
            Some(b) => b,
            None => continue,
        };

        let behavior = classify_class_methods(body, source.source_bytes);
        if !behavior.is_empty() {
            let methods_str = behavior.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            violations.push(violation(
                node_line(class_node),
                format!("{class_name} has behavior methods: {methods_str}"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_reexport_shims
// -------------------------------------------------------------------------

fn classify_expression_stmt(stmt: tree_sitter::Node, source: &[u8]) -> &'static str {
    let mut cursor = stmt.walk();
    let children: Vec<_> = stmt.named_children(&mut cursor).collect();
    if children.is_empty() || children[0].kind() != "assignment" {
        return "other";
    }

    let assign = children[0];
    let left = match node_field(assign, "left") {
        Some(n) if n.kind() == "identifier" => n,
        _ => return "logic",
    };

    if node_text(left, source) == "__all__" {
        "all"
    } else {
        "logic"
    }
}

fn extract_module_docstring(root: tree_sitter::Node, source: &[u8]) -> String {
    let mut cursor = root.walk();
    let first = match root.named_children(&mut cursor).next() {
        Some(n) if n.kind() == "expression_statement" => n,
        _ => return String::new(),
    };
    let mut inner_cursor = first.walk();
    let inner = match first.named_children(&mut inner_cursor).next() {
        Some(n) if n.kind() == "string" => n,
        _ => return String::new(),
    };
    node_text(inner, source).to_string()
}

pub fn check_no_reexport_shims(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut has_imports = false;
    let mut has_all = false;
    let mut has_logic = false;

    let root = source.tree.root_node();
    let docstring = extract_module_docstring(root, source.source_bytes);

    let compat_keywords = ["backward compatibility", "deprecated", "re-export", "compatibility shim"];
    let doc_lower = docstring.to_lowercase();
    let has_compat_docstring = compat_keywords.iter().any(|kw| doc_lower.contains(kw));

    let mut cursor = root.walk();
    for stmt in root.named_children(&mut cursor) {
        match stmt.kind() {
            "import_statement" | "import_from_statement" => has_imports = true,
            "function_definition" | "class_definition" | "decorated_definition" => has_logic = true,
            "expression_statement" => {
                match classify_expression_stmt(stmt, source.source_bytes) {
                    "all" => has_all = true,
                    "logic" => has_logic = true,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let mut violations = Vec::new();

    if has_imports && has_all && !has_logic {
        violations.push(violation(
            1,
            "re-export shim detected (imports + __all__, no logic)".to_string(),
        ));
    } else if has_compat_docstring {
        violations.push(violation(
            1,
            "backward compatibility docstring found".to_string(),
        ));
    }

    violations
}

// -------------------------------------------------------------------------
// hardcoded_config
// -------------------------------------------------------------------------

const CONSTANT_NODE_TYPES: &[&str] = &[
    "string", "integer", "float", "true", "false", "none", "concatenated_string",
];

/// Collection constructor names that are equivalent to literal syntax.
/// `frozenset([...])` is the same intent as `{...}` — hardcoded config.
const COLLECTION_CONSTRUCTORS: &[&str] = &["frozenset", "set", "dict", "list", "tuple"];

/// Check if a `call` node is a collection constructor (frozenset, set, dict, list, tuple).
fn is_collection_constructor(call_node: tree_sitter::Node, source: &[u8]) -> bool {
    let func = match node_field(call_node, "function") {
        Some(n) if n.kind() == "identifier" => n,
        _ => return false,
    };
    COLLECTION_CONSTRUCTORS.contains(&node_text(func, source))
}

/// Extract the assignment node from a top-level statement, if present.
fn extract_assignment(stmt: tree_sitter::Node) -> Option<tree_sitter::Node> {
    if stmt.kind() == "expression_statement" {
        stmt.named_child(0).filter(|c| c.kind() == "assignment")
    } else if stmt.kind() == "assignment" {
        Some(stmt)
    } else {
        None
    }
}

/// Classify what kind of hardcoded config violation an assignment represents.
fn hardcoded_config_message(assign: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let left = node_field(assign, "left").filter(|n| n.kind() == "identifier")?;
    let right = node_field(assign, "right")?;
    let name = node_text(left, source);

    if name.starts_with("__") && name.ends_with("__") {
        return None;
    }

    if right.kind() == "dictionary" || right.kind() == "list" {
        Some(format!("hard-coded config {name} found (dict/list at module level)"))
    } else if right.kind() == "call" && is_collection_constructor(right, source) {
        Some(format!("hard-coded config {name} found (collection constructor at module level)"))
    } else if name.len() > 1
        && name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
        && CONSTANT_NODE_TYPES.contains(&right.kind())
    {
        Some(format!("hard-coded constant {name} found (ALL_CAPS value)"))
    } else {
        None
    }
}

pub fn check_hardcoded_config(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    // Dispatch files ARE typed lookup tables — exempt from hardcoded config check.
    let classification = classify::classify_file_v2(source.file_path);
    if matches!(classification.level, Level::L3 | Level::L6) {
        return Vec::new();
    }

    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for stmt in root.named_children(&mut cursor) {
        let assign = match extract_assignment(stmt) {
            Some(a) => a,
            None => continue,
        };
        if let Some(msg) = hardcoded_config_message(assign, source.source_bytes) {
            violations.push(violation(node_line(assign), msg));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// import_count
// -------------------------------------------------------------------------

/// Detect excessive import fan-in by category.
///
/// Counts import statements separately for functions/, structures/, and other.
/// High function-import count is the strongest signal of fragmented OOP —
/// a coordinator importing single-function siblings.
pub fn check_import_count(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let root = source.tree.root_node();
    let mut cursor = root.walk();
    let mut fn_imports = 0;
    let mut other_imports = 0;

    for stmt in root.named_children(&mut cursor) {
        let module_path = match import_module_path(stmt, source.source_bytes) {
            Some(p) => p,
            None => continue,
        };

        if module_path.contains("functions.") || module_path.contains("functions/") {
            fn_imports += 1;
        } else if module_path.contains("structures.") || module_path.contains("structures/") {
            // No limit on structure imports — functions need their data types
        } else {
            other_imports += 1;
        }
    }

    let mut violations = Vec::new();

    if fn_imports > config.max_function_imports {
        violations.push(violation(
            1,
            "too many imports from functions/ — this module is coordinating, not computing".to_string(),
        ));
    }
    if other_imports > config.max_other_imports {
        violations.push(violation(
            1,
            "too many external/stdlib imports — module has too many external dependencies".to_string(),
        ));
    }

    violations
}

/// Extract the module path from an import statement.
fn import_module_path<'a>(node: tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    if node.kind() == "import_from_statement" {
        return node_field(node, "module_name").map(|n| node_text(n, source));
    }
    if node.kind() == "import_statement" {
        let mut cursor = node.walk();
        return node.named_children(&mut cursor)
            .find(|c| c.kind() == "dotted_name")
            .map(|n| node_text(n, source));
    }
    None
}

// -------------------------------------------------------------------------
// structures_no_functions
// -------------------------------------------------------------------------

fn unwrap_function(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
    if node.kind() == "function_definition" {
        return Some(node);
    }
    if node.kind() == "decorated_definition" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "function_definition" {
                return Some(child);
            }
        }
    }
    None
}

pub fn check_structures_no_functions(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    // Matrix handles dispatch — only called for DataStructure files
    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for stmt in root.named_children(&mut cursor) {
        let func_node = match unwrap_function(stmt) {
            Some(f) => f,
            None => continue,
        };
        let func_name = node_field(func_node, "name")
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");
        violations.push(violation(
            node_line(func_node),
            format!("function '{func_name}()' defined in structures/"),
        ));
    }
    violations
}

// -------------------------------------------------------------------------
// structures_import_boundary
// -------------------------------------------------------------------------

const STRUCTURES_ALLOWED_STDLIB: &[&str] = &[
    "pydantic", "typing", "collections", "enum", "tree_sitter",
    "typing_extensions", "annotated_types",
];

const STRUCTURES_ALLOWED_MODULES: &[&str] = &["pydantic_validators"];

fn is_structures_internal(module_path: &str) -> bool {
    let parts: Vec<&str> = module_path.split('.').collect();
    if parts.contains(&"structures") {
        return true;
    }
    let last = parts.last().copied().unwrap_or("");
    STRUCTURES_ALLOWED_MODULES.contains(&last)
}

pub fn check_structures_import_boundary(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    // Matrix handles dispatch — only called for DataStructure files
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, _level) = extract_module_info(node, source.source_bytes);
        if module.is_empty() {
            continue;
        }

        let parts: Vec<&str> = module.split('.').collect();
        let top = parts.first().copied().unwrap_or("");

        if STRUCTURES_ALLOWED_STDLIB.contains(&top) {
            continue;
        }

        if !is_structures_internal(&module) {
            violations.push(violation(
                node_line(node),
                format!("structures/ file imports from outside data zone: {module}"),
            ));
        }
    }

    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "dotted_name" {
                continue;
            }
            let module = node_text(child, source.source_bytes);
            let parts: Vec<&str> = module.split('.').collect();
            let top = parts.first().copied().unwrap_or("");

            if STRUCTURES_ALLOWED_STDLIB.contains(&top) {
                continue;
            }

            if !is_structures_internal(module) {
                violations.push(violation(
                    node_line(node),
                    format!("structures/ file imports from outside data zone: {module}"),
                ));
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// v2_structure_import_boundary
// -------------------------------------------------------------------------

fn is_structure_internal(module_path: &str) -> bool {
    let parts: Vec<&str> = module_path.split('.').collect();
    if parts.contains(&"structure") {
        return true;
    }
    let last = parts.last().copied().unwrap_or("");
    STRUCTURES_ALLOWED_MODULES.contains(&last)
}

pub fn check_v2_structure_import_boundary(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, _level) = extract_module_info(node, source.source_bytes);
        if module.is_empty() {
            continue;
        }

        let parts: Vec<&str> = module.split('.').collect();
        let top = parts.first().copied().unwrap_or("");

        if STRUCTURES_ALLOWED_STDLIB.contains(&top) {
            continue;
        }

        if !is_structure_internal(&module) {
            violations.push(violation(
                node_line(node),
                format!("structure zone file imports from outside data zone: {module}"),
            ));
        }
    }

    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "dotted_name" {
                continue;
            }
            let module = node_text(child, source.source_bytes);
            let parts: Vec<&str> = module.split('.').collect();
            let top = parts.first().copied().unwrap_or("");

            if STRUCTURES_ALLOWED_STDLIB.contains(&top) {
                continue;
            }

            if !is_structure_internal(module) {
                violations.push(violation(
                    node_line(node),
                    format!("structure zone file imports from outside data zone: {module}"),
                ));
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// classes_only_in_structures
// -------------------------------------------------------------------------

pub fn check_classes_only_in_structures(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    // Matrix handles dispatch — only called for zone files (Pure/Impure/Unsafe)
    let mut violations = Vec::new();

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let class_name = node_field(class_node, "name")
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");
        violations.push(violation(
            node_line(class_node),
            format!("class '{class_name}' defined outside structures/"),
        ));
    }
    violations
}

// -------------------------------------------------------------------------
// v2_classes_only_in_structure
// -------------------------------------------------------------------------

pub fn check_v2_classes_only_in_structure(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    // Matrix handles dispatch — only called for logic zone files (pure/impure/transform/orchestrate)
    let mut violations = Vec::new();

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let class_name = node_field(class_node, "name")
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");
        violations.push(violation(
            node_line(class_node),
            format!("class '{class_name}' defined outside structure zone"),
        ));
    }
    violations
}

// -------------------------------------------------------------------------
// max_functions_outside_zones
// -------------------------------------------------------------------------

pub fn check_max_functions_outside_zones(
    source: &ParsedSource,
    config: &CheckConfig,
) -> Vec<Violation> {
    // Matrix handles dispatch — only called for Outside/Script files
    let mut functions: Vec<&str> = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for stmt in root.named_children(&mut cursor) {
        let func_node = match unwrap_function(stmt) {
            Some(f) => f,
            None => continue,
        };
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };
        if func_name.starts_with('_') || func_name.starts_with("test_") {
            continue;
        }
        functions.push(func_name);
    }

    if functions.len() <= config.max_functions_outside_zones {
        return Vec::new();
    }

    // Fuzzy message — no counts, no thresholds
    vec![violation(
        1,
        "too many top-level functions outside zones".to_string(),
    )]
}

// -------------------------------------------------------------------------
// v2 structure enforcement — no logic in structure/
// -------------------------------------------------------------------------

/// In v2 structure zone: no function definitions, no bare constants.
/// Only class definitions (Pydantic models, Enums) and type aliases allowed.
pub fn check_v2_structure_no_logic(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let classification = classify::classify_file_v2(source.file_path);
    if !classification.zone.is_structure() {
        return Vec::new();
    }

    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for child in root.named_children(&mut cursor) {
        match child.kind() {
            "function_definition" | "async_function_definition" => {
                let name = node_field(child, "name")
                    .map(|n| node_text(n, source.source_bytes))
                    .unwrap_or("<unknown>");
                violations.push(violation(
                    node_line(child),
                    format!("function '{name}' in structure zone (no logic allowed)"),
                ));
            }
            "expression_statement" => {
                if is_module_level_constant(child, source.source_bytes) {
                    violations.push(violation(
                        node_line(child),
                        "module-level constant in structure zone (use enum or model)".to_string(),
                    ));
                }
            }
            _ => {}
        }
    }

    violations
}

// -------------------------------------------------------------------------
// v2 structure enforcement — classes must be pydantic or enum
// -------------------------------------------------------------------------

/// Allowed base class names for structure/ zone classes (model/ and config/).
const STRUCTURE_ALLOWED_BASES: &[&str] = &[
    "BaseModel", "RootModel",
    "Enum", "IntEnum", "StrEnum", "Flag", "IntFlag",
];

/// Allowed base class names for structure/exception/ classes.
const STRUCTURE_EXCEPTION_BASES: &[&str] = &[
    "Exception", "ValueError", "TypeError", "RuntimeError",
    "IOError", "OSError", "KeyError", "AttributeError",
    "NotImplementedError", "PermissionError", "FileNotFoundError",
];

/// In v2 structure zone: every class must inherit from an allowed base.
/// model/ and config/: BaseModel, RootModel, or Enum variants.
/// exception/: Exception or standard exception subclasses.
pub fn check_v2_structure_bases(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let classification = classify::classify_file_v2(source.file_path);
    if !classification.zone.is_structure() {
        return Vec::new();
    }

    let in_exception_dir = source.file_path.contains("/structure/exception/");
    let mut violations = Vec::new();

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let class_name = node_field(class_node, "name")
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");

        let bases = extract_base_classes(class_node, source.source_bytes);

        if bases.is_empty() {
            violations.push(violation(
                node_line(class_node),
                format!("'{class_name}' has no base class"),
            ));
        } else {
            let has_model_base = bases.iter().any(|b| STRUCTURE_ALLOWED_BASES.contains(&b.as_str()));
            let has_exception_base = in_exception_dir
                && bases.iter().any(|b| STRUCTURE_EXCEPTION_BASES.contains(&b.as_str()));
            if !has_model_base && !has_exception_base {
                let expected = if in_exception_dir {
                    "BaseModel, Enum, or Exception"
                } else {
                    "BaseModel or Enum"
                };
                violations.push(violation(
                    node_line(class_node),
                    format!("'{class_name}' inherits from unknown base (must be {expected})"),
                ));
            }
        }
    }

    violations
}

/// Extract base class names from a class definition's argument list.
///
/// For `class Foo(BaseModel, SomeMixin):` returns `["BaseModel", "SomeMixin"]`.
/// For `class Foo:` returns empty vec.
/// Handles both simple identifiers and dotted names (takes the last part).
fn extract_base_classes(class_node: tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let arg_list = match class_node.child_by_field_name("superclasses") {
        Some(al) => al,
        None => return Vec::new(),
    };

    let mut bases = Vec::new();
    let mut cursor = arg_list.walk();

    for child in arg_list.named_children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                bases.push(node_text(child, source).to_string());
            }
            "attribute" => {
                // For `pydantic.BaseModel`, take the attribute part
                if let Some(attr) = child.child_by_field_name("attribute") {
                    bases.push(node_text(attr, source).to_string());
                }
            }
            "subscript" => {
                // For `RootModel[str]`, take the base name from the value field
                if let Some(value) = child.child_by_field_name("value") {
                    bases.push(node_text(value, source).to_string());
                }
            }
            _ => {}
        }
    }

    bases
}

// -------------------------------------------------------------------------
// v2 dispatch enforcement — only typed dispatch tables
// -------------------------------------------------------------------------

/// In v2 dispatch level: only `dict[type[...], Callable]` assignments allowed.
/// No functions, no classes, no untyped assignments, no other module-level code.
/// Imports are allowed (for structure types and handler function references).
pub fn check_v2_dispatch_only_tables(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let classification = classify::classify_file_v2(source.file_path);
    if !matches!(classification.level, Level::L3 | Level::L6) {
        return Vec::new();
    }

    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for child in root.named_children(&mut cursor) {
        match child.kind() {
            // Imports allowed
            "import_statement" | "import_from_statement" => continue,
            // Docstrings allowed (expression_statement with string)
            "expression_statement" => {
                if is_docstring(child, source.source_bytes) {
                    continue;
                }
                // Typed assignments: check for type annotation
                if is_typed_dispatch_assignment(child, source.source_bytes) {
                    continue;
                }
                violations.push(violation(
                    node_line(child),
                    "dispatch file may only contain typed dispatch tables".to_string(),
                ));
            }
            // Functions and classes forbidden
            "function_definition" | "async_function_definition" => {
                let name = node_field(child, "name")
                    .map(|n| node_text(n, source.source_bytes))
                    .unwrap_or("<unknown>");
                violations.push(violation(
                    node_line(child),
                    format!("function '{name}' in dispatch file (only dispatch tables allowed)"),
                ));
            }
            "class_definition" => {
                let name = node_field(child, "name")
                    .map(|n| node_text(n, source.source_bytes))
                    .unwrap_or("<unknown>");
                violations.push(violation(
                    node_line(child),
                    format!("class '{name}' in dispatch file (only dispatch tables allowed)"),
                ));
            }
            "decorated_definition" => {
                violations.push(violation(
                    node_line(child),
                    "decorated definition in dispatch file (only dispatch tables allowed)".to_string(),
                ));
            }
            // Type alias statements allowed (type X = ...)
            "type_alias_statement" => continue,
            // Everything else is suspicious
            _ => {}
        }
    }

    violations
}

/// Check if an expression_statement is a module docstring (string literal).
fn is_docstring(expr_stmt: tree_sitter::Node, _source: &[u8]) -> bool {
    let mut cursor = expr_stmt.walk();
    let first = match expr_stmt.named_children(&mut cursor).next() {
        Some(n) => n,
        None => return false,
    };
    first.kind() == "string"
}

/// Check if an expression_statement is a typed dispatch assignment.
///
/// Looks for: `NAME: dict[..., Callable...] = { ... }`
/// The assignment must have a type annotation containing "dict" and "Callable".
/// Key types vary (type[Model], str, Enum) — only the Callable value type matters.
fn is_typed_dispatch_assignment(expr_stmt: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = expr_stmt.walk();
    let inner = match expr_stmt.named_children(&mut cursor).next() {
        Some(n) if n.kind() == "assignment" => n,
        _ => return false,
    };

    // Must have a type annotation
    let type_node = match node_field(inner, "type") {
        Some(t) => t,
        None => return false,
    };

    // Must have a right-hand side (the dict value)
    if node_field(inner, "right").is_none() {
        return false;
    }

    // Check the annotation text contains the dispatch table signature markers
    let annotation_text = node_text(type_node, source);
    annotation_text.contains("dict") && annotation_text.contains("Callable")
}

// -------------------------------------------------------------------------
// v2 logic enforcement — no constants in logic/
// -------------------------------------------------------------------------

/// In v2 logic zones: no module-level data bindings.
/// Only function definitions allowed at module level.
pub fn check_v2_logic_no_constants(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let classification = classify::classify_file_v2(source.file_path);
    if classification.zone.is_structure() {
        return Vec::new();
    }
    if matches!(classification.level, Level::L0 | Level::L3 | Level::L6 | Level::Outside) {
        return Vec::new();
    }

    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for child in root.named_children(&mut cursor) {
        if child.kind() == "expression_statement" && is_module_level_constant(child, source.source_bytes) {
            violations.push(violation(
                node_line(child),
                "module-level constant in logic zone (move to structure zone as enum or model)".to_string(),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// unknown_file_in_zone
// -------------------------------------------------------------------------

/// Valid level filenames for logic zones (pure, impure, transform).
const LOGIC_ZONE_FILENAMES: &[&str] = &[
    "ffi.py", "primitive.py", "simple.py", "dispatch.py",
    "composed.py", "assembled.py", "__init__.py",
];

/// Valid level filenames for the orchestrate zone.
const ORCHESTRATE_ZONE_FILENAMES: &[&str] = &[
    "orchestrate.py", "dispatch.py", "__init__.py",
];

/// The valid filenames and the name of a zone that has a filename convention.
/// Structure zones have none, and a file in no zone has none to follow.
fn zone_filename_convention(zone: Zone) -> Option<(&'static [&'static str], &'static str)> {
    match zone {
        Zone::Pure => Some((LOGIC_ZONE_FILENAMES, "pure")),
        Zone::Impure => Some((LOGIC_ZONE_FILENAMES, "impure")),
        Zone::Transform => Some((LOGIC_ZONE_FILENAMES, "transform")),
        Zone::Orchestrate => Some((ORCHESTRATE_ZONE_FILENAMES, "orchestrate")),
        Zone::Structure
        | Zone::StructureGen
        | Zone::StructureModel
        | Zone::StructureConfig
        | Zone::StructureExample
        | Zone::Outside => None,
    }
}

/// Detect files with unrecognized names inside v2 zones.
///
/// Each zone has a fixed set of valid filenames. A file that doesn't match
/// any recognized level name is misplaced — the LLM created a file that
/// the architecture doesn't know how to classify.
pub fn check_unknown_file_in_zone(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let classification = classify::classify_file_v2(source.file_path);

    // Only applies to logic zones where files got Level::Outside
    if classification.level != Level::Outside {
        return Vec::new();
    }

    let filename = source.file_path.rsplit('/').next().unwrap_or("");

    let (valid_names, zone_name) = match zone_filename_convention(classification.zone) {
        Some(convention) => convention,
        None => return Vec::new(), // structure has no filename convention
    };

    if valid_names.contains(&filename) {
        return Vec::new(); // __init__.py is valid and gets Outside legitimately
    }

    let allowed = valid_names.iter()
        .filter(|f| **f != "__init__.py")
        .copied()
        .collect::<Vec<_>>()
        .join(", ");

    vec![violation(
        1,
        format!(
            "'{filename}' is not a valid filename in the {zone_name} zone (valid: {allowed})",
        ),
    )]
}

// -------------------------------------------------------------------------
// no_inline_dispatch
// -------------------------------------------------------------------------

/// Detect dict literals inside functions where all values are bare identifiers
/// (function references). These are inline dispatch tables that should live in
/// a dedicated dispatch.py file.
///
/// Heuristic: a dictionary with 2+ pairs where every value is a bare identifier
/// (function reference), found inside a function body. Attribute access values
/// (e.g. `model.field`) are data access, not callable dispatch — excluded to
/// avoid false positives on model_copy update dicts and edge attribute dicts.
pub fn check_no_inline_dispatch(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let body = match node_field(func, "body") {
            Some(b) => b,
            None => continue,
        };

        for dict_node in find_nodes_by_type(body, "dictionary") {
            let pairs: Vec<_> = {
                let mut cursor = dict_node.walk();
                dict_node
                    .named_children(&mut cursor)
                    .filter(|c| c.kind() == "pair")
                    .collect()
            };

            if pairs.len() < 2 {
                continue;
            }

            let all_callable_values = pairs.iter().all(|pair| {
                let mut cursor = pair.walk();
                let children: Vec<_> = pair.named_children(&mut cursor).collect();
                // pair has key + value as named children; value is the last one
                match children.last() {
                    Some(value) => value.kind() == "identifier",
                    None => false,
                }
            });

            if all_callable_values {
                let func_name = node_field(func, "name")
                    .map(|n| node_text(n, source.source_bytes))
                    .unwrap_or("<unknown>");
                violations.push(violation(
                    node_line(dict_node),
                    format!(
                        "inline dispatch table in '{func_name}' — extract to a dispatch.py file"
                    ),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// (internal helpers)
// -------------------------------------------------------------------------

/// Detect module-level constant assignments.
///
/// Matches: `NAME = {...}`, `NAME = frozenset(...)`, `NAME = [...]`, `NAME = (...)`
/// Skips: `type Name = ...` (type aliases), string literals (module docstrings),
/// `model_rebuild()` calls, and other non-assignment expressions.
fn is_module_level_constant(expr_stmt: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = expr_stmt.walk();
    for child in expr_stmt.named_children(&mut cursor) {
        if child.kind() == "assignment" {
            let left = match child.child_by_field_name("left") {
                Some(n) => n,
                None => continue,
            };
            // Only flag UPPER_CASE or CamelCase assignments that aren't classes
            let name = node_text(left, source);
            if left.kind() != "identifier" {
                continue;
            }
            // Skip type alias style: lowercase or mixed with generic subscripts
            if name.starts_with("type ") {
                continue;
            }
            // Check if RHS is a data literal or constructor
            if let Some(right) = child.child_by_field_name("right") {
                return matches!(
                    right.kind(),
                    "dictionary"
                        | "set"
                        | "list"
                        | "tuple"
                        | "set_comprehension"
                        | "list_comprehension"
                        | "dictionary_comprehension"
                ) || is_frozenset_call(right, source);
            }
        }
    }
    false
}

fn is_frozenset_call(node: tree_sitter::Node, source: &[u8]) -> bool {
    if node.kind() != "call" {
        return false;
    }
    let func = match node.child_by_field_name("function") {
        Some(n) => n,
        None => return false,
    };
    node_text(func, source) == "frozenset"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::build_parsed_source;
    use crate::structures::FileKind;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source("/test/file.py", source).unwrap()
    }

    fn parse_with_path(code: &str, path: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        let path: &'static str = Box::leak(path.to_string().into_boxed_str());
        build_parsed_source(path, source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
    }

    // -- no_methods_in_classes --

    #[test]
    fn method_in_class_caught() {
        let parsed = parse("class Foo:\n    def greet(self):\n        pass\n");
        let violations = check_no_methods_in_classes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("greet"));
        assert!(violations[0].message.contains("Foo"));
    }

    #[test]
    fn decorated_method_in_class_caught() {
        let parsed = parse("class Foo(BaseModel):\n    name: str\n    @field_validator('name')\n    @classmethod\n    def validate_name(cls, v):\n        return v\n");
        let violations = check_no_methods_in_classes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("validate_name"));
        assert!(violations[0].message.contains("Foo"));
    }

    #[test]
    fn class_without_methods_ok() {
        let parsed = parse("class Foo:\n    x: int = 0\n");
        let violations = check_no_methods_in_classes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_callable_protocol --

    #[test]
    fn callable_protocol_caught() {
        let parsed = parse("class MyFunc(Protocol):\n    def __call__(self, x: int) -> str:\n        ...\n");
        let violations = check_no_callable_protocol(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("MyFunc"));
        assert!(violations[0].message.contains("Callable alias"));
    }

    #[test]
    fn protocol_without_call_ok() {
        let parsed = parse("class Readable(Protocol):\n    def read(self) -> bytes:\n        ...\n");
        let violations = check_no_callable_protocol(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn non_protocol_class_ok() {
        let parsed = parse("class Foo:\n    def __call__(self):\n        pass\n");
        let violations = check_no_callable_protocol(&parsed, &default_config());
        assert!(violations.is_empty()); // not a Protocol
    }

    // -- pydantic_only --

    #[test]
    fn dataclass_caught() {
        let parsed = parse("@dataclass\nclass Foo:\n    x: int\n");
        let violations = check_pydantic_only(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn typed_dict_caught() {
        let parsed = parse("class Foo(TypedDict):\n    x: int\n");
        let violations = check_pydantic_only(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn pydantic_model_ok() {
        let parsed = parse("class Foo(BaseModel):\n    x: int\n");
        let violations = check_pydantic_only(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- god_classes --

    #[test]
    fn god_class_caught() {
        let parsed = parse("class Foo:\n    def process(self):\n        pass\n");
        let violations = check_god_classes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Foo"));
        assert!(violations[0].message.contains("process"));
    }

    #[test]
    fn dunder_methods_ok() {
        let parsed = parse("class Foo:\n    def __init__(self):\n        pass\n    def __repr__(self):\n        pass\n");
        let violations = check_god_classes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn test_class_skipped() {
        let parsed = parse("class TestFoo:\n    def process(self):\n        pass\n");
        let violations = check_god_classes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn error_class_skipped() {
        let parsed = parse("class MyError:\n    def process(self):\n        pass\n");
        let violations = check_god_classes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn property_decorator_ok() {
        let parsed = parse("class Foo:\n    @property\n    def name(self):\n        return self._name\n");
        let violations = check_god_classes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_reexport_shims --

    #[test]
    fn reexport_shim_caught() {
        let parsed = parse("from foo import bar\n__all__ = ['bar']\n");
        let violations = check_no_reexport_shims(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("re-export shim"));
    }

    #[test]
    fn module_with_logic_ok() {
        let parsed = parse("from foo import bar\n__all__ = ['bar']\ndef process(): pass\n");
        let violations = check_no_reexport_shims(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn compat_docstring_caught() {
        let parsed = parse("\"\"\"Backward compatibility shim.\"\"\"\nfrom foo import bar\n");
        let violations = check_no_reexport_shims(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("backward compatibility"));
    }

    // -- hardcoded_config --

    #[test]
    fn module_level_dict_caught() {
        let parsed = parse("CONFIG = {'key': 'value'}\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("CONFIG"));
    }

    #[test]
    fn frozenset_constructor_caught() {
        let parsed = parse("KNOWN_SECTIONS = frozenset(['a', 'b', 'c'])\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("KNOWN_SECTIONS"));
    }

    #[test]
    fn set_constructor_caught() {
        let parsed = parse("VALID_TYPES = set(['input', 'output'])\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("VALID_TYPES"));
    }

    #[test]
    fn all_caps_constant_caught() {
        let parsed = parse("MAX_RETRIES = 3\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("MAX_RETRIES"));
    }

    #[test]
    fn dunder_name_ok() {
        let parsed = parse("__version__ = '1.0'\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn lowercase_assignment_ok() {
        let parsed = parse("result = compute()\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- import_count --

    #[test]
    fn few_function_imports_ok() {
        let parsed = parse(
            "from pkg.functions.pure.utils import helper\nfrom pkg.structures.types import MyType\ndef work(): pass\n",
        );
        let violations = check_import_count(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn many_function_imports_caught() {
        let code = (0..5)
            .map(|i| format!("from pkg.functions.pure.reshape_{i} import reshape_{i}\n"))
            .collect::<String>()
            + "def coordinate(): pass\n";
        let parsed = parse(&code);
        let violations = check_import_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("functions/"));
    }

    #[test]
    fn function_imports_at_threshold_ok() {
        let code = (0..3)
            .map(|i| format!("from pkg.functions.pure.mod_{i} import fn_{i}\n"))
            .collect::<String>()
            + "def work(): pass\n";
        let parsed = parse(&code);
        let violations = check_import_count(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn structure_imports_no_limit() {
        let code = (0..10)
            .map(|i| format!("from pkg.structures.types_{i} import Model_{i}\n"))
            .collect::<String>()
            + "def work(): pass\n";
        let parsed = parse(&code);
        let violations = check_import_count(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn many_stdlib_imports_caught() {
        let code = (0..5)
            .map(|i| format!("from stdlib_{i} import thing_{i}\n"))
            .collect::<String>()
            + "def work(): pass\n";
        let parsed = parse(&code);
        let violations = check_import_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("external/stdlib"));
    }

    #[test]
    fn import_count_messages_carry_no_numbers() {
        let code = (0..5)
            .map(|i| format!("from pkg.functions.pure.reshape_{i} import reshape_{i}\n"))
            .chain((0..5).map(|i| format!("from stdlib_{i} import thing_{i}\n")))
            .collect::<String>()
            + "def coordinate(): pass\n";
        let parsed = parse(&code);
        let violations = check_import_count(&parsed, &default_config());
        assert_eq!(violations.len(), 2);
        for viol in &violations {
            assert!(!viol.message.contains(|c: char| c.is_ascii_digit()), "{}", viol.message);
        }
    }

    // -- structures_no_functions --

    #[test]
    fn function_in_structures_caught() {
        let parsed = parse("def helper():\n    pass\n");
        let violations = check_structures_no_functions(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("helper"));
    }

    #[test]
    fn class_in_structures_ok() {
        let parsed = parse("class Foo:\n    x: int = 0\n");
        let violations = check_structures_no_functions(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- structures_import_boundary --

    #[test]
    fn import_from_functions_caught() {
        let parsed = parse("from myproject.functions.pure.helper import process\n");
        let violations = check_structures_import_boundary(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("outside data zone"));
    }

    #[test]
    fn pydantic_import_ok() {
        let parsed = parse("from pydantic import BaseModel\n");
        let violations = check_structures_import_boundary(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn structures_internal_import_ok() {
        let parsed = parse("from myproject.structures.base import BaseEntity\n");
        let violations = check_structures_import_boundary(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn pydantic_validators_ok() {
        let parsed = parse("from myproject.pydantic_validators import check_name\n");
        let violations = check_structures_import_boundary(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- classes_only_in_structures --

    #[test]
    fn class_outside_structures_caught() {
        let parsed = parse("class Foo:\n    x: int = 0\n");
        let violations = check_classes_only_in_structures(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Foo"));
        assert!(violations[0].message.contains("outside structures"));
    }

    #[test]
    fn no_classes_ok() {
        let parsed = parse("def helper(): pass\n");
        let violations = check_classes_only_in_structures(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- max_functions_outside_zones --

    #[test]
    fn few_functions_ok() {
        let parsed = parse("def a(): pass\ndef b(): pass\ndef c(): pass\n");
        let violations = check_max_functions_outside_zones(&parsed, &default_config());
        assert!(violations.is_empty()); // 3 functions, max 3
    }

    #[test]
    fn too_many_functions_caught() {
        let parsed = parse("def a(): pass\ndef b(): pass\ndef c(): pass\ndef d(): pass\n");
        let violations = check_max_functions_outside_zones(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("too many"));
        // Verify fuzzy — no counts in message
        assert!(!violations[0].message.contains("4"));
        assert!(!violations[0].message.contains("max"));
    }

    #[test]
    fn private_functions_skipped() {
        let parsed = parse("def _a(): pass\ndef _b(): pass\ndef _c(): pass\ndef _d(): pass\ndef public(): pass\n");
        let violations = check_max_functions_outside_zones(&parsed, &default_config());
        assert!(violations.is_empty()); // Only 1 public function
    }

    #[test]
    fn test_functions_skipped() {
        let parsed = parse("def test_a(): pass\ndef test_b(): pass\ndef test_c(): pass\ndef test_d(): pass\n");
        let violations = check_max_functions_outside_zones(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- v2_structure_no_logic --

    #[test]
    fn v2_structure_function_caught() {
        let parsed = parse_with_path(
            "def helper():\n    return 1\n",
            "/project/src/pkg/structure/models.py",
        );
        let violations = check_v2_structure_no_logic(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("function"));
    }

    #[test]
    fn v2_structure_class_ok() {
        let parsed = parse_with_path(
            "from pydantic import BaseModel\nclass Foo(BaseModel):\n    x: int\n",
            "/project/src/pkg/structure/models.py",
        );
        let violations = check_v2_structure_no_logic(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_structure_constant_caught() {
        let parsed = parse_with_path(
            "LOOKUP = {\"a\": 1, \"b\": 2}\n",
            "/project/src/pkg/structure/constants.py",
        );
        let violations = check_v2_structure_no_logic(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("constant"));
    }

    #[test]
    fn v2_structure_type_alias_ok() {
        let parsed = parse_with_path(
            "type FieldRef = int | str\n",
            "/project/src/pkg/structure/types.py",
        );
        let violations = check_v2_structure_no_logic(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_non_structure_skipped() {
        let parsed = parse_with_path(
            "def helper():\n    return 1\n",
            "/project/src/pkg/logic/pure/helpers/primitive.py",
        );
        let violations = check_v2_structure_no_logic(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- v2_structure_bases --

    #[test]
    fn v2_structure_plain_class_caught() {
        let parsed = parse_with_path(
            "class Foo:\n    x: int = 0\n",
            "/project/src/pkg/structure/models.py",
        );
        let violations = check_v2_structure_bases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("no base class"));
    }

    #[test]
    fn v2_structure_basemodel_ok() {
        let parsed = parse_with_path(
            "from pydantic import BaseModel\nclass Foo(BaseModel):\n    x: int\n",
            "/project/src/pkg/structure/models.py",
        );
        let violations = check_v2_structure_bases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_structure_enum_ok() {
        let parsed = parse_with_path(
            "from enum import Enum\nclass Color(Enum):\n    red = 1\n",
            "/project/src/pkg/structure/enums.py",
        );
        let violations = check_v2_structure_bases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_structure_unknown_base_caught() {
        let parsed = parse_with_path(
            "class Foo(SomeRandomBase):\n    x: int = 0\n",
            "/project/src/pkg/structure/models.py",
        );
        let violations = check_v2_structure_bases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("unknown base"));
    }

    #[test]
    fn v2_structure_non_structure_skipped() {
        let parsed = parse_with_path(
            "class Foo:\n    x: int = 0\n",
            "/project/src/pkg/logic/pure/helpers/primitive.py",
        );
        let violations = check_v2_structure_bases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- v2_logic_no_constants --

    #[test]
    fn v2_logic_constant_caught() {
        let parsed = parse_with_path(
            "DEFAULTS = {\"key\": \"value\"}\n",
            "/project/src/pkg/logic/pure/helpers/primitive.py",
        );
        let violations = check_v2_logic_no_constants(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("constant"));
    }

    #[test]
    fn v2_logic_frozenset_caught() {
        let parsed = parse_with_path(
            "from enum import Enum\nclass Cap(Enum):\n    read = 1\nREAD_SET = frozenset({Cap.read})\n",
            "/project/src/pkg/logic/pure/helpers/primitive.py",
        );
        let violations = check_v2_logic_no_constants(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn v2_logic_function_ok() {
        let parsed = parse_with_path(
            "def helper():\n    return 1\n",
            "/project/src/pkg/logic/pure/helpers/primitive.py",
        );
        let violations = check_v2_logic_no_constants(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_logic_in_structure_skipped() {
        let parsed = parse_with_path(
            "DEFAULTS = {\"key\": \"value\"}\n",
            "/project/src/pkg/structure/config.py",
        );
        let violations = check_v2_logic_no_constants(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- v2_dispatch_only_tables --

    #[test]
    fn v2_dispatch_typed_table_ok() {
        let code = "from typing import Callable\nfrom structures.models import ModelA, ModelB\nfrom logic.pure.simple.converters import convert_a, convert_b\n\nDISPATCH: dict[type[ModelA], Callable] = {\n    ModelA: convert_a,\n    ModelB: convert_b,\n}\n";
        let parsed = parse_with_path(code, "/project/src/pkg/logic/pure/converters/dispatch.py");
        let violations = check_v2_dispatch_only_tables(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_dispatch_function_caught() {
        let code = "def helper():\n    return 1\n";
        let parsed = parse_with_path(code, "/project/src/pkg/logic/pure/converters/dispatch.py");
        let violations = check_v2_dispatch_only_tables(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("function"));
    }

    #[test]
    fn v2_dispatch_untyped_dict_caught() {
        let code = "LOOKUP = {'a': 1, 'b': 2}\n";
        let parsed = parse_with_path(code, "/project/src/pkg/logic/pure/converters/dispatch.py");
        let violations = check_v2_dispatch_only_tables(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("dispatch tables"));
    }

    #[test]
    fn v2_dispatch_non_dispatch_file_skipped() {
        let code = "def helper():\n    return 1\n";
        let parsed = parse_with_path(code, "/project/src/pkg/logic/pure/helpers/simple.py");
        let violations = check_v2_dispatch_only_tables(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_dispatch_class_caught() {
        let code = "class Foo:\n    x: int = 0\n";
        let parsed = parse_with_path(code, "/project/src/pkg/logic/transform/models/dispatch.py");
        let violations = check_v2_dispatch_only_tables(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("class"));
    }

    #[test]
    fn v2_dispatch_docstring_ok() {
        let code = "\"\"\"Dispatch tables for time converters.\"\"\"\nfrom typing import Callable\n";
        let parsed = parse_with_path(code, "/project/src/pkg/logic/impure/time/dispatch.py");
        let violations = check_v2_dispatch_only_tables(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- unknown_file_in_zone --

    #[test]
    fn unknown_file_in_pure_caught() {
        let parsed = parse_with_path(
            "x: int = 1\n",
            "/project/src/pkg/logic/pure/helpers/utils.py",
        );
        let violations = check_unknown_file_in_zone(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("utils.py"));
        assert!(violations[0].message.contains("pure"));
    }

    #[test]
    fn valid_file_in_pure_ok() {
        let parsed = parse_with_path(
            "def add(a: int, b: int) -> int:\n    return a + b\n",
            "/project/src/pkg/logic/pure/math/simple.py",
        );
        let violations = check_unknown_file_in_zone(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn unknown_file_in_orchestrate_caught() {
        let parsed = parse_with_path(
            "x: int = 1\n",
            "/project/src/pkg/logic/orchestrate/pipeline/main.py",
        );
        let violations = check_unknown_file_in_zone(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("main.py"));
        assert!(violations[0].message.contains("orchestrate"));
    }

    #[test]
    fn structure_zone_any_filename_ok() {
        let parsed = parse_with_path(
            "class Foo:\n    x: int = 0\n",
            "/project/src/pkg/structure/model/anything.py",
        );
        let violations = check_unknown_file_in_zone(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn init_file_in_zone_ok() {
        let parsed = parse_with_path(
            "\n",
            "/project/src/pkg/logic/pure/helpers/__init__.py",
        );
        let violations = check_unknown_file_in_zone(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- v2_classes_only_in_structure --

    #[test]
    fn v2_class_outside_structure_caught() {
        let parsed = parse_with_path(
            "class Foo:\n    x: int = 0\n",
            "/project/src/pkg/logic/pure/helpers/composed.py",
        );
        let violations = check_v2_classes_only_in_structure(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("outside structure zone"));
    }

    #[test]
    fn v2_no_classes_in_logic_ok() {
        let parsed = parse_with_path(
            "def helper(): pass\n",
            "/project/src/pkg/logic/pure/helpers/composed.py",
        );
        let violations = check_v2_classes_only_in_structure(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- v2_structure_import_boundary --

    #[test]
    fn v2_structure_internal_import_ok() {
        let parsed = parse_with_path(
            "from regin.structure.model.gate_types import GateResult\n",
            "/project/src/pkg/structure/model/foo.py",
        );
        let violations = check_v2_structure_import_boundary(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_structure_pydantic_import_ok() {
        let parsed = parse_with_path(
            "from pydantic import BaseModel\n",
            "/project/src/pkg/structure/model/foo.py",
        );
        let violations = check_v2_structure_import_boundary(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_structure_logic_import_caught() {
        let parsed = parse_with_path(
            "from regin.logic.pure.helpers.composed import something\n",
            "/project/src/pkg/structure/model/foo.py",
        );
        let violations = check_v2_structure_import_boundary(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("outside data zone"));
    }

    // -- v2_structure_bases with RootModel --

    #[test]
    fn v2_structure_rootmodel_ok() {
        let parsed = parse_with_path(
            "from pydantic import RootModel\nclass AgentName(RootModel[str]):\n    root: str\n",
            "/project/src/pkg/structure/gen/schema/models.py",
        );
        let violations = check_v2_structure_bases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_inline_dispatch --

    #[test]
    fn inline_dispatch_caught() {
        let parsed = parse(
            "def compile(root, graph):\n    dispatch = {\n        \"pattern\": build_pattern,\n        \"enum\": build_enum,\n    }\n",
        );
        let violations = check_no_inline_dispatch(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("dispatch table"));
        assert!(violations[0].message.contains("compile"));
    }

    #[test]
    fn inline_dispatch_enum_keys_caught() {
        let parsed = parse(
            "def run(args):\n    handlers = {\n        Mode.generate: run_generate,\n        Mode.check: run_check,\n    }\n",
        );
        let violations = check_no_inline_dispatch(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("run"));
    }

    #[test]
    fn data_dict_ok() {
        let parsed = parse(
            "def build():\n    config = {\n        \"key\": \"value\",\n        \"other\": \"data\",\n    }\n",
        );
        let violations = check_no_inline_dispatch(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn mixed_values_ok() {
        let parsed = parse(
            "def build():\n    info = {\n        \"name\": some_var,\n        \"count\": 42,\n    }\n",
        );
        let violations = check_no_inline_dispatch(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn single_entry_ok() {
        let parsed = parse(
            "def build():\n    lookup = {\n        \"only\": some_func,\n    }\n",
        );
        let violations = check_no_inline_dispatch(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn attribute_values_not_caught() {
        // Data dicts with attribute access values (model.field) are not dispatch tables
        let parsed = parse(
            "def build(model, meta):\n    updates = {\n        \"title\": model.title.root,\n        \"description\": meta.description,\n    }\n",
        );
        let violations = check_no_inline_dispatch(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn module_level_dict_not_caught() {
        let parsed = parse(
            "DISPATCH = {\n    \"a\": func_a,\n    \"b\": func_b,\n}\n",
        );
        let violations = check_no_inline_dispatch(&parsed, &default_config());
        assert!(violations.is_empty()); // only catches function-local
    }
}
