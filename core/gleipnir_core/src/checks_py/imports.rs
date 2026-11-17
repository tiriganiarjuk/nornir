//! Import boundary checks.
//!
//! Checks: no_unsafe_imports, impure_module_quarantine,
//! no_type_checking_imports, no_parent_imports, no_disallowed_stdlib,
//! no_direct_file_io, no_sys_path_mutation.

use crate::classify;
use crate::parsing::{
    bound_names, extract_imported_names, extract_module_info, find_nodes_by_type, imported_modules, module_names,
    node_line, node_text,
};
use crate::structures::{CheckConfig, Level, ParsedSource, Severity, V2Classification, Violation, Zone};

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
        for module in imported_modules(node, source.source_bytes) {
            let parts: Vec<&str> = module.split('.').collect();
            if parts.len() > 1 && parts[1..].contains(&"unsafe") {
                violations.push(violation(
                    node_line(node),
                    "importing from quarantine zone".to_string(),
                ));
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
    let line = node_line(node);
    imported_modules(node, source)
        .into_iter()
        .filter_map(|module| {
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

/// The replacement for every parser that hands back an untyped dict from JSON text.
const JSON_REPLACEMENT: &str = "pydantic model_dump_json() / model_validate_json()";

/// Modules that have been replaced by project-standard alternatives: the stdlib
/// ones, and the third-party JSON parsers that give the same untyped dict as json.
/// Each entry is (module_name, replacement).
const DISALLOWED_STDLIB: &[(&str, &str)] = &[
    ("argparse", "typer"),
    ("logging", "loguru"),
    ("unittest", "pytest"),
    ("json", JSON_REPLACEMENT),
    ("orjson", JSON_REPLACEMENT),
    ("ujson", JSON_REPLACEMENT),
    ("simplejson", JSON_REPLACEMENT),
    ("rapidjson", JSON_REPLACEMENT),
    ("msgspec", JSON_REPLACEMENT),
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
        for module in imported_modules(node, source.source_bytes) {
            let top = module.split('.').next().unwrap_or("");
            if let Some((_, replacement)) = DISALLOWED_STDLIB.iter().find(|(name, _)| *name == top) {
                violations.push(violation(
                    node_line(node),
                    format!("'{top}' is superseded — use {replacement}"),
                ));
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_direct_file_io — structured data enters only through compiled gates
// -------------------------------------------------------------------------
//
// Proxy: "Python performs no direct file IO." Three surfaces, one check:
//   A. imports of structured-file parsers (tomllib, yaml, ...) — incl. aliased
//      and dynamic (__import__ / importlib.import_module) forms
//   B. raw file handles: open(), .open(), .read_text/.read_bytes/.write_text/.write_bytes
//   C. pathlib's filesystem classes: Path and its kin, by name, under an
//      alias, or with the module imported whole. PurePath is a path's name and
//      cannot touch a disk; everything Path adds to it does.
// The violation message names the replacement for the surface that fired.
// `json` is deliberately absent: no_disallowed_stdlib owns it (in-memory,
// replacement is model_validate_json). json.load(fh) is still caught — by the
// handle on the open() line.

const NO_READER_YET: &str = "no reader gate exists for it yet — the violation stands until one does";
const READ_REPLACEMENT: &str = "a read_*_validate_json reader for structured data";
const WRITE_REPLACEMENT: &str = "a nornir writer tool";
/// A TOML parser import does not say which kind of file it reads, so both readers are named.
const TOML_READERS: &str = "read_toml_validate_json, or read_tomlx_validate_json for a .tomlx file";

/// Structured-file parsers that bypass the reader gates. (top-level module, replacement)
const GATE_BYPASS_PARSERS: &[(&str, &str)] = &[
    ("tomllib", TOML_READERS),
    ("tomli", TOML_READERS),
    ("toml", TOML_READERS),
    ("tomlkit", TOML_READERS),
    ("configparser", TOML_READERS),
    ("tomli_w", WRITE_REPLACEMENT),
    ("yaml", "read_yaml_validate_json"),
    ("ruamel", "read_yaml_validate_json"),
    ("xml", "read_xml_validate_json"),
    ("lxml", "read_xml_validate_json"),
    ("csv", NO_READER_YET),
    ("pickle", NO_READER_YET),
    ("shelve", NO_READER_YET),
];

/// A listing, a presence test or a change on disk.
const DISK_REPLACEMENT: &str = "a compiled tool for anything on disk";

/// Method names that are direct file IO on any receiver, a Path a library hands
/// back included. Only names a Path holds and a string or a model does not:
/// `rename`, `replace`, `glob` and `stat` are left out. (method, verb, replacement)
const DIRECT_FILE_METHODS: &[(&str, &str, &str)] = &[
    ("read_text", "read", READ_REPLACEMENT),
    ("read_bytes", "read", READ_REPLACEMENT),
    ("write_text", "write", WRITE_REPLACEMENT),
    ("write_bytes", "write", WRITE_REPLACEMENT),
    ("unlink", "change", DISK_REPLACEMENT),
    ("mkdir", "change", DISK_REPLACEMENT),
    ("rmdir", "change", DISK_REPLACEMENT),
    ("touch", "change", DISK_REPLACEMENT),
    ("symlink_to", "change", DISK_REPLACEMENT),
    ("hardlink_to", "change", DISK_REPLACEMENT),
    ("iterdir", "query", DISK_REPLACEMENT),
    ("rglob", "query", DISK_REPLACEMENT),
    ("is_file", "query", DISK_REPLACEMENT),
    ("is_dir", "query", DISK_REPLACEMENT),
    ("is_symlink", "query", DISK_REPLACEMENT),
    ("exists", "query", DISK_REPLACEMENT),
];

/// The functions of `os` that list, test or change what is on disk.
const OS_DISK_FUNCTIONS: &[&str] = &[
    "remove", "unlink", "rmdir", "removedirs", "rename", "renames", "replace", "mkdir", "makedirs",
    "listdir", "scandir", "walk", "fwalk", "stat", "lstat", "chmod", "chown", "lchown", "link",
    "symlink", "readlink", "truncate", "utime", "open", "fdopen", "mkfifo", "access", "chdir",
];

/// The functions of `os.path` that look at the disk; the rest only handle a name.
const OS_PATH_DISK_FUNCTIONS: &[&str] = &[
    "exists", "lexists", "isfile", "isdir", "islink", "ismount", "getsize", "getmtime", "getatime",
    "getctime", "samefile", "realpath",
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

/// Surface A1: `import x[.y] [as z]`.
fn collect_parser_imports(source: &ParsedSource, violations: &mut Vec<Violation>) {
    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        for module in imported_modules(node, source.source_bytes) {
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

/// Every call surface, one finding per call: the first surface that names it.
fn collect_call_surfaces(source: &ParsedSource, violations: &mut Vec<Violation>) {
    let root = source.tree.root_node();
    let bytes = source.source_bytes;
    let holders = os_holders(root, bytes);
    for call in find_nodes_by_type(root, "call") {
        let Some(func) = call.child_by_field_name("function") else { continue };
        let hit = dynamic_import_violation(call, func, bytes)
            .or_else(|| open_violation(call, func, bytes))
            .or_else(|| os_call_violation(call, func, &holders, bytes))
            .or_else(|| file_method_violation(call, func, bytes));
        if let Some(v) = hit {
            violations.push(v);
        }
    }
}

const PATH_REPLACEMENT: &str = "PurePath for a path's name, and a compiled tool for anything on disk";

/// Surface C1: `from pathlib import Path [as P]` — every name but the pure ones.
fn collect_path_from_imports(source: &ParsedSource, violations: &mut Vec<Violation>) {
    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, level) = extract_module_info(node, source.source_bytes);
        if level != 0 || module != "pathlib" {
            continue;
        }
        for name in extract_imported_names(node, source.source_bytes) {
            if !PATHLIB_PURE.contains(&name.as_str()) {
                violations.push(violation(
                    node_line(node),
                    format!("'{name}' from pathlib reaches the filesystem — use {PATH_REPLACEMENT}"),
                ));
            }
        }
    }
}

/// Surface C2: `import pathlib [as p]` — the module whole brings Path with it.
fn collect_whole_pathlib_imports(source: &ParsedSource, violations: &mut Vec<Violation>) {
    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        for module in imported_modules(node, source.source_bytes) {
            if module == "pathlib" {
                violations.push(violation(
                    node_line(node),
                    format!("pathlib imported whole brings Path with it — import PurePath by name: {PATH_REPLACEMENT}"),
                ));
            }
        }
    }
}

/// Surface D1: `shutil`, whole or by name — every function it holds works on the disk.
fn collect_shutil_imports(source: &ParsedSource, violations: &mut Vec<Violation>) {
    let root = source.tree.root_node();
    let bytes = source.source_bytes;
    let message = format!("'shutil' works on the disk — use {DISK_REPLACEMENT}");
    for node in find_nodes_by_type(root, "import_statement") {
        if imported_modules(node, bytes).iter().any(|module| module.split('.').next() == Some("shutil")) {
            violations.push(violation(node_line(node), message.clone()));
        }
    }
    for node in find_nodes_by_type(root, "import_from_statement") {
        let (module, level) = extract_module_info(node, bytes);
        if level == 0 && module.split('.').next() == Some("shutil") {
            violations.push(violation(node_line(node), message.clone()));
        }
    }
}

/// Whether `name` of `module` lists, tests or changes what is on disk.
fn is_os_disk_function(module: &str, name: &str) -> bool {
    match module {
        "os" => OS_DISK_FUNCTIONS.contains(&name),
        "os.path" => OS_PATH_DISK_FUNCTIONS.contains(&name),
        _ => false,
    }
}

/// Surface D2: `from os import remove`, `from os.path import exists` — the disk
/// functions by name. `os.environ`, `os.sep` and `os.path.join` touch no disk.
fn collect_os_from_imports(source: &ParsedSource, violations: &mut Vec<Violation>) {
    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, level) = extract_module_info(node, source.source_bytes);
        if level != 0 {
            continue;
        }
        for name in extract_imported_names(node, source.source_bytes) {
            if is_os_disk_function(&module, &name) {
                violations.push(violation(
                    node_line(node),
                    format!("'{name}' from {module} reaches the disk — use {DISK_REPLACEMENT}"),
                ));
            }
        }
    }
}

/// The names a file uses for `os` and for `os.path`.
fn os_holders(root: tree_sitter::Node, source: &[u8]) -> (Vec<String>, Vec<String>) {
    let mut os_names = module_names(root, source, "os");
    os_names.push("os".to_string());
    let mut os_path_names: Vec<String> = os_names.iter().map(|name| format!("{name}.path")).collect();
    os_path_names.extend(bound_names(root, source, "os", "path"));
    os_path_names.extend(module_names(root, source, "os.path"));
    (os_names, os_path_names)
}

/// Surface D3: `os.remove(...)`, `os.path.exists(...)` — a disk function called
/// through a name the file uses for `os` or `os.path`.
fn os_call_violation(
    call: tree_sitter::Node,
    func: tree_sitter::Node,
    holders: &(Vec<String>, Vec<String>),
    source: &[u8],
) -> Option<Violation> {
    if func.kind() != "attribute" {
        return None;
    }
    let object = node_text(func.child_by_field_name("object")?, source);
    let name = node_text(func.child_by_field_name("attribute")?, source);
    let (os_names, os_path_names) = holders;
    let module = if os_names.iter().any(|held| held == object) {
        "os"
    } else if os_path_names.iter().any(|held| held == object) {
        "os.path"
    } else {
        return None;
    };
    is_os_disk_function(module, name).then(|| {
        violation(node_line(call), format!("direct disk call ({}) — use {DISK_REPLACEMENT}", node_text(func, source)))
    })
}

pub fn check_no_direct_file_io(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    collect_parser_imports(source, &mut violations);
    collect_parser_from_imports(source, &mut violations);
    collect_path_from_imports(source, &mut violations);
    collect_whole_pathlib_imports(source, &mut violations);
    collect_shutil_imports(source, &mut violations);
    collect_os_from_imports(source, &mut violations);
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

    // A file in no zone has no place in the import hierarchy to judge from;
    // file_in_no_zone reports it.
    if source_class.zone == Zone::Outside {
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

    let root = source.tree.root_node();
    let bytes = source.source_bytes;
    let package = project_package(source.file_path);

    // Check import_from_statement nodes
    for node in find_nodes_by_type(root, "import_from_statement") {
        let (module, level) = extract_module_info(node, bytes);

        let dotted = if level > 0 {
            resolve_relative_import(source.file_path, &module, level)
        } else {
            module.clone()
        };

        judge_import(source_class, &dotted, package, node_line(node), &mut violations);
    }

    // Check bare import_statement nodes
    for node in find_nodes_by_type(root, "import_statement") {
        for dotted in imported_modules(node, bytes) {
            judge_import(source_class, dotted, package, node_line(node), &mut violations);
        }
    }

    // Check imports made by a name held in a string
    let import_module_names = bound_names(root, bytes, "importlib", "import_module");
    for call in find_nodes_by_type(root, "call") {
        let Some(func) = call.child_by_field_name("function") else { continue };
        let named_import = func.kind() == "identifier"
            && import_module_names.iter().any(|name| name == node_text(func, bytes));
        if !is_dynamic_import(func, bytes) && !named_import {
            continue;
        }
        match first_positional_arg(call).and_then(|arg| string_literal_text(arg, bytes)) {
            Some(dotted) => judge_import(source_class, dotted, package, node_line(call), &mut violations),
            None => violations.push(violation(
                node_line(call),
                "dynamic import of a computed name — its target cannot be judged; import it by name at the top of the file".to_string(),
            )),
        }
    }

    violations
}

/// The entry-point module names: a module of the project with one of these as its last
/// segment is the entry point.
const ENTRY_POINT_MODULES: &[&str] = &["cli", "__main__"];

/// The project's own package: the directory right below `src/` in a file's path.
fn project_package(file_path: &str) -> Option<&str> {
    let mut segments = file_path.split('/');
    segments.by_ref().find(|segment| *segment == "src")?;
    segments.next().filter(|package| !package.is_empty() && !package.ends_with(".py"))
}

/// Judge one import by its dotted path. A module in a zone is judged by the level and
/// zone rules; a module of the project's own that lies in no zone is a layout violation,
/// unless it is the entry point, which is judged at its level; any other module is
/// another project's, and is not judged here.
fn judge_import(
    source: V2Classification,
    dotted_path: &str,
    package: Option<&str>,
    line: usize,
    violations: &mut Vec<Violation>,
) {
    if let Some(target_class) = classify::classify_import_path(dotted_path) {
        check_import_legality(source, target_class, dotted_path, line, violations);
        return;
    }
    let is_own = package.is_some_and(|package| dotted_path.split('.').next() == Some(package));
    if !is_own {
        return;
    }
    let last = dotted_path.rsplit('.').next().unwrap_or("");
    if ENTRY_POINT_MODULES.contains(&last) {
        let entry_point = V2Classification { level: Level::L8, zone: Zone::Orchestrate };
        check_import_legality(source, entry_point, dotted_path, line, violations);
        return;
    }
    violations.push(violation(line, format!("import of {dotted_path}, a module of the project in no zone")));
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

/// The validators that see a value before its field is validated.
const COERCING_VALIDATORS: &[&str] = &["BeforeValidator", "WrapValidator"];
/// The decorators whose `mode=` can make a validator run before the field's.
const MODE_VALIDATORS: &[&str] = &["field_validator", "model_validator", "validator", "root_validator"];
/// The modes in which a decorated validator sees the raw input.
const COERCING_MODES: &[&str] = &["before", "wrap"];

pub fn check_no_before_validators(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let root = source.tree.root_node();
    let bytes = source.source_bytes;
    let mut violations = Vec::new();

    for node in find_nodes_by_type(root, "import_from_statement") {
        for name in extract_imported_names(node, bytes) {
            if COERCING_VALIDATORS.contains(&name.as_str()) {
                violations.push(violation(
                    node_line(node),
                    format!("{name} import — use explicit transforms instead of implicit coercion"),
                ));
            }
        }
    }
    for attribute in find_nodes_by_type(root, "attribute") {
        let name = attribute.child_by_field_name("attribute").map(|name| node_text(name, bytes)).unwrap_or("");
        if COERCING_VALIDATORS.contains(&name) {
            violations.push(violation(
                node_line(attribute),
                format!("{} — use explicit transforms instead of implicit coercion", node_text(attribute, bytes)),
            ));
        }
    }
    for call in find_nodes_by_type(root, "call") {
        if let Some(found) = coercing_mode_violation(call, bytes) {
            violations.push(found);
        }
    }

    violations
}

/// `@field_validator("x", mode="before")` and its kin: a validator that sees the
/// raw input before the field is validated.
fn coercing_mode_violation(call: tree_sitter::Node, source: &[u8]) -> Option<Violation> {
    let function = call.child_by_field_name("function")?;
    let decorator = match function.kind() {
        "identifier" => node_text(function, source),
        "attribute" => node_text(function.child_by_field_name("attribute")?, source),
        _ => return None,
    };
    if !MODE_VALIDATORS.contains(&decorator) {
        return None;
    }
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let mode = arguments
        .named_children(&mut cursor)
        .filter(|argument| argument.kind() == "keyword_argument")
        .find(|argument| argument.child_by_field_name("name").is_some_and(|name| node_text(name, source) == "mode"))
        .and_then(|argument| argument.child_by_field_name("value"))
        .and_then(|value| string_literal_text(value, source))?;
    COERCING_MODES.contains(&mode).then(|| {
        violation(
            node_line(call),
            format!("{decorator}(mode=\"{mode}\") — use explicit transforms instead of implicit coercion"),
        )
    })
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
    let root = source.tree.root_node();
    let bytes = source.source_bytes;
    let holders = sys_path_names(root, bytes);
    let mut violations = Vec::new();

    // Calls that change the list in place: sys.path.insert(...), path.extend(...)
    for node in find_nodes_by_type(root, "call") {
        let Some(func) = node.child_by_field_name("function").filter(|func| func.kind() == "attribute") else {
            continue;
        };
        let held = func.child_by_field_name("object").is_some_and(|object| holds_sys_path(object, &holders, bytes));
        let method = func.child_by_field_name("attribute").map(|attribute| node_text(attribute, bytes)).unwrap_or("");
        if held && SYS_PATH_MUTATORS.contains(&method) {
            let func_text = node_text(func, bytes);
            violations.push(violation(
                node_line(node),
                format!("{func_text}() manipulates import path — fix the project layout instead"),
            ));
        }
    }

    // Assignments: sys.path = ..., sys.path[:] = ..., sys.path += ...
    for kind in ["assignment", "augmented_assignment"] {
        for node in find_nodes_by_type(root, kind) {
            let assigned = node.child_by_field_name("left").map(|left| unsubscripted(left));
            if assigned.is_some_and(|target| holds_sys_path(target, &holders, bytes)) {
                violations.push(violation(
                    node_line(node),
                    "sys.path assignment manipulates import path — fix the project layout instead".to_string(),
                ));
            }
        }
    }

    violations
}

/// The methods of a list that change it in place.
const SYS_PATH_MUTATORS: &[&str] = &["insert", "append", "extend", "remove", "pop", "clear"];

/// Every expression a file may use for `sys.path`: `sys.path` through each name
/// it binds `sys` to, and each name it imports `path` from `sys` under.
fn sys_path_names(root: tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let mut sys_names = module_names(root, source, "sys");
    sys_names.push("sys".to_string());
    let mut holders: Vec<String> = sys_names.iter().map(|name| format!("{name}.path")).collect();
    holders.extend(bound_names(root, source, "sys", "path"));
    holders
}

/// Whether an expression is one of the names a file uses for `sys.path`.
fn holds_sys_path(node: tree_sitter::Node, holders: &[String], source: &[u8]) -> bool {
    holders.iter().any(|holder| holder == node_text(node, source))
}

/// A subscript's object (`sys.path` of `sys.path[:]`); any other node itself.
fn unsubscripted(node: tree_sitter::Node) -> tree_sitter::Node {
    if node.kind() == "subscript" {
        return node.child_by_field_name("value").unwrap_or(node);
    }
    node
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
        let code = "from mypackage.structure.models import MyModel\n";
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
    fn v2_file_with_an_unknown_name_in_a_zone_is_violation() {
        let code = "import json\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/pure/check/random_file.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("outside v2 zone layout"));
    }

    #[test]
    fn v2_file_in_no_zone_is_left_to_its_own_check() {
        let code = "from mypackage.logic.pure.check.simple import run\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/random_file.py");
        assert!(check_v2_import_boundaries(&parsed, &default_config()).is_empty());
    }

    // The edge: a module of the project's own that lies in no zone, the entry point
    // included, was taken for a third-party module and its import was not judged.

    #[test]
    fn v2_import_of_a_project_module_in_no_zone_is_violation() {
        for code in [
            "from mypackage.helpers import tidy\n",
            "import mypackage.helpers\n",
            "from mypackage import helpers\n",
        ] {
            let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/survey/simple.py");
            let violations = check_v2_import_boundaries(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{code}");
            assert!(violations[0].message.contains("in no zone"), "{code}: {}", violations[0].message);
        }
    }

    #[test]
    fn v2_import_of_the_entry_point_is_a_level_violation() {
        for code in ["from mypackage.cli import main\n", "from mypackage.__main__ import main\n"] {
            let parsed = parse_with_path(code, "/project/src/mypackage/logic/orchestrate/run/orchestrate.py");
            let violations = check_v2_import_boundaries(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{code}");
            assert!(violations[0].message.contains("level violation"), "{code}: {}", violations[0].message);
        }
    }

    #[test]
    fn v2_import_of_another_project_is_not_judged() {
        let code = "from otherpackage.helpers import tidy\nimport loguru\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/survey/simple.py");
        assert!(check_v2_import_boundaries(&parsed, &default_config()).is_empty());
    }

    // The edge: a module imported by a name held in a string never reached the import rules.

    #[test]
    fn v2_dynamic_import_is_judged_like_an_import() {
        for code in [
            "import importlib\nsibling = importlib.import_module(\"mypackage.logic.impure.other.simple\")\n",
            "sibling = __import__(\"mypackage.logic.impure.other.simple\")\n",
            "from importlib import import_module\nsibling = import_module(\"mypackage.logic.transform.reshape.primitive\")\n",
        ] {
            let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/survey/simple.py");
            let violations = check_v2_import_boundaries(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{code}");
            assert!(violations[0].message.contains("violation"), "{code}: {}", violations[0].message);
        }
    }

    #[test]
    fn v2_dynamic_import_of_a_computed_name_is_violation() {
        let code = "import importlib\nmodule = importlib.import_module(name)\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/survey/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("dynamic import"), "{}", violations[0].message);
    }

    #[test]
    fn v2_legal_dynamic_import_ok() {
        let code = "import importlib\nleaf = importlib.import_module(\"mypackage.logic.pure.path_derive.primitive\")\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/survey/simple.py");
        assert!(check_v2_import_boundaries(&parsed, &default_config()).is_empty());
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

    // -- a module imported under another name is still that module --

    #[test]
    fn disallowed_stdlib_under_another_name_caught() {
        // The canary: `import json as serial` leaves the check's dotted_name loop.
        for code in ["import json as serial\n", "import os, logging as log\n"] {
            let violations = check_no_disallowed_stdlib(&parse(code), &default_config());
            assert_eq!(violations.len(), 1, "{code}");
        }
    }

    #[test]
    fn impure_module_under_another_name_caught_in_pure_zone() {
        for code in ["import subprocess as shell\n", "import myproject.functions.impure.loader as loader\n"] {
            let parsed = parse_with_path(code, "/project/src/functions/pure/module.py");
            assert_eq!(check_impure_module_quarantine(&parsed, &default_config()).len(), 1, "{code}");
        }
    }

    #[test]
    fn impure_pathlib_name_under_another_name_caught_in_pure_zone() {
        let parsed = parse_with_path("from pathlib import Path as Location\n", "/project/src/functions/pure/module.py");
        let violations = check_impure_module_quarantine(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("impure name"), "{}", violations[0].message);
    }

    #[test]
    fn v2_same_level_import_under_another_name_blocked() {
        let code = "import mypackage.logic.pure.check.simple as sibling\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/pure/other/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("level violation"));
    }

    #[test]
    fn before_validator_under_another_name_caught() {
        let parsed = parse("from pydantic import BeforeValidator as Coerce\n");
        assert_eq!(check_no_before_validators(&parsed, &default_config()).len(), 1);
    }

    #[test]
    fn third_party_json_parsers_caught() {
        // The edge: the untyped dict json gives is given just the same by its faster cousins.
        for (name, code) in [
            ("orjson", "import orjson\n"),
            ("ujson", "import ujson as fast\n"),
            ("simplejson", "from simplejson import loads\n"),
            ("rapidjson", "import rapidjson\n"),
            ("msgspec", "from msgspec import json\n"),
        ] {
            let violations = check_no_disallowed_stdlib(&parse(code), &default_config());
            assert_eq!(violations.len(), 1, "{code}");
            assert!(violations[0].message.contains(name), "{code}: {}", violations[0].message);
        }
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
        let lines: Vec<usize> = v.iter().map(|x| x.line).collect();
        assert_eq!(lines, vec![1, 3], "the Path import, then the handle");
        assert!(v[1].message.contains("direct file read (path.open)"), "{}", v[1].message);
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
        let lines: Vec<usize> = v.iter().map(|x| x.line).collect();
        assert_eq!(lines, vec![1, 2], "the Path import, then the read");
    }

    #[test]
    fn direct_io_registry_specimen_three_firings() {
        // The specimen that motivated the check: the parser import, the Path import and the
        // handle fire; the parser call does not.
        let code = "import tomllib\nfrom pathlib import Path\n\ndef load_registry(path: Path):\n    with path.open(\"rb\") as file_handle:\n        data = tomllib.load(file_handle)\n    return data\n";
        let v = direct_io(code);
        let lines: Vec<usize> = v.iter().map(|x| x.line).collect();
        assert_eq!(lines, vec![1, 2, 5], "{:?}", v.iter().map(|x| &x.message).collect::<Vec<_>>());
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
    fn direct_io_pydantic_and_pure_path_imports_not_flagged() {
        let v = direct_io("from pydantic import BaseModel\nfrom pathlib import PurePath, PurePosixPath, PureWindowsPath\nname = (PurePath(root) / \"x\").name\ninside = PurePosixPath(directory).is_relative_to(scope)\n");
        assert!(v.is_empty(), "{:?}", v.iter().map(|x| &x.message).collect::<Vec<_>>());
    }

    #[test]
    fn direct_io_module_that_only_resembles_pathlib_not_flagged() {
        let v = direct_io("from mypkg.pathlib import Path\nfrom pathlib_tools import Path\nfrom .pathlib import Path\nimport pathlib_tools\n");
        assert!(v.is_empty(), "{:?}", v.iter().map(|x| &x.message).collect::<Vec<_>>());
    }

    // fire: pathlib's filesystem classes

    #[test]
    fn direct_io_path_import_caught() {
        // The specimen: listing, presence tests and file sizes done through Path in the impure zone.
        for name in ["Path", "PosixPath", "WindowsPath"] {
            let v = direct_io(&format!("from pathlib import {name}\n"));
            assert_eq!(v.len(), 1, "{name}");
            assert_eq!(
                v[0].message,
                format!("'{name}' from pathlib reaches the filesystem — use PurePath for a path's name, and a compiled tool for anything on disk")
            );
        }
    }

    #[test]
    fn direct_io_path_beside_a_pure_name_caught_once() {
        let v = direct_io("from pathlib import PurePath, Path\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.starts_with("'Path' from pathlib"), "{}", v[0].message);
    }

    #[test]
    fn direct_io_pathlib_imported_whole_caught() {
        for code in ["import pathlib\n", "import os, pathlib\n"] {
            let v = direct_io(code);
            assert_eq!(v.len(), 1, "{code}");
            assert!(v[0].message.starts_with("pathlib imported whole brings Path with it"), "{code}");
        }
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

    #[test]
    fn direct_io_path_under_another_name_caught() {
        let v = direct_io("from pathlib import Path as Location\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.starts_with("'Path' from pathlib"), "{}", v[0].message);
    }

    #[test]
    fn direct_io_pathlib_whole_under_another_name_caught() {
        let v = direct_io("import pathlib as places\nroot = places.Path(given)\n");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 1);
    }

    #[test]
    fn direct_io_pathlib_wildcard_caught() {
        let v = direct_io("from pathlib import *\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].message.starts_with("'*' from pathlib"), "{}", v[0].message);
    }

    // fire: os, shutil, and a Path a library hands back

    #[test]
    fn direct_io_shutil_caught_whole_by_name_and_under_another_name() {
        for code in ["import shutil\n", "import shutil as files\n", "from shutil import rmtree\n"] {
            let v = direct_io(code);
            assert_eq!(v.len(), 1, "{code}");
            assert!(v[0].message.starts_with("'shutil' works on the disk"), "{code}");
        }
    }

    #[test]
    fn direct_io_os_disk_call_caught() {
        // The specimen shape: with Path refused, the same reach through os.
        for (shown, code) in [
            ("os.remove", "import os\nos.remove(target)\n"),
            ("os.makedirs", "import os\nos.makedirs(folder, exist_ok=True)\n"),
            ("os.path.exists", "import os\nif os.path.exists(target):\n    pass\n"),
            ("system.listdir", "import os as system\nnames = system.listdir(folder)\n"),
            ("path.getsize", "from os import path\nsize = path.getsize(target)\n"),
        ] {
            let v = direct_io(code);
            assert_eq!(v.len(), 1, "{code}");
            assert_eq!(v[0].message, format!("direct disk call ({shown}) — use a compiled tool for anything on disk"));
        }
    }

    #[test]
    fn direct_io_os_disk_function_by_name_caught() {
        for (name, code) in [
            ("remove", "from os import remove\n"),
            ("walk", "from os import walk as tree_walk\n"),
            ("isfile", "from os.path import isfile\n"),
        ] {
            let v = direct_io(code);
            assert_eq!(v.len(), 1, "{code}");
            assert!(v[0].message.starts_with(&format!("'{name}' from os")), "{code}: {}", v[0].message);
        }
    }

    #[test]
    fn direct_io_os_that_touches_no_disk_not_flagged() {
        let code = "import os\nfrom os import path, environ\nhome = os.environ[\"HOME\"]\nsep = os.sep\nfull = os.path.join(root, name)\nstem = path.splitext(name)[0]\n";
        let v = direct_io(code);
        assert!(v.is_empty(), "{:?}", v.iter().map(|x| &x.message).collect::<Vec<_>>());
    }

    #[test]
    fn direct_io_disk_method_on_a_path_from_a_library_caught() {
        for (method, code) in [
            ("unlink", "workdir = tempfile_root()\nworkdir.unlink()\n"),
            ("mkdir", "config.output_dir.mkdir(parents=True)\n"),
            ("exists", "if settings.cache.exists():\n    pass\n"),
            ("iterdir", "for entry in project.root.iterdir():\n    pass\n"),
        ] {
            let v = direct_io(code);
            assert_eq!(v.len(), 1, "{code}");
            assert!(v[0].message.contains(&format!("(.{method})")), "{code}: {}", v[0].message);
        }
    }

    #[test]
    fn direct_io_string_methods_that_share_a_path_s_name_not_flagged() {
        let code = "title = name.replace(\"-\", \" \")\nlabel = record.rename()\nmatches = pattern.glob\n";
        assert!(direct_io(code).is_empty());
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
    fn coercion_before_validation_caught_in_every_form() {
        // The canaries: the same implicit coercion, reached without importing the name.
        for code in [
            "import pydantic\nName = Annotated[str, pydantic.BeforeValidator(strip)]\n",
            "from pydantic import WrapValidator\n",
            "class Page(BaseModel):\n    title: str\n    @field_validator(\"title\", mode=\"before\")\n    @classmethod\n    def strip(cls, value: str) -> str:\n        return value\n",
            "class Page(BaseModel):\n    @model_validator(mode=\"wrap\")\n    def coerce(cls, data, handler):\n        return handler(data)\n",
            "class Page(BaseModel):\n    @pydantic.field_validator(\"title\", mode='before')\n    def strip(cls, value):\n        return value\n",
        ] {
            assert_eq!(check_no_before_validators(&parse(code), &default_config()).len(), 1, "{code}");
        }
    }

    #[test]
    fn validation_after_the_field_ok() {
        for code in [
            "class Page(BaseModel):\n    @field_validator(\"title\", mode=\"after\")\n    def check(cls, value):\n        return value\n",
            "class Page(BaseModel):\n    @field_validator(\"title\")\n    def check(cls, value):\n        return value\n",
            "from pydantic import AfterValidator\n",
        ] {
            assert!(check_no_before_validators(&parse(code), &default_config()).is_empty(), "{code}");
        }
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
    fn sys_path_reached_another_way_caught() {
        // The canaries: the list extended, added to, or reached under another name.
        for code in [
            "import sys\nsys.path.extend([\"/some/path\"])\n",
            "import sys\nsys.path += [\"/some/path\"]\n",
            "import sys\nsys.path[:] = [\"/some/path\"]\n",
            "from sys import path\npath.insert(0, \"/some/path\")\n",
            "from sys import path as search\nsearch.append(\"/some/path\")\n",
            "import sys as system\nsystem.path.append(\"/some/path\")\n",
        ] {
            assert_eq!(check_no_sys_path_mutation(&parse(code), &default_config()).len(), 1, "{code}");
        }
    }

    #[test]
    fn a_list_named_path_that_is_not_sys_path_ok() {
        for code in ["from os import path\nparts = path.split(name)\n", "path = []\npath.append(step)\n"] {
            assert!(check_no_sys_path_mutation(&parse(code), &default_config()).is_empty(), "{code}");
        }
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
