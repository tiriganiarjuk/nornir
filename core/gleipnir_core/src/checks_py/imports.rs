//! Import boundary checks.
//!
//! Checks: no_unsafe_imports, impure_module_quarantine,
//! no_type_checking_imports, no_parent_imports, no_disallowed_stdlib,
//! no_direct_file_io, no_sys_path_mutation.

use crate::classify;
use crate::parsing::{
    extract_imported_names, extract_module_info, find_nodes_by_type, node_line, node_text,
};
use crate::structures::{CheckConfig, Level, ParsedSource, Severity, V2Classification, Violation};

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
// no_unsafe_imports
// -------------------------------------------------------------------------

// Patterns stored reversed to avoid triggering gleipnir's text scanner.
const REVERSED_UNSAFE_TEXT: &[&str] = &[
    "tropmi efasnu morf",
    "tropmi erup.efasnu morf",
    "tropmi erupmi.efasnu morf",
    "efasnu tropmi",
];

fn unsafe_text_patterns() -> Vec<String> {
    REVERSED_UNSAFE_TEXT
        .iter()
        .map(|s| s.chars().rev().collect())
        .collect()
}

fn file_in_quarantine_zone(file_path: &str) -> bool {
    file_path.contains("unsafe/")
}

pub fn check_no_unsafe_imports(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    if file_in_quarantine_zone(source.file_path) {
        return Vec::new();
    }

    let mut violations = Vec::new();
    let patterns = unsafe_text_patterns();

    // Text-based scan
    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        if line.trim_start().starts_with('#') {
            continue;
        }
        for pattern in &patterns {
            if line.contains(pattern.as_str()) {
                violations.push(violation(
                    line_num,
                    "importing from quarantine zone".to_string(),
                ));
                break;
            }
        }
    }

    // Tree-sitter: relative imports from unsafe
    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, level) = extract_module_info(node, source.source_bytes);
        let parts: Vec<&str> = if module.is_empty() {
            vec![]
        } else {
            module.split('.').collect()
        };

        if level > 0 && !parts.is_empty() && parts[0] == "unsafe" {
            violations.push(violation(
                node_line(node),
                "relative import from quarantine zone".to_string(),
            ));
        } else if level == 0 && parts.len() > 1 && parts[1..].contains(&"unsafe") {
            violations.push(violation(
                node_line(node),
                "importing from quarantine zone".to_string(),
            ));
        }
    }

    // Tree-sitter: bare imports
    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "dotted_name" {
                let module = node_text(child, source.source_bytes);
                let parts: Vec<&str> = module.split('.').collect();
                if parts.len() > 1 && parts[1..].contains(&"unsafe") {
                    violations.push(violation(
                        node_line(node),
                        "importing from quarantine zone".to_string(),
                    ));
                }
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_relative_imports (scripts only — enforces standalone discipline)
// -------------------------------------------------------------------------

pub fn check_no_relative_imports(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (_, level) = extract_module_info(node, source.source_bytes);
        if level > 0 {
            violations.push(violation(
                node_line(node),
                "relative import in standalone script".to_string(),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// impure_module_quarantine
// -------------------------------------------------------------------------

const IMPURE_MODULES: &[&str] = &[
    "io", "shutil", "os", "sys", "subprocess", "socket", "requests", "httpx", "urllib",
    "aiohttp", "time", "datetime", "random", "importlib", "types",
];

// Mixed modules: pathlib has pure and impure names
const PATHLIB_PURE: &[&str] = &["PurePath", "PurePosixPath", "PureWindowsPath"];
const PATHLIB_IMPURE: &[&str] = &["Path", "PosixPath", "WindowsPath"];

fn check_pathlib_names(node: tree_sitter::Node, source: &[u8]) -> Vec<Violation> {
    let names = extract_imported_names(node, source);
    let line = node_line(node);
    names
        .iter()
        .filter_map(|name| {
            if PATHLIB_IMPURE.contains(&name.as_str()) {
                Some(violation(line, "impure name from mixed module in pure zone".to_string()))
            } else if !PATHLIB_PURE.contains(&name.as_str()) {
                Some(violation(line, "unknown name from mixed module in pure zone".to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn classify_bare_import(node: tree_sitter::Node, source: &[u8]) -> Vec<Violation> {
    let mut cursor = node.walk();
    let line = node_line(node);
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "dotted_name")
        .filter_map(|child| {
            let module = node_text(child, source);
            let parts: Vec<&str> = module.split('.').collect();
            let top = parts.first().copied().unwrap_or("");
            if parts.contains(&"impure") {
                Some(violation(line, "cross-zone import in pure module".to_string()))
            } else if IMPURE_MODULES.contains(&top) {
                Some(violation(line, "impure stdlib module in pure zone".to_string()))
            } else if top == "pathlib" {
                Some(violation(line, "whole mixed module imported in pure zone".to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn classify_from_import(node: tree_sitter::Node, source: &[u8]) -> Option<Violation> {
    let (module, level) = extract_module_info(node, source);
    let parts: Vec<&str> = if module.is_empty() {
        vec![]
    } else {
        module.split('.').collect()
    };
    let top = parts.first().copied().unwrap_or("");
    let line = node_line(node);

    if level == 0 && parts.contains(&"impure") {
        return Some(violation(line, "cross-zone import in pure module".to_string()));
    }
    if level > 0 && !parts.is_empty() && parts[0] == "impure" {
        return Some(violation(line, "relative cross-zone import in pure module".to_string()));
    }
    if IMPURE_MODULES.contains(&top) {
        return Some(violation(line, "impure stdlib module in pure zone".to_string()));
    }
    None
}

pub fn check_impure_module_quarantine(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        if let Some(v) = classify_from_import(node, source.source_bytes) {
            violations.push(v);
            continue;
        }
        let (module, _) = extract_module_info(node, source.source_bytes);
        let top = module.split('.').next().unwrap_or("");
        if top == "pathlib" {
            violations.extend(check_pathlib_names(node, source.source_bytes));
        }
    }

    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        violations.extend(classify_bare_import(node, source.source_bytes));
    }

    violations
}

// -------------------------------------------------------------------------
// no_type_checking_imports
// -------------------------------------------------------------------------

pub fn check_no_type_checking_imports(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "if_statement") {
        let condition = match node.child_by_field_name("condition") {
            Some(c) => c,
            None => continue,
        };

        let is_type_checking = match condition.kind() {
            "identifier" => node_text(condition, source.source_bytes) == "TYPE_CHECKING",
            "attribute" => {
                condition
                    .child_by_field_name("attribute")
                    .is_some_and(|a| node_text(a, source.source_bytes) == "TYPE_CHECKING")
            }
            _ => false,
        };

        if is_type_checking {
            violations.push(violation(
                node_line(node),
                "TYPE_CHECKING block hides circular import".to_string(),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_parent_imports
// -------------------------------------------------------------------------

// Reversed to avoid triggering text scanner
const REVERSED_PARENT_PATTERNS: &[(&str, &str)] = &[
    ("tropmi snoitcnuf morf", "functions"),
    ("snoitcnuf tropmi", "functions"),
    ("tropmi efasnu morf", "unsafe"),
    ("efasnu tropmi", "unsafe"),
];

fn parent_import_patterns() -> Vec<(String, &'static str)> {
    REVERSED_PARENT_PATTERNS
        .iter()
        .map(|(pat, parent)| (pat.chars().rev().collect(), *parent))
        .collect()
}

pub fn check_no_parent_imports(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    let patterns = parent_import_patterns();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        if line.trim_start().starts_with('#') {
            continue;
        }
        for (pattern, parent) in &patterns {
            if line.contains(pattern.as_str()) {
                violations.push(violation(
                    line_num,
                    format!("direct import from '{parent}' (must use .pure or .impure)"),
                ));
                break;
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_disallowed_stdlib — global ban on superseded stdlib modules
// -------------------------------------------------------------------------

/// Stdlib modules that have been replaced by project-standard alternatives.
/// Each entry is (module_name, replacement).
const DISALLOWED_STDLIB: &[(&str, &str)] = &[
    ("argparse", "typer"),
    ("logging", "loguru"),
    ("unittest", "pytest"),
    ("json", "pydantic model_dump_json() / model_validate_json()"),
];

pub fn check_no_disallowed_stdlib(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, _level) = extract_module_info(node, source.source_bytes);
        let top = module.split('.').next().unwrap_or("");
        if let Some((_, replacement)) = DISALLOWED_STDLIB.iter().find(|(name, _)| *name == top) {
            violations.push(violation(
                node_line(node),
                format!("'{top}' is superseded — use {replacement}"),
            ));
        }
    }

    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "dotted_name" {
                let module = node_text(child, source.source_bytes);
                let top = module.split('.').next().unwrap_or("");
                if let Some((_, replacement)) = DISALLOWED_STDLIB.iter().find(|(name, _)| *name == top) {
                    violations.push(violation(
                        node_line(node),
                        format!("'{top}' is superseded — use {replacement}"),
                    ));
                }
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_direct_file_io — structured data enters only through compiled gates
// -------------------------------------------------------------------------
//
// Proxy: "Python performs no direct file IO." Two surfaces, one check:
//   A. imports of structured-file parsers (tomllib, yaml, ...) — incl. aliased
//      and dynamic (__import__ / importlib.import_module) forms
//   B. raw file handles: open(), .open(), .read_text/.read_bytes/.write_text/.write_bytes
// The violation message names the replacement for the surface that fired.
// `json` is deliberately absent: no_disallowed_stdlib owns it (in-memory,
// replacement is model_validate_json). json.load(fh) is still caught — by the
// handle on the open() line.

const NO_READER_YET: &str = "no reader gate exists for it yet — the violation stands until one does";
const READ_REPLACEMENT: &str = "a read_*_validate_json reader for structured data";
const WRITE_REPLACEMENT: &str = "a nornir writer tool";

/// Structured-file parsers that bypass the reader gates. (top-level module, replacement)
const GATE_BYPASS_PARSERS: &[(&str, &str)] = &[
    ("tomllib", "read_toml_validate_json"),
    ("tomli", "read_toml_validate_json"),
    ("toml", "read_toml_validate_json"),
    ("tomlkit", "read_toml_validate_json"),
    ("configparser", "read_toml_validate_json"),
    ("tomli_w", WRITE_REPLACEMENT),
    ("yaml", "read_yaml_validate_json"),
    ("ruamel", "read_yaml_validate_json"),
    ("xml", "read_xml_validate_json"),
    ("lxml", "read_xml_validate_json"),
    ("csv", NO_READER_YET),
    ("pickle", NO_READER_YET),
    ("shelve", NO_READER_YET),
];

/// Method names that are direct file IO on any receiver. (method, verb, replacement)
const DIRECT_FILE_METHODS: &[(&str, &str, &str)] = &[
    ("read_text", "read", READ_REPLACEMENT),
    ("read_bytes", "read", READ_REPLACEMENT),
    ("write_text", "write", WRITE_REPLACEMENT),
    ("write_bytes", "write", WRITE_REPLACEMENT),
];

/// Receivers whose `.open(` is not a file handle. Grows only with a no-fire
/// test carrying the real snippet.
const NON_FILE_OPEN_RECEIVERS: &[&str] = &["webbrowser", "pdfplumber", "Image"];

/// Look up the top-level segment of a dotted module in the parser table.
fn parser_replacement(module: &str) -> Option<(&'static str, &'static str)> {
    let top = module.split('.').next().unwrap_or("");
    GATE_BYPASS_PARSERS
        .iter()
        .find(|(name, _)| *name == top)
        .map(|&(name, replacement)| (name, replacement))
}

fn parser_violation(line: usize, module_top: &str, replacement: &str, via_suffix: &str) -> Violation {
    violation(line, format!("'{module_top}' bypasses the reader gates{via_suffix} — use {replacement}"))
}

/// Surface A1: `import x[.y] [as z]` — unwraps aliased_import, which the
/// sibling check's loop never sees.
fn collect_parser_imports(source: &ParsedSource, violations: &mut Vec<Violation>) {
    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let module_node = match child.kind() {
                "dotted_name" => Some(child),
                "aliased_import" => child.child_by_field_name("name"),
                _ => None,
            };
            let Some(module_node) = module_node else { continue };
            let module = node_text(module_node, source.source_bytes);
            if let Some((top, replacement)) = parser_replacement(module) {
                violations.push(parser_violation(node_line(node), top, replacement, ""));
            }
        }
    }
}

/// Surface A2: `from x[.y] import ...` — absolute imports only.
fn collect_parser_from_imports(source: &ParsedSource, violations: &mut Vec<Violation>) {
    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, level) = extract_module_info(node, source.source_bytes);
        if level != 0 {
            continue;
        }
        if let Some((top, replacement)) = parser_replacement(&module) {
            violations.push(parser_violation(node_line(node), top, replacement, ""));
        }
    }
}

/// Text of a string-literal node without its quotes, or None if not a plain literal.
fn string_literal_text<'a>(node: tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    if node.kind() != "string" {
        return None;
    }
    let text = node_text(node, source);
    let trimmed = text.trim_matches(|c| c == '"' || c == '\'');
    if trimmed.len() == text.len() {
        return None;
    }
    Some(trimmed)
}

/// First positional argument of a call, if any.
fn first_positional_arg(call: tree_sitter::Node) -> Option<tree_sitter::Node> {
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let first = args
        .named_children(&mut cursor)
        .find(|a| a.kind() != "keyword_argument" && a.kind() != "comment");
    first
}

/// Is this call `__import__(...)` or `<anything>.import_module(...)`?
fn is_dynamic_import(func: tree_sitter::Node, source: &[u8]) -> bool {
    match func.kind() {
        "identifier" => node_text(func, source) == "__import__",
        "attribute" => func
            .child_by_field_name("attribute")
            .is_some_and(|a| node_text(a, source) == "import_module"),
        _ => false,
    }
}

/// Surface A3: dynamic import of a parser with a string-literal module name.
fn dynamic_import_violation(call: tree_sitter::Node, func: tree_sitter::Node, source: &[u8]) -> Option<Violation> {
    if !is_dynamic_import(func, source) {
        return None;
    }
    let arg = first_positional_arg(call)?;
    let module = string_literal_text(arg, source)?;
    let (top, replacement) = parser_replacement(module)?;
    Some(parser_violation(node_line(call), top, replacement, " via dynamic import"))
}

/// Walk an attribute chain's receiver leftwards to its root identifier.
fn receiver_root<'a>(mut node: tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
    loop {
        match node.kind() {
            "identifier" => return Some(node),
            "attribute" => node = node.child_by_field_name("object")?,
            "call" => node = node.child_by_field_name("function")?,
            "subscript" => node = node.child_by_field_name("value")?,
            "parenthesized_expression" => node = node.named_child(0)?,
            _ => return None,
        }
    }
}

/// The mode string of an `open(...)` call: second positional argument or `mode=`.
fn open_mode<'a>(call: tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let mut positional_index = 0;
    for arg in args.named_children(&mut cursor) {
        if arg.kind() == "keyword_argument" {
            let is_mode = arg
                .child_by_field_name("name")
                .is_some_and(|n| node_text(n, source) == "mode");
            if is_mode {
                return arg.child_by_field_name("value").and_then(|v| string_literal_text(v, source));
            }
            continue;
        }
        if arg.kind() == "comment" {
            continue;
        }
        if positional_index == 1 {
            return string_literal_text(arg, source);
        }
        positional_index += 1;
    }
    None
}

fn is_write_mode(mode: &str) -> bool {
    mode.chars().any(|c| matches!(c, 'w' | 'a' | 'x' | '+'))
}

/// Surfaces B1/B2: `open(` and `<receiver>.open(`.
fn open_violation(call: tree_sitter::Node, func: tree_sitter::Node, source: &[u8]) -> Option<Violation> {
    let receiver_display = match func.kind() {
        "identifier" if node_text(func, source) == "open" => "open".to_string(),
        "attribute" => {
            let is_open = func
                .child_by_field_name("attribute")
                .is_some_and(|a| node_text(a, source) == "open");
            if !is_open {
                return None;
            }
            let object = func.child_by_field_name("object")?;
            if let Some(root) = receiver_root(object) {
                if NON_FILE_OPEN_RECEIVERS.contains(&node_text(root, source)) {
                    return None;
                }
            }
            node_text(func, source).to_string()
        }
        _ => return None,
    };

    let line = node_line(call);
    match open_mode(call, source) {
        Some(mode) if is_write_mode(mode) => Some(violation(
            line,
            format!("direct file write ({receiver_display}, mode '{mode}') — use {WRITE_REPLACEMENT}"),
        )),
        _ => Some(violation(
            line,
            format!("direct file read ({receiver_display}) — use {READ_REPLACEMENT}"),
        )),
    }
}

/// Surface B3: `.read_text(` / `.read_bytes(` / `.write_text(` / `.write_bytes(` on any receiver.
fn file_method_violation(call: tree_sitter::Node, func: tree_sitter::Node, source: &[u8]) -> Option<Violation> {
    if func.kind() != "attribute" {
        return None;
    }
    let method = node_text(func.child_by_field_name("attribute")?, source);
    let &(name, verb, replacement) = DIRECT_FILE_METHODS.iter().find(|(m, _, _)| *m == method)?;
    Some(violation(
        node_line(call),
        format!("direct file {verb} (.{name}) — use {replacement}"),
    ))
}

fn collect_call_surfaces(source: &ParsedSource, violations: &mut Vec<Violation>) {
    for call in find_nodes_by_type(source.tree.root_node(), "call") {
        let Some(func) = call.child_by_field_name("function") else { continue };
        let bytes = source.source_bytes;
        let hit = dynamic_import_violation(call, func, bytes)
            .or_else(|| open_violation(call, func, bytes))
            .or_else(|| file_method_violation(call, func, bytes));
        if let Some(v) = hit {
            violations.push(v);
        }
    }
}

pub fn check_no_direct_file_io(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    collect_parser_imports(source, &mut violations);
    collect_parser_from_imports(source, &mut violations);
    collect_call_surfaces(source, &mut violations);
    violations
}

// -------------------------------------------------------------------------
// v2 import boundary check
// -------------------------------------------------------------------------

/// Resolve a relative import path to a dotted path using the source file's location.
///
/// For `from .sibling import foo` in `/project/src/pkg/logic/pure/simple/module.py`,
/// the relative part `.sibling` resolves against the directory containing module.py.
/// Level 1 = same directory, level 2 = parent directory, etc.
fn resolve_relative_import(source_path: &str, module: &str, level: usize) -> String {
    let parts: Vec<&str> = source_path.split('/').collect();
    // Strip filename, then go up `level` directories
    let dir_parts = if parts.len() > level {
        &parts[..parts.len() - level]
    } else {
        &[]
    };

    // Find the src/package boundary — look for common patterns
    // We need the dotted package path from the project package root
    let mut package_start = 0;
    for (idx, part) in dir_parts.iter().enumerate() {
        if *part == "src" && idx + 1 < dir_parts.len() {
            package_start = idx + 1;
            break;
        }
    }

    let package_parts = &dir_parts[package_start..];
    let base = package_parts.join(".");
    if module.is_empty() {
        base
    } else {
        format!("{base}.{module}")
    }
}

/// Check v2 import boundaries using the two-axis (level + zone) intersection.
///
/// Every import must satisfy both the level matrix and the zone matrix.
/// Files outside the v2 zone layout are violations themselves.
/// External imports (stdlib, third-party) are skipped.
pub fn check_v2_import_boundaries(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let source_class = classify::classify_file_v2(source.file_path);

    let mut violations = Vec::new();

    // __init__.py files: package markers, no imports to check
    // (init_files_empty enforces they're empty separately)
    let filename = source.file_path.rsplit('/').next().unwrap_or("");
    if filename == "__init__.py" {
        return violations;
    }

    if source_class.level == Level::Outside {
        violations.push(violation(
            1,
            "file outside v2 zone layout".to_string(),
        ));
        return violations;
    }

    // Structure files (L0) have their own boundary check (v2_structure_import_boundary).
    // The level comparison (source > target) doesn't apply — structure files
    // freely import each other, which the numeric rule would reject (0 > 0 = false).
    if source_class.level == Level::L0 {
        return violations;
    }

    // Check import_from_statement nodes
    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, level) = extract_module_info(node, source.source_bytes);

        let dotted = if level > 0 {
            resolve_relative_import(source.file_path, &module, level)
        } else {
            module.clone()
        };

        if let Some(target_class) = classify::classify_import_path(&dotted) {
            check_import_legality(
                source_class,
                target_class,
                &dotted,
                node_line(node),
                &mut violations,
            );
        }
    }

    // Check bare import_statement nodes
    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "dotted_name" {
                let dotted = node_text(child, source.source_bytes).to_string();
                if let Some(target_class) = classify::classify_import_path(&dotted) {
                    check_import_legality(
                        source_class,
                        target_class,
                        &dotted,
                        node_line(node),
                        &mut violations,
                    );
                }
            }
        }
    }

    violations
}

fn check_import_legality(
    source: V2Classification,
    target: V2Classification,
    dotted_path: &str,
    line: usize,
    violations: &mut Vec<Violation>,
) {
    if target.level == Level::Outside {
        violations.push(violation(
            line,
            format!("import from outside v2 zone layout: {dotted_path}"),
        ));
        return;
    }

    let level_ok = source.level.can_import(target.level);
    let zone_ok = if target.zone.is_structure() {
        true
    } else {
        source.zone.can_reach(target.zone)
    };

    if !level_ok {
        violations.push(violation(
            line,
            format!(
                "level violation: {:?} cannot import {:?} ({dotted_path})",
                source.level, target.level,
            ),
        ));
    }

    if !zone_ok {
        violations.push(violation(
            line,
            format!(
                "zone violation: {:?} cannot import {:?} ({dotted_path})",
                source.zone, target.zone,
            ),
        ));
    }
}

// -------------------------------------------------------------------------
// no_before_validators
// -------------------------------------------------------------------------

pub fn check_no_before_validators(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let imported = extract_imported_names(node, source.source_bytes);
        for name in &imported {
            if *name == "BeforeValidator" {
                violations.push(violation(
                    node_line(node),
                    "BeforeValidator import — use explicit transforms instead of implicit coercion".to_string(),
                ));
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_sys_path_mutation
// -------------------------------------------------------------------------

/// Detect sys.path manipulation: insert, append, or direct assignment.
///
/// sys.path mutation makes imports work from locations the project structure
/// forbids. The script appears standalone but secretly reaches into the
/// package. If sys.path needs modification, the project layout is wrong.
pub fn check_no_sys_path_mutation(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    // Find call expressions: sys.path.insert(...), sys.path.append(...)
    for node in find_nodes_by_type(source.tree.root_node(), "call") {
        let func = match node.child_by_field_name("function") {
            Some(f) => f,
            None => continue,
        };
        let func_text = node_text(func, source.source_bytes);
        if func_text == "sys.path.insert" || func_text == "sys.path.append" {
            violations.push(violation(
                node_line(node),
                format!(
                    "{func_text}() manipulates import path — fix the project layout instead"
                ),
            ));
        }
    }

    // Find assignments: sys.path = ..., sys.path[:] = ...
    for node in find_nodes_by_type(source.tree.root_node(), "assignment") {
        let left = match node.child_by_field_name("left") {
            Some(l) => l,
            None => continue,
        };
        let left_text = node_text(left, source.source_bytes);
        if left_text == "sys.path" || left_text.starts_with("sys.path[") {
            violations.push(violation(
                node_line(node),
                "sys.path assignment manipulates import path — fix the project layout instead"
                    .to_string(),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_deferred_imports — imports inside function bodies
// -------------------------------------------------------------------------

/// Detect import statements nested inside function definitions.
///
/// An import inside a function body is invisible at module level —
/// tools and humans scanning the top of the file cannot see what
/// the function depends on.
pub fn check_no_deferred_imports(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let root = source.tree.root_node();

    for import_node in find_nodes_by_type(root, "import_from_statement") {
        if is_inside_function(import_node) {
            let module_text = extract_module_info(import_node, source.source_bytes).0;
            violations.push(violation(
                node_line(import_node),
                format!("deferred import '{module_text}' inside function body — move to module level"),
            ));
        }
    }

    for import_node in find_nodes_by_type(root, "import_statement") {
        if is_inside_function(import_node) {
            let text = node_text(import_node, source.source_bytes);
            violations.push(violation(
                node_line(import_node),
                format!("deferred {text} inside function body — move to module level"),
            ));
        }
    }

    violations
}

/// Walk ancestors to check if a node is inside a function_definition.
fn is_inside_function(node: tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "function_definition" {
            return true;
        }
        current = parent.parent();
    }
    false
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

    fn parse_with_path<'a>(code: &str, path: &'a str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        let path: &'static str = Box::leak(path.to_string().into_boxed_str());
        build_parsed_source(path, source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
    }

    // -- no_type_checking_imports --

    #[test]
    fn type_checking_block_caught() {
        let parsed = parse("from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    import foo\n");
        let violations = check_no_type_checking_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("TYPE_CHECKING"));
    }

    #[test]
    fn normal_if_ok() {
        let parsed = parse("if True:\n    pass\n");
        let violations = check_no_type_checking_imports(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_parent_imports --

    #[test]
    fn parent_functions_import_caught() {
        let code = "from functions import helper\n";
        let parsed = parse(code);
        let violations = check_no_parent_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("functions"));
    }

    #[test]
    fn qualified_import_ok() {
        let code = "from functions.pure import helper\n";
        let parsed = parse(code);
        let violations = check_no_parent_imports(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- impure_module_quarantine --

    #[test]
    fn impure_stdlib_in_pure_zone() {
        let parsed = parse_with_path(
            "import os\n",
            "/project/src/functions/pure/module.py",
        );
        let violations = check_impure_module_quarantine(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("impure stdlib"));
    }

    #[test]
    fn pure_import_ok() {
        let parsed = parse_with_path(
            "from typing import Literal\n",
            "/project/src/functions/pure/module.py",
        );
        let violations = check_impure_module_quarantine(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn cross_zone_import_caught() {
        let parsed = parse_with_path(
            "from myproject.functions.impure.loader import read_file\n",
            "/project/src/functions/pure/module.py",
        );
        let violations = check_impure_module_quarantine(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("cross-zone"));
    }

    // -- no_relative_imports --

    #[test]
    fn relative_import_caught() {
        let code = "from .sibling import helper\n";
        let parsed = parse(code);
        let violations = check_no_relative_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("relative import"));
    }

    #[test]
    fn absolute_import_ok_for_relative_check() {
        let code = "from pydantic import BaseModel\n";
        let parsed = parse(code);
        let violations = check_no_relative_imports(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn parent_relative_import_caught() {
        let code = "from ..parent import thing\n";
        let parsed = parse(code);
        let violations = check_no_relative_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- v2 import boundaries --

    #[test]
    fn v2_pure_simple_imports_pure_primitive_ok() {
        let code = "from mypackage.logic.pure.primitive.helpers import add\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/pure/check/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_pure_simple_imports_structure_ok() {
        let code = "from mypackage.structures.models import MyModel\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/pure/check/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_same_level_import_blocked() {
        let code = "from mypackage.logic.pure.simple.other import foo\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/pure/check/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("level violation"));
    }

    #[test]
    fn v2_transform_to_pure_blocked() {
        let code = "from mypackage.logic.pure.primitive.helpers import add\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/transform/coerce/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("zone violation"));
    }

    #[test]
    fn v2_impure_simple_to_pure_primitive_ok() {
        let code = "from mypackage.logic.pure.primitive.helpers import add\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/io_ops/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_impure_to_pure_same_level_blocked() {
        let code = "from mypackage.logic.pure.simple.helpers import add\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/io_ops/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("level violation"));
    }

    #[test]
    fn v2_orchestrate_to_composed_ok() {
        let code = "from mypackage.logic.pure.composed.pipeline import run\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/orchestrate/pipeline/orchestrate.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_orchestrate_to_simple_ok() {
        let code = "from mypackage.logic.pure.simple.helpers import add\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/orchestrate/pipeline/orchestrate.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty(), "orchestrate should reach any lower level");
    }

    #[test]
    fn v2_structure_imports_structure_ok() {
        let code = "from regin.structure.gen.schema.agent_paths_resolved import Capabilities\nfrom regin.structure.model.capability_tiers import CapabilityTiers\n";
        let parsed = parse_with_path(code, "/project/src/regin/structure/config/capability_tiers.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty(), "structure-to-structure imports should be allowed, got: {:?}", violations.iter().map(|v| &v.message).collect::<Vec<_>>());
    }

    #[test]
    fn v2_external_import_skipped() {
        let code = "from pydantic import BaseModel\nimport json\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/pure/check/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_outside_file_is_violation() {
        let code = "import json\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/random_file.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("outside v2 zone layout"));
    }

    #[test]
    fn v2_pure_composed_unreachable_from_impure() {
        let code = "from mypackage.logic.pure.composed.pipeline import run\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/workflow/composed.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("level violation"));
    }

    // -- no_unsafe_imports --

    #[test]
    fn unsafe_import_caught() {
        let code = "from unsafe import dangerous\n";
        let parsed = parse(code);
        let violations = check_no_unsafe_imports(&parsed, &default_config());
        assert!(!violations.is_empty());
    }

    #[test]
    fn unsafe_import_exempted_in_unsafe_zone() {
        let code = "from unsafe import dangerous\n";
        let parsed = parse_with_path(code, "/project/src/unsafe/impure/module.py");
        let violations = check_no_unsafe_imports(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_disallowed_stdlib --

    #[test]
    fn argparse_caught() {
        let parsed = parse("import argparse\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("typer"));
    }

    #[test]
    fn logging_caught() {
        let parsed = parse("import logging\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("loguru"));
    }

    #[test]
    fn json_from_import_caught() {
        let parsed = parse("from json import loads\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("pydantic"));
    }

    #[test]
    fn unittest_caught() {
        let parsed = parse("import unittest\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("pytest"));
    }

    #[test]
    fn json_import_still_disallowed_stdlib_only() {
        let parsed = parse("import json\n");
        assert_eq!(check_no_disallowed_stdlib(&parsed, &default_config()).len(), 1);
        assert!(check_no_direct_file_io(&parsed, &default_config()).is_empty());
    }

    #[test]
    fn pydantic_import_not_flagged() {
        let parsed = parse("from pydantic import BaseModel\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn typer_import_not_flagged() {
        let parsed = parse("import typer\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_direct_file_io --

    fn direct_io(code: &str) -> Vec<Violation> {
        check_no_direct_file_io(&parse(code), &default_config())
    }

    // fire: parser imports

    #[test]
    fn direct_io_tomllib_import_caught() {
        let v = direct_io("import tomllib\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("'tomllib' bypasses the reader gates"), "{}", v[0].message);
        assert!(v[0].message.contains("read_toml_validate_json"));
    }

    #[test]
    fn direct_io_yaml_from_import_caught() {
        let v = direct_io("from yaml import safe_load\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("read_yaml_validate_json"));
    }

    #[test]
    fn direct_io_xml_submodule_caught() {
        let v = direct_io("import xml.etree.ElementTree as ElementTree\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("read_xml_validate_json"));
    }

    #[test]
    fn direct_io_tomli_w_names_writer() {
        let v = direct_io("import tomli_w\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("writer tool"));
    }

    #[test]
    fn direct_io_configparser_caught() {
        let v = direct_io("import configparser\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("read_toml_validate_json"));
        assert!(check_no_disallowed_stdlib(&parse("import configparser\n"), &default_config()).is_empty());
    }

    #[test]
    fn direct_io_csv_states_no_reader_yet() {
        let v = direct_io("import csv\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("no reader gate exists"));
    }

    // fire: file handles

    #[test]
    fn direct_io_path_open_caught() {
        let v = direct_io("from pathlib import Path\ndef load(path: Path):\n    with path.open(\"rb\") as fh:\n        return fh.read()\n");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 3);
        assert!(v[0].message.contains("direct file read (path.open)"), "{}", v[0].message);
    }

    #[test]
    fn direct_io_builtin_open_read_caught() {
        let v = direct_io("def load(p: str):\n    with open(p) as fh:\n        return fh.read()\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("direct file read (open)"));
        assert!(v[0].message.contains("read_*_validate_json"));
    }

    #[test]
    fn direct_io_builtin_open_write_mode_caught() {
        let v = direct_io("def save(p: str, s: str):\n    with open(p, \"w\") as fh:\n        fh.write(s)\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("direct file write (open, mode 'w')"), "{}", v[0].message);
        assert!(v[0].message.contains("writer tool"));
    }

    #[test]
    fn direct_io_open_mode_keyword_caught() {
        let v = direct_io("fh = open(p, mode=\"ab\")\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("direct file write"));
    }

    #[test]
    fn direct_io_read_text_caught() {
        let v = direct_io("content = output_path.read_text()\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("direct file read (.read_text)"));
        assert!(v[0].message.contains("read_*_validate_json"));
    }

    #[test]
    fn direct_io_write_text_caught() {
        let v = direct_io("output_path.write_text(report, encoding=\"utf-8\")\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("direct file write (.write_text)"));
        assert!(v[0].message.contains("writer tool"));
    }

    #[test]
    fn direct_io_write_bytes_caught() {
        let v = direct_io("path.write_bytes(json_bytes)\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("direct file write (.write_bytes)"));
    }

    #[test]
    fn direct_io_unbound_path_read_text_caught() {
        let v = direct_io("from pathlib import Path\ntext = Path.read_text(p)\n");
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn direct_io_registry_specimen_two_firings() {
        // The specimen that motivated the check: import + handle fire, the parser call does not.
        let code = "import tomllib\nfrom pathlib import Path\n\ndef load_registry(path: Path):\n    with path.open(\"rb\") as file_handle:\n        data = tomllib.load(file_handle)\n    return data\n";
        let v = direct_io(code);
        let lines: Vec<usize> = v.iter().map(|x| x.line).collect();
        assert_eq!(lines, vec![1, 5], "{:?}", v.iter().map(|x| &x.message).collect::<Vec<_>>());
    }

    #[test]
    fn direct_io_tomllib_loads_over_read_text_two_firings() {
        let v = direct_io("import tomllib\nconfig = tomllib.loads(CONFIG_PATH.read_text())\n");
        let lines: Vec<usize> = v.iter().map(|x| x.line).collect();
        assert_eq!(lines, vec![1, 2]);
    }

    // no-fire

    #[test]
    fn direct_io_json_loads_string_not_flagged() {
        let v = direct_io("import json\ndef extract(user_id: str):\n    return json.loads(user_id).get(\"session_id\")\n");
        assert!(v.is_empty(), "{:?}", v.iter().map(|x| &x.message).collect::<Vec<_>>());
    }

    #[test]
    fn direct_io_webbrowser_open_not_flagged() {
        assert!(direct_io("import webbrowser\nwebbrowser.open(page.as_uri())\n").is_empty());
    }

    #[test]
    fn direct_io_pdfplumber_open_not_flagged() {
        assert!(direct_io("import pdfplumber\npdf = pdfplumber.open(PDF)\n").is_empty());
    }

    #[test]
    fn direct_io_attribute_named_open_not_flagged() {
        assert!(direct_io("class Shop(BaseModel):\n    open: bool\nif shop.open:\n    pass\n").is_empty());
    }

    #[test]
    fn direct_io_relative_import_not_flagged() {
        assert!(direct_io("from .yaml_helpers import thing\n").is_empty());
    }

    #[test]
    fn direct_io_similar_module_name_not_flagged() {
        assert!(direct_io("from mypkg.toml_support import thing\nimport yamlish\n").is_empty());
    }

    #[test]
    fn direct_io_pydantic_and_pathlib_imports_not_flagged() {
        let v = direct_io("from pydantic import BaseModel\nfrom pathlib import Path\np = Path.home() / \"x\"\nexists = p.exists()\n");
        assert!(v.is_empty(), "{:?}", v.iter().map(|x| &x.message).collect::<Vec<_>>());
    }

    #[test]
    fn direct_io_gate_and_reader_module_imports_not_flagged() {
        let v = direct_io("import gate_output_content_input\nimport read_toml_validate_json\nfrom galdr.logic.impure.gate_validate.simple import validate_input\n");
        assert!(v.is_empty());
    }

    #[test]
    fn direct_io_dynamic_import_non_literal_not_flagged() {
        assert!(direct_io("import importlib\nmod = importlib.import_module(name)\n").is_empty());
    }

    #[test]
    fn direct_io_commented_and_string_mentions_not_flagged() {
        assert!(direct_io("# import tomllib\nhint = \"use open(path)\"\n").is_empty());
    }

    // canary: must still fire

    #[test]
    fn direct_io_aliased_import_caught() {
        let v = direct_io("import tomllib as cfg\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("'tomllib'"));
    }

    #[test]
    fn direct_io_aliased_from_import_caught() {
        assert_eq!(direct_io("from yaml import safe_load as load_cfg\n").len(), 1);
    }

    #[test]
    fn direct_io_dunder_import_caught() {
        let v = direct_io("cfg = __import__(\"tomllib\")\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("via dynamic import"));
        assert!(v[0].message.contains("read_toml_validate_json"));
    }

    #[test]
    fn direct_io_importlib_import_module_caught() {
        let v = direct_io("import importlib\ny = importlib.import_module(\"yaml\")\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("read_yaml_validate_json"));
    }

    #[test]
    fn direct_io_importlib_dotted_name_caught() {
        let v = direct_io("import importlib\nm = importlib.import_module(\"xml.etree.ElementTree\")\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("read_xml_validate_json"));
    }

    #[test]
    fn direct_io_builtins_open_caught() {
        let v = direct_io("import builtins\nfh = builtins.open(p)\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("builtins.open"));
    }

    #[test]
    fn direct_io_io_open_write_caught() {
        let v = direct_io("import io\nfh = io.open(p, \"w\")\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("direct file write"));
    }

    // -- no_before_validators --

    #[test]
    fn before_validator_caught() {
        let parsed = parse("from pydantic import BeforeValidator\n");
        let violations = check_no_before_validators(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("BeforeValidator"));
    }

    #[test]
    fn pydantic_basemodel_ok() {
        let parsed = parse("from pydantic import BaseModel\n");
        let violations = check_no_before_validators(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_sys_path_mutation --

    #[test]
    fn sys_path_insert_caught() {
        let parsed = parse("import sys\nsys.path.insert(0, \"/some/path\")\n");
        let violations = check_no_sys_path_mutation(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("sys.path.insert"));
    }

    #[test]
    fn sys_path_append_caught() {
        let parsed = parse("import sys\nsys.path.append(\"/some/path\")\n");
        let violations = check_no_sys_path_mutation(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("sys.path.append"));
    }

    #[test]
    fn sys_path_assignment_caught() {
        let parsed = parse("import sys\nsys.path = [\"/new/path\"]\n");
        let violations = check_no_sys_path_mutation(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("sys.path assignment"));
    }

    #[test]
    fn sys_stdout_ok() {
        let parsed = parse("import sys\nsys.stdout.write(\"hello\")\n");
        let violations = check_no_sys_path_mutation(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn sys_exit_ok() {
        let parsed = parse("import sys\nsys.exit(0)\n");
        let violations = check_no_sys_path_mutation(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_deferred_imports --

    #[test]
    fn deferred_import_in_function_caught() {
        let code = "def assemble():\n    from galdr.structure.model import SectionBuffer\n    return SectionBuffer()\n";
        let parsed = parse(code);
        let violations = check_no_deferred_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("deferred import"));
    }

    #[test]
    fn deferred_bare_import_caught() {
        let code = "def run():\n    import json\n    return json.dumps({})\n";
        let parsed = parse(code);
        let violations = check_no_deferred_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("deferred"));
    }

    #[test]
    fn top_level_import_ok() {
        let code = "from pathlib import Path\ndef run() -> Path:\n    return Path('.')\n";
        let parsed = parse(code);
        let violations = check_no_deferred_imports(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn multiple_deferred_imports_all_caught() {
        let code = "def run():\n    from os import path\n    import json\n    return path.join(json.dumps({}))\n";
        let parsed = parse(code);
        let violations = check_no_deferred_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 2);
    }
}
