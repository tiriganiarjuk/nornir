//! Pure dependency resolution for Python source, at symbol level.
//!
//! Two entries over one engine:
//!   resolve_file     — the symbols a file imports, and what those use, transitively
//!   resolve_function — one top-level definition, and what it uses, transitively
//!
//! A reference is any name a definition mentions (see `references`), matched
//! against what its file defines at top level and what it imports. The result
//! is a `Closure`: symbols in canonical order (dependencies first, ties by
//! `(file, name)`), plus findings — dependency problems that are reported, never
//! silently absorbed. Same source tree, same bytes.
//!
//! No I/O of its own — callers provide file bytes through a callback.

use std::fmt;

pub mod digest;
pub mod extract;
pub mod order;
pub mod references;
pub mod render;
pub mod resolve;
mod ts;

/// Resolution mode: what to render for each symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveMode {
    /// Bare signatures: `def name(params) -> ret`. Minimal token cost.
    Signatures,
    /// Depth 0 and 1 get full (stripped) bodies, deeper symbols get signatures.
    Hybrid,
    /// Every symbol gets its full body, stripped of docstrings and comments.
    Full,
}

impl ResolveMode {
    /// Parse a mode name as given on a command line or in an environment variable.
    pub fn parse(value: &str) -> Result<ResolveMode, String> {
        match value {
            "signatures" | "sig" => Ok(ResolveMode::Signatures),
            "hybrid" => Ok(ResolveMode::Hybrid),
            "full" => Ok(ResolveMode::Full),
            other => Err(format!(
                "unknown mode '{other}' (expected: signatures, hybrid, full)"
            )),
        }
    }

    /// The mode in force for a symbol at `depth` from the entry.
    pub fn at_depth(self, depth: usize) -> ResolveMode {
        match self {
            ResolveMode::Hybrid if depth > 1 => ResolveMode::Signatures,
            other => other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ResolveMode::Signatures => "signatures",
            ResolveMode::Hybrid => "hybrid",
            ResolveMode::Full => "full",
        }
    }
}

/// Classification of a top-level definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
    Constant,
    TypeAlias,
}

/// A symbol in a closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSymbol {
    /// Defining file, relative to the project root.
    pub file: String,
    /// The top-level name.
    pub name: String,
    pub kind: SymbolKind,
    /// Reference hops from the entry: 0 is the target function itself,
    /// 1 the names it references directly (or, for a file, its imports).
    pub depth: usize,
    /// Rendered text (signature or stripped body, per mode and depth).
    pub rendered: String,
}

impl ResolvedSymbol {
    /// The symbol's canonical key, `file::name`.
    pub fn key(&self) -> String {
        format!("{}::{}", self.file, self.name)
    }
}

/// A dependency problem found while resolving. Reported, never fatal: the
/// closure is still rendered, but no digest is produced while any stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Symbols that depend on each other, so none can be defined before the rest.
    Cycle { members: Vec<String> },
    /// `import a.b` of a project-local module: the names used are not stated.
    PlainProjectImport { file: String, line: usize, module: String },
    /// `name` is imported from `file`, but that file does not define it.
    Unresolved { file: String, name: String },
    /// A dependency file could not be read or parsed.
    Unreadable { file: String },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Finding::Cycle { members } => write!(
                f,
                "dependency cycle: {} — these depend on each other, so none can be defined \
                 before the others; what they share belongs below all of them, where each \
                 can import it",
                members.join(", ")
            ),
            Finding::PlainProjectImport { file, line, module } => write!(
                f,
                "{file}:{line}: plain import of project module `{module}` — a project-local \
                 import names what it uses: `from {module} import name`"
            ),
            Finding::Unresolved { file, name } => write!(
                f,
                "`{name}` is imported from {file}, which does not define it — the import \
                 should point at the file that defines `{name}`"
            ),
            Finding::Unreadable { file } => {
                write!(f, "{file} could not be read or parsed")
            }
        }
    }
}

/// The result of resolving an entry: symbols in canonical order, and findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closure {
    /// `path` for a file entry, `path::name` for a function entry.
    pub entry: String,
    pub symbols: Vec<ResolvedSymbol>,
    pub findings: Vec<Finding>,
}

impl Closure {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// A failure that leaves nothing to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The target file does not parse as Python.
    Parse { file: String },
    /// The named function target is not a top-level definition in its file.
    NotTopLevel { file: String, name: String },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::Parse { file } => write!(f, "{file}: not parseable as Python"),
            ResolveError::NotTopLevel { file, name } => {
                write!(f, "{file}: `{name}` is not a top-level definition")
            }
        }
    }
}

impl std::error::Error for ResolveError {}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::resolve::{ResolveConfig, Target};

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

    // ── Extract: constants, type aliases, wildcard ───────────────

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

    #[test]
    fn extract_wildcard_gets_all_public() {
        let source = b"def public_fn(): pass\ndef _private(): pass\nVALUE = 42\ntype Row = dict[str, int]\n";
        let tree = ts::parse_python(source).unwrap();
        let names = extract::extract_all_top_level_names(source, &tree);
        assert_eq!(names, vec!["public_fn", "VALUE", "Row"]);
    }

    #[test]
    fn find_top_level_sees_definitions_only() {
        let source = b"from a import imported\n@deco\ndef f(): pass\nclass C: pass\nX = 1\n";
        let tree = ts::parse_python(source).unwrap();
        assert_eq!(extract::find_top_level(&tree, source, "f").map(|n| n.kind()), Some("decorated_definition"));
        assert_eq!(extract::find_top_level(&tree, source, "C").map(|n| n.kind()), Some("class_definition"));
        assert_eq!(extract::find_top_level(&tree, source, "X").map(|n| n.kind()), Some("expression_statement"));
        assert!(extract::find_top_level(&tree, source, "imported").is_none());
    }

    // ── Extract: decorated and async functions ──────────────────

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
        assert!(!syms[0].rendered.contains("# validation logic"));
    }

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

    // ── Order ───────────────────────────────────────────────────

    fn sym(file: &str, name: &str) -> ResolvedSymbol {
        ResolvedSymbol {
            file: file.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            depth: 0,
            rendered: String::new(),
        }
    }

    fn keys(symbols: &[ResolvedSymbol]) -> Vec<String> {
        symbols.iter().map(ResolvedSymbol::key).collect()
    }

    #[test]
    fn order_puts_dependencies_first_then_alphabetical() {
        // main references both; zeta references alpha.
        let symbols = vec![sym("m.py", "main"), sym("z.py", "zeta"), sym("a.py", "alpha")];
        let edges = vec![
            ("m.py::main".to_string(), "z.py::zeta".to_string()),
            ("m.py::main".to_string(), "a.py::alpha".to_string()),
            ("z.py::zeta".to_string(), "a.py::alpha".to_string()),
        ];
        let (ordered, findings) = order::canonical_order(symbols, &edges);
        assert_eq!(keys(&ordered), vec!["a.py::alpha", "z.py::zeta", "m.py::main"]);
        assert!(findings.is_empty());
    }

    #[test]
    fn order_is_independent_of_input_order() {
        let edges = vec![("b.py::b".to_string(), "a.py::a".to_string())];
        let (one, _) = order::canonical_order(vec![sym("a.py", "a"), sym("b.py", "b"), sym("c.py", "c")], &edges);
        let (two, _) = order::canonical_order(vec![sym("c.py", "c"), sym("b.py", "b"), sym("a.py", "a")], &edges);
        assert_eq!(keys(&one), keys(&two));
        assert_eq!(keys(&one), vec!["a.py::a", "b.py::b", "c.py::c"]);
    }

    #[test]
    fn order_reports_a_cycle_and_still_places_everything() {
        let symbols = vec![sym("f.py", "even"), sym("f.py", "odd"), sym("f.py", "main")];
        let edges = vec![
            ("f.py::even".to_string(), "f.py::odd".to_string()),
            ("f.py::odd".to_string(), "f.py::even".to_string()),
            ("f.py::main".to_string(), "f.py::even".to_string()),
        ];
        let (ordered, findings) = order::canonical_order(symbols, &edges);
        assert_eq!(ordered.len(), 3);
        assert_eq!(
            findings,
            vec![Finding::Cycle { members: vec!["f.py::even".to_string(), "f.py::odd".to_string()] }]
        );
        assert_eq!(keys(&ordered), vec!["f.py::even", "f.py::odd", "f.py::main"]);
    }

    // ── Digest ──────────────────────────────────────────────────

    #[test]
    fn digest_is_sha256_hex_of_the_text() {
        assert_eq!(
            digest::closure_digest("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // ── Engine: fixtures on disk ────────────────────────────────
    //
    // The module resolver probes the filesystem, so fixtures are real files.

    fn fixture(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join("dep_resolve_core_tests").join(name);
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        for (rel, content) in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
        }
        root
    }

    fn path_str(path: &Path) -> String {
        path.to_string_lossy().to_string()
    }

    fn read(path: &str) -> Option<Vec<u8>> {
        fs::read(path).ok()
    }

    fn config(mode: ResolveMode) -> ResolveConfig {
        ResolveConfig { mode, max_depth: 10 }
    }

    fn resolve_with<T>(root: &Path, file: &str, run: impl FnOnce(&Target) -> T) -> T {
        let path = root.join(file);
        let source = fs::read(&path).unwrap();
        let target = Target { file_path: &path_str(&path), source: &source, project_root: &path_str(root) };
        run(&target)
    }

    fn function_closure(root: &Path, file: &str, name: &str, mode: ResolveMode) -> Closure {
        resolve_with(root, file, |target| resolve::resolve_function(target, name, read, &config(mode)).unwrap())
    }

    fn file_closure(root: &Path, file: &str, mode: ResolveMode) -> Closure {
        resolve_with(root, file, |target| resolve::resolve_file(target, read, &config(mode)).unwrap())
    }

    const APP_MAIN: &str = "from app.lib import f, unused_in_main\nfrom app.models import Row\n\nLIMIT = 3\n\ndef helper(row: Row) -> int:\n    return row.n + LIMIT\n\ndef main(row: Row) -> int:\n    return f(helper(row))\n";
    const APP_LIB: &str = "from app.deep import base\nfrom app.extra import never_used\n\ndef f(n: int) -> int:\n    return base(n)\n\ndef unused_in_main() -> int:\n    return never_used()\n";
    const APP_DEEP: &str = "def base(n: int) -> int:\n    return n\n";
    const APP_EXTRA: &str = "def never_used() -> int:\n    return 0\n";
    const APP_MODELS: &str = "class Row:\n    n: int\n";

    fn app_fixture(name: &str) -> PathBuf {
        fixture(
            name,
            &[
                ("app/main.py", APP_MAIN),
                ("app/lib.py", APP_LIB),
                ("app/deep.py", APP_DEEP),
                ("app/extra.py", APP_EXTRA),
                ("app/models.py", APP_MODELS),
            ],
        )
    }

    #[test]
    fn function_closure_follows_same_file_helpers_and_imports() {
        let root = app_fixture("function_closure");
        let closure = function_closure(&root, "app/main.py", "main", ResolveMode::Full);
        assert!(closure.is_clean(), "{:?}", closure.findings);
        assert_eq!(
            keys(&closure.symbols),
            vec![
                "app/deep.py::base",
                "app/lib.py::f",
                "app/main.py::LIMIT",
                "app/models.py::Row",
                "app/main.py::helper",
                "app/main.py::main",
            ]
        );
        assert_eq!(closure.entry, "app/main.py::main");
        let main = closure.symbols.last().unwrap();
        assert_eq!(main.depth, 0);
        assert!(main.rendered.contains("return f(helper(row))"));
    }

    #[test]
    fn function_closure_excludes_what_the_referenced_symbols_do_not_use() {
        let root = app_fixture("function_closure_precise");
        let closure = function_closure(&root, "app/main.py", "main", ResolveMode::Full);
        let keys = keys(&closure.symbols);
        assert!(!keys.iter().any(|k| k.contains("unused_in_main")));
        assert!(!keys.iter().any(|k| k.contains("never_used")));
    }

    #[test]
    fn file_closure_covers_imports_not_the_target_itself() {
        let root = app_fixture("file_closure");
        let closure = file_closure(&root, "app/main.py", ResolveMode::Signatures);
        assert!(closure.is_clean(), "{:?}", closure.findings);
        assert_eq!(
            keys(&closure.symbols),
            vec![
                "app/deep.py::base",
                "app/extra.py::never_used",
                "app/lib.py::f",
                "app/lib.py::unused_in_main",
                "app/models.py::Row",
            ]
        );
        assert_eq!(closure.entry, "app/main.py");
        assert!(closure.symbols.iter().all(|s| s.depth >= 1));
    }

    #[test]
    fn hybrid_renders_bodies_near_the_entry_and_signatures_deeper() {
        let root = app_fixture("hybrid");
        let closure = function_closure(&root, "app/main.py", "main", ResolveMode::Hybrid);
        let by_name = |name: &str| closure.symbols.iter().find(|s| s.name == name).unwrap();
        assert!(by_name("helper").rendered.contains("return row.n + LIMIT"));
        assert!(by_name("f").rendered.contains("return base(n)"));
        assert!(!by_name("base").rendered.contains("return n"));
        assert_eq!(by_name("base").depth, 2);
    }

    #[test]
    fn max_depth_stops_following_references() {
        let root = app_fixture("max_depth");
        let shallow = ResolveConfig { mode: ResolveMode::Full, max_depth: 1 };
        let closure = resolve_with(&root, "app/main.py", |target| {
            resolve::resolve_function(target, "main", read, &shallow).unwrap()
        });
        let keys = keys(&closure.symbols);
        assert!(keys.contains(&"app/lib.py::f".to_string()));
        assert!(!keys.contains(&"app/deep.py::base".to_string()));
    }

    #[test]
    fn aliased_import_resolves_to_the_original_name() {
        let root = fixture(
            "alias",
            &[
                ("pkg/a.py", "from pkg.b import original as local\n\ndef f():\n    return local()\n"),
                ("pkg/b.py", "def original():\n    return 1\n"),
            ],
        );
        let closure = function_closure(&root, "pkg/a.py", "f", ResolveMode::Full);
        assert!(closure.is_clean());
        assert_eq!(keys(&closure.symbols), vec!["pkg/b.py::original", "pkg/a.py::f"]);
    }

    #[test]
    fn wildcard_import_resolves_the_names_actually_used() {
        let root = fixture(
            "wildcard",
            &[
                ("pkg/a.py", "from pkg.b import *\n\ndef f():\n    return used()\n"),
                ("pkg/b.py", "def used():\n    return 1\n\ndef not_used():\n    return 2\n"),
            ],
        );
        let closure = function_closure(&root, "pkg/a.py", "f", ResolveMode::Full);
        assert!(closure.is_clean());
        assert_eq!(keys(&closure.symbols), vec!["pkg/b.py::used", "pkg/a.py::f"]);
        let file = file_closure(&root, "pkg/a.py", ResolveMode::Full);
        assert_eq!(keys(&file.symbols), vec!["pkg/b.py::not_used", "pkg/b.py::used"]);
    }

    #[test]
    fn same_file_cycle_is_a_finding() {
        let root = fixture(
            "same_file_cycle",
            &[("pkg/m.py", "def even(n):\n    return n == 0 or odd(n - 1)\n\ndef odd(n):\n    return n != 0 and even(n - 1)\n")],
        );
        let closure = function_closure(&root, "pkg/m.py", "even", ResolveMode::Full);
        assert_eq!(
            closure.findings,
            vec![Finding::Cycle { members: vec!["pkg/m.py::even".to_string(), "pkg/m.py::odd".to_string()] }]
        );
        assert_eq!(closure.symbols.len(), 2);
    }

    #[test]
    fn cross_file_cycle_is_a_finding() {
        let root = fixture(
            "cross_file_cycle",
            &[
                ("pkg/a.py", "from pkg.b import g\n\ndef f():\n    return g()\n"),
                ("pkg/b.py", "from pkg.a import f\n\ndef g():\n    return f()\n"),
            ],
        );
        let closure = file_closure(&root, "pkg/a.py", ResolveMode::Full);
        assert_eq!(
            closure.findings,
            vec![Finding::Cycle { members: vec!["pkg/a.py::f".to_string(), "pkg/b.py::g".to_string()] }]
        );
    }

    #[test]
    fn self_recursion_is_not_a_cycle() {
        let root = fixture("self_recursion", &[("pkg/m.py", "def fact(n):\n    return 1 if n < 2 else n * fact(n - 1)\n")]);
        let closure = function_closure(&root, "pkg/m.py", "fact", ResolveMode::Full);
        assert!(closure.is_clean());
        assert_eq!(closure.symbols.len(), 1);
    }

    #[test]
    fn plain_project_import_is_a_finding_and_third_party_is_not() {
        let root = fixture(
            "plain_import",
            &[
                ("pkg/a.py", "import numpy\nimport pkg.b\n\ndef f():\n    return pkg.b.g()\n"),
                ("pkg/b.py", "def g():\n    return 1\n"),
            ],
        );
        let closure = function_closure(&root, "pkg/a.py", "f", ResolveMode::Full);
        assert_eq!(
            closure.findings,
            vec![Finding::PlainProjectImport { file: "pkg/a.py".to_string(), line: 2, module: "pkg.b".to_string() }]
        );
    }

    #[test]
    fn import_of_an_undefined_name_is_a_finding() {
        let root = fixture(
            "unresolved",
            &[
                ("pkg/a.py", "from pkg.b import missing\n\ndef f():\n    return missing()\n"),
                ("pkg/b.py", "def present():\n    return 1\n"),
            ],
        );
        let closure = function_closure(&root, "pkg/a.py", "f", ResolveMode::Full);
        assert_eq!(
            closure.findings,
            vec![Finding::Unresolved { file: "pkg/b.py".to_string(), name: "missing".to_string() }]
        );
    }

    #[test]
    fn function_target_must_be_top_level() {
        let root = fixture("not_top_level", &[("pkg/a.py", "def outer():\n    def inner():\n        pass\n    return inner\n")]);
        let err = resolve_with(&root, "pkg/a.py", |target| {
            resolve::resolve_function(target, "inner", read, &config(ResolveMode::Full)).unwrap_err()
        });
        assert_eq!(err, ResolveError::NotTopLevel { file: "pkg/a.py".to_string(), name: "inner".to_string() });
    }

    // ── Render + digest: canonical bytes ────────────────────────

    #[test]
    fn rendering_ignores_import_order_and_docstrings() {
        let one = fixture(
            "canon_one",
            &[
                ("pkg/a.py", "from pkg.b import g\nfrom pkg.c import h\n\ndef f():\n    \"\"\"Doc.\"\"\"\n    return g() + h()\n"),
                ("pkg/b.py", "def g():\n    return 1\n"),
                ("pkg/c.py", "def h():\n    return 2\n"),
            ],
        );
        let two = fixture(
            "canon_two",
            &[
                ("pkg/a.py", "from pkg.c import h\nfrom pkg.b import g\n\ndef f():\n    # a comment\n    return g() + h()\n"),
                ("pkg/b.py", "def g():\n    \"\"\"Other doc.\"\"\"\n    return 1\n"),
                ("pkg/c.py", "def h():\n    return 2\n"),
            ],
        );
        let text_one = render::render_symbols(&function_closure(&one, "pkg/a.py", "f", ResolveMode::Full));
        let text_two = render::render_symbols(&function_closure(&two, "pkg/a.py", "f", ResolveMode::Full));
        assert_eq!(text_one, text_two);
        assert!(text_one.starts_with("── pkg/b.py::g ──\n"));
    }

    #[test]
    fn rendering_changes_when_a_helper_body_changes() {
        let one = fixture("body_one", &[("pkg/a.py", "def helper():\n    return 1\n\ndef f():\n    return helper()\n")]);
        let two = fixture("body_two", &[("pkg/a.py", "def helper():\n    return 2\n\ndef f():\n    return helper()\n")]);
        let text_one = render::render_symbols(&function_closure(&one, "pkg/a.py", "f", ResolveMode::Full));
        let text_two = render::render_symbols(&function_closure(&two, "pkg/a.py", "f", ResolveMode::Full));
        assert_ne!(digest::closure_digest(&text_one), digest::closure_digest(&text_two));
    }

    #[test]
    fn render_closure_appends_findings_after_the_symbols() {
        let closure = Closure {
            entry: "x.py".to_string(),
            symbols: vec![sym("a.py", "a")],
            findings: vec![Finding::Unreadable { file: "b.py".to_string() }],
        };
        let text = render::render_closure(&closure);
        assert!(text.starts_with("── a.py::a ──\n"));
        assert!(text.contains("── findings ──\nb.py could not be read or parsed"));
        assert_eq!(render::render_closure(&Closure { entry: "x.py".to_string(), symbols: vec![], findings: vec![] }), "");
    }

    #[test]
    fn render_simulate_shows_banners() {
        let closure = Closure { entry: "main.py".to_string(), symbols: vec![], findings: vec![] };
        let output = render::render_simulate(&closure, "print('hello')", ResolveMode::Signatures);
        assert!(output.contains("INJECTION (signatures)"));
        assert!(output.contains("FILE CONTENT"));
        assert!(output.contains("print('hello')"));
    }
}
