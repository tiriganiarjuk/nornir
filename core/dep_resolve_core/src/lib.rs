//! Pure dependency resolution for Python source files.
//!
//! Given a Python file's source bytes and a callback for reading dependency files,
//! resolves imported symbols to their definitions and renders them as context.
//!
//! Two modes:
//!   Signatures — compact interface contracts (function sigs, class fields, constants)
//!   Full — complete definition bodies
//!
//! Output is topologically ordered: deepest dependencies first.
//! No I/O — callers provide source bytes via callback.

pub mod extract;
pub mod references;
pub mod resolve;
pub mod render;
mod ts;

/// Resolution mode: what to extract from dependency definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveMode {
    /// Bare signatures: `def name(params) -> ret`. Minimal token cost.
    Signatures,
    /// Direct deps get full bodies (stripped), transitive deps get signatures.
    Hybrid,
    /// All levels get full bodies (stripped of docstrings and comments).
    Full,
}

/// A resolved symbol extracted from a dependency file.
#[derive(Debug, Clone)]
pub struct ResolvedSymbol {
    /// The imported name (e.g. "resolve_schema_ref").
    pub name: String,
    /// The full import path (e.g. "draupnir.logic.pure.path_operations.resolve_schema_ref").
    pub import_path: String,
    /// Kind of symbol: function, class, constant, type_alias, unknown.
    pub kind: SymbolKind,
    /// Rendered text (signature or full body depending on mode).
    pub rendered: String,
}

/// Classification of a resolved symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
    Constant,
    TypeAlias,
    Unknown,
}

/// Result of resolving dependencies for a single file.
#[derive(Debug, Clone)]
pub struct ResolutionResult {
    /// The target file path (relative to project root).
    pub target_file: String,
    /// Resolved dependencies grouped by source file, in topological order (deepest first).
    pub groups: Vec<DependencyGroup>,
}

/// A group of symbols from a single dependency file.
#[derive(Debug, Clone)]
pub struct DependencyGroup {
    /// Source file path relative to project root.
    pub source_file: String,
    /// Resolved symbols from this file.
    pub symbols: Vec<ResolvedSymbol>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Extract: function signatures ────────────────────────────

    #[test]
    fn extract_function_signature() {
        let source = b"def resolve_ref(path: str, base: Path) -> ResolvedRef:\n    \"\"\"Resolve a schema ref.\"\"\"\n    return ResolvedRef(path)\n";
        let tree = ts::parse_python(source).unwrap();
        let names = vec!["resolve_ref".to_string()];
        let syms = extract::extract_symbols(source, &tree, &names, ResolveMode::Signatures);

        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].kind, SymbolKind::Function);
        assert!(syms[0].rendered.contains("def resolve_ref(path: str, base: Path) -> ResolvedRef"));
        // Signatures mode strips docstrings — no comment appended
        assert!(!syms[0].rendered.contains("#"));
    }

    #[test]
    fn extract_function_full_body() {
        let source = b"def add(a: int, b: int) -> int:\n    \"\"\"Add two numbers.\"\"\"\n    return a + b\n";
        let tree = ts::parse_python(source).unwrap();
        let names = vec!["add".to_string()];
        let syms = extract::extract_symbols(source, &tree, &names, ResolveMode::Full);

        assert_eq!(syms.len(), 1);
        assert!(syms[0].rendered.contains("return a + b"));
        // Full mode strips docstrings
        assert!(!syms[0].rendered.contains("Add two numbers"));
    }

    #[test]
    fn extract_skips_unrequested_names() {
        let source = b"def foo(): pass\ndef bar(): pass\n";
        let tree = ts::parse_python(source).unwrap();
        let names = vec!["bar".to_string()];
        let syms = extract::extract_symbols(source, &tree, &names, ResolveMode::Signatures);

        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "bar");
    }

    // ── Extract: class signatures ───────────────────────────────

    #[test]
    fn extract_class_signature() {
        let source = b"class Config(BaseModel):\n    host: str\n    port: int = 8080\n    def validate(self) -> bool: ...\n";
        let tree = ts::parse_python(source).unwrap();
        let names = vec!["Config".to_string()];
        let syms = extract::extract_symbols(source, &tree, &names, ResolveMode::Signatures);

        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].kind, SymbolKind::Class);
        let rendered = &syms[0].rendered;
        assert!(rendered.contains("class Config(BaseModel):"));
        assert!(rendered.contains("def validate(self) -> bool"));
    }

    // ── Extract: constants and type aliases ──────────────────────

    #[test]
    fn extract_constant() {
        let source = b"MAX_RETRIES = 3\n";
        let tree = ts::parse_python(source).unwrap();
        let names = vec!["MAX_RETRIES".to_string()];
        let syms = extract::extract_symbols(source, &tree, &names, ResolveMode::Signatures);

        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].kind, SymbolKind::Constant);
        assert!(syms[0].rendered.contains("MAX_RETRIES = 3"));
    }

    // ── Extract: wildcard ───────────────────────────────────────

    #[test]
    fn extract_wildcard_gets_all_public() {
        let source = b"def public_fn(): pass\ndef _private(): pass\nVALUE = 42\n";
        let tree = ts::parse_python(source).unwrap();
        let names = vec!["*".to_string()];
        let syms = extract::extract_symbols(source, &tree, &names, ResolveMode::Signatures);

        let extracted_names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(extracted_names.contains(&"public_fn"));
        assert!(extracted_names.contains(&"VALUE"));
        // _private should still be extracted by extract_symbols with want_all
        // (filtering of _ prefixed names is in extract_all_top_level_names for wildcard resolution)
    }

    // ── Extract: decorated functions ────────────────────────────

    #[test]
    fn extract_decorated_function_signature() {
        let source = b"@validator\ndef check_name(value: str) -> str:\n    return value\n";
        let tree = ts::parse_python(source).unwrap();
        let names = vec!["check_name".to_string()];
        let syms = extract::extract_symbols(source, &tree, &names, ResolveMode::Signatures);

        assert_eq!(syms.len(), 1);
        assert!(syms[0].rendered.contains("@validator"));
        assert!(syms[0].rendered.contains("def check_name(value: str) -> str"));
    }

    #[test]
    fn extract_decorated_function_full() {
        let source = b"@validator\ndef check_name(value: str) -> str:\n    # validation logic\n    return value\n";
        let tree = ts::parse_python(source).unwrap();
        let names = vec!["check_name".to_string()];
        let syms = extract::extract_symbols(source, &tree, &names, ResolveMode::Full);

        assert_eq!(syms.len(), 1);
        assert!(syms[0].rendered.contains("@validator"));
        assert!(syms[0].rendered.contains("return value"));
        // Comment lines are stripped
        assert!(!syms[0].rendered.contains("# validation logic"));
    }

    // ── Extract: async functions ────────────────────────────────

    #[test]
    fn extract_async_function() {
        let source = b"async def fetch(url: str) -> Response:\n    pass\n";
        let tree = ts::parse_python(source).unwrap();
        let names = vec!["fetch".to_string()];
        let syms = extract::extract_symbols(source, &tree, &names, ResolveMode::Signatures);

        assert_eq!(syms.len(), 1);
        assert!(syms[0].rendered.starts_with("async def fetch"));
    }

    // ── References ──────────────────────────────────────────────

    /// The names mentioned by the first top-level statement of `source`.
    fn references_of_first(source: &[u8]) -> Vec<String> {
        let tree = ts::parse_python(source).unwrap();
        let definition = tree.root_node().named_child(0).unwrap();
        references::collect_references(definition, source)
            .into_iter()
            .collect()
    }

    fn has(names: &[String], wanted: &str) -> bool {
        names.iter().any(|name| name == wanted)
    }

    #[test]
    fn references_include_body_names() {
        let names = references_of_first(b"def f(x):\n    return helper(x) + LIMIT\n");
        assert!(has(&names, "helper"));
        assert!(has(&names, "LIMIT"));
    }

    #[test]
    fn references_include_annotations_defaults_and_return_type() {
        let names =
            references_of_first(b"def f(a: Alpha, b: int = FALLBACK) -> Beta:\n    pass\n");
        assert!(has(&names, "Alpha"));
        assert!(has(&names, "FALLBACK"));
        assert!(has(&names, "Beta"));
    }

    #[test]
    fn references_include_decorators() {
        let names = references_of_first(b"@register(Kind)\ndef f():\n    pass\n");
        assert!(has(&names, "register"));
        assert!(has(&names, "Kind"));
    }

    #[test]
    fn references_exclude_attribute_side() {
        let names = references_of_first(b"def f(obj):\n    return obj.helper.deeper\n");
        assert!(has(&names, "obj"));
        assert!(!has(&names, "helper"));
        assert!(!has(&names, "deeper"));
    }

    #[test]
    fn references_exclude_keyword_argument_names() {
        let names = references_of_first(b"def f():\n    return build(label=VALUE)\n");
        assert!(has(&names, "build"));
        assert!(has(&names, "VALUE"));
        assert!(!has(&names, "label"));
    }

    #[test]
    fn references_of_class_cover_bases_fields_and_methods() {
        let source = b"class C(Base):\n    field: FieldType = make()\n    def m(self) -> Out:\n        return util(self.x)\n";
        let names = references_of_first(source);
        for wanted in ["Base", "FieldType", "make", "Out", "util"] {
            assert!(has(&names, wanted), "missing {wanted}");
        }
        assert!(!has(&names, "x"));
    }

    #[test]
    fn references_are_sorted_and_unique() {
        let names = references_of_first(b"def f():\n    return zeta(alpha) + zeta(alpha)\n");
        assert_eq!(names, vec!["alpha", "f", "zeta"]);
    }

    // ── Render: injection ───────────────────────────────────────

    #[test]
    fn render_injection_empty_groups() {
        let result = ResolutionResult {
            target_file: "main.py".to_string(),
            groups: vec![],
        };
        assert_eq!(render::render_injection(&result), "");
    }

    #[test]
    fn render_injection_with_symbols() {
        let result = ResolutionResult {
            target_file: "main.py".to_string(),
            groups: vec![DependencyGroup {
                source_file: "lib/utils.py".to_string(),
                symbols: vec![ResolvedSymbol {
                    name: "helper".to_string(),
                    import_path: "lib.utils.helper".to_string(),
                    kind: SymbolKind::Function,
                    rendered: "def helper(x: int) -> int".to_string(),
                }],
            }],
        };
        let output = render::render_injection(&result);
        assert!(output.contains("Dependency interfaces for: main.py"));
        assert!(output.contains("lib/utils.py"));
        assert!(output.contains("def helper(x: int) -> int"));
    }

    // ── Render: simulate ────────────────────────────────────────

    #[test]
    fn render_simulate_shows_banners() {
        let result = ResolutionResult {
            target_file: "main.py".to_string(),
            groups: vec![],
        };
        let output = render::render_simulate(&result, "print('hello')", ResolveMode::Signatures);
        assert!(output.contains("INJECTION (signatures)"));
        assert!(output.contains("FILE CONTENT"));
        assert!(output.contains("print('hello')"));
    }
}
