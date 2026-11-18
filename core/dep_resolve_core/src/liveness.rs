//! Liveness: the top-level functions and classes of a project that nothing alive reaches.
//!
//! A project is every Python file the caller hands over. A top-level definition is
//! alive when something alive references it. What is alive to begin with:
//!
//!   the module-level code of every file — each top-level statement that is neither
//!     a definition nor an import: a `__main__` guard, `app.command()(run)`;
//!   every decorated definition — its decorator may register it;
//!   every declared entry point — `[project.scripts]` in `pyproject.toml`.
//!
//! References are the names a definition mentions (see `references`), resolved as the
//! resolver resolves them — a definition of the same file, a name imported with
//! `from … import …`, a public name of a `from … import *` module — and `module.name`
//! where `module` is a module of the project. An import alone keeps nothing alive: a
//! name imported only by dead code is dead.
//!
//! No I/O: the caller hands over every file's bytes, and modules are found among them.

use std::collections::{BTreeSet, HashMap};

use tree_sitter::Node;

use crate::references::collect_references;
use crate::ts::{extract_import_bindings, extract_module_info, find_nodes_by_type, node_field, node_text, parse_python};
use crate::SymbolKind;

/// How many modules a re-exported name is followed through before it is given up.
const REEXPORT_HOPS: usize = 8;

/// One file of the project: its path relative to the project root, and its bytes.
pub struct ProjectFile<'a> {
    pub relative_path: &'a str,
    pub source: &'a [u8],
}

/// A declared entry point: the module, and the name in it that is run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPoint {
    pub module: String,
    pub name: String,
}

impl EntryPoint {
    /// An entry point as `[project.scripts]` writes one: `package.module:name`.
    /// `package.module:object.attribute` names the object; its attribute is run on it.
    pub fn parse(target: &str) -> Option<EntryPoint> {
        let (module, object) = target.split_once(':')?;
        let module = module.trim();
        let name = object.split('.').next().unwrap_or("").trim();
        if module.is_empty() || name.is_empty() {
            return None;
        }
        Some(EntryPoint { module: module.to_string(), name: name.to_string() })
    }
}

/// A top-level function or class that nothing alive reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadSymbol {
    pub file: String,
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
}

/// What the liveness examination found: the dead definitions in (file, line) order,
/// and the files that could not be parsed, without which the result is not complete.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Liveness {
    pub dead: Vec<DeadSymbol>,
    pub unparsed: Vec<String>,
}

/// The dead top-level functions and classes of a project.
pub fn dead_symbols(files: &[ProjectFile], entry_points: &[EntryPoint]) -> Liveness {
    let mut unparsed = Vec::new();
    let mut facts = Vec::new();
    for file in files {
        match file_facts(file) {
            Some(found) => facts.push(found),
            None => unparsed.push(file.relative_path.to_string()),
        }
    }
    let project = Project::new(facts);
    let alive = project.alive_from(&project.roots(entry_points));
    Liveness { dead: project.dead(&alive), unparsed }
}

// ── What each file holds ────────────────────────────────────────

/// What a top-level definition is, where, and the names it mentions.
struct Definition {
    name: String,
    kind: SymbolKind,
    line: usize,
    decorated: bool,
    mentions: Mentions,
}

/// The names a piece of code mentions: bare names, and `a.b.c` chains of names.
#[derive(Default)]
struct Mentions {
    names: BTreeSet<String>,
    chains: BTreeSet<String>,
}

/// One file, as the examination needs it.
struct FileFacts {
    relative_path: String,
    module: String,
    package: String,
    definitions: Vec<Definition>,
    /// What the module-level code mentions: the statements that are neither
    /// definitions nor imports.
    module_code: Mentions,
    /// local name → (module, name in it), from `from … import …`.
    imports: HashMap<String, (String, String)>,
    /// Modules imported with `from … import *`.
    star_modules: Vec<String>,
    /// local name → module, from `import a.b as name`.
    module_aliases: HashMap<String, String>,
}

/// A file's module: `src/app/logic/run.py` → `app.logic.run`, `src/app/__init__.py` → `app`.
fn module_name(relative_path: &str) -> String {
    let path = relative_path.strip_prefix("src/").unwrap_or(relative_path);
    let path = path.strip_suffix(".py").unwrap_or(path);
    let path = path.strip_suffix("/__init__").unwrap_or(path);
    path.replace('/', ".")
}

/// The package relative imports are taken from: a package's own for `__init__.py`,
/// the enclosing one for any other module.
fn package_of(relative_path: &str, module: &str) -> String {
    if relative_path.ends_with("__init__.py") {
        return module.to_string();
    }
    module.rsplit_once('.').map_or(String::new(), |(package, _)| package.to_string())
}

/// `module` as an absolute name, for an import `level` dots deep from `package`.
fn absolute_module(package: &str, module: &str, level: usize) -> String {
    if level == 0 {
        return module.to_string();
    }
    let mut base = package.to_string();
    for _ in 1..level {
        base = base.rsplit_once('.').map_or(String::new(), |(parent, _)| parent.to_string());
    }
    match (base.is_empty(), module.is_empty()) {
        (true, _) => module.to_string(),
        (false, true) => base,
        (false, false) => format!("{base}.{module}"),
    }
}

fn file_facts(file: &ProjectFile) -> Option<FileFacts> {
    let tree = parse_python(file.source).ok()?;
    let module = module_name(file.relative_path);
    let mut facts = FileFacts {
        relative_path: file.relative_path.to_string(),
        package: package_of(file.relative_path, &module),
        module,
        definitions: Vec::new(),
        module_code: Mentions::default(),
        imports: HashMap::new(),
        star_modules: Vec::new(),
        module_aliases: HashMap::new(),
    };
    let root = tree.root_node();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        record_statement(&mut facts, statement, file.source);
    }
    Some(facts)
}

/// File one top-level statement: an import into the import tables, a definition
/// among the definitions, anything else into the module-level code.
fn record_statement(facts: &mut FileFacts, statement: Node, source: &[u8]) {
    match statement.kind() {
        "import_from_statement" => record_from_import(facts, statement, source),
        "import_statement" => record_plain_import(facts, statement, source),
        "future_import_statement" | "comment" => {}
        _ => match definition_of(statement, source) {
            Some(definition) => facts.definitions.push(definition),
            None => add_mentions(&mut facts.module_code, statement, source),
        },
    }
}

fn record_from_import(facts: &mut FileFacts, statement: Node, source: &[u8]) {
    let (module, level) = extract_module_info(statement, source);
    let module = absolute_module(&facts.package, &module, level);
    for binding in extract_import_bindings(statement, source) {
        if binding.local == "*" {
            facts.star_modules.push(module.to_string());
        } else {
            facts.imports.insert(binding.local, (module.to_string(), binding.original));
        }
    }
}

/// `import a.b as name` binds `name` to `a.b`; `import a.b` is reached through its
/// full chain, `a.b.attribute`, and needs no binding.
fn record_plain_import(facts: &mut FileFacts, statement: Node, source: &[u8]) {
    let mut cursor = statement.walk();
    for child in statement.named_children(&mut cursor) {
        let alias = node_field(child, "alias").map(|alias| node_text(alias, source).to_string());
        let module = node_field(child, "name").map(|name| node_text(name, source).to_string());
        if let (Some(alias), Some(module)) = (alias, module) {
            facts.module_aliases.insert(alias, module);
        }
    }
}

/// The definition a top-level statement makes, if it makes one.
fn definition_of(statement: Node, source: &[u8]) -> Option<Definition> {
    let decorated = statement.kind() == "decorated_definition";
    let inner = if decorated { node_field(statement, "definition")? } else { statement };
    let (name_node, kind) = match inner.kind() {
        "function_definition" => (node_field(inner, "name")?, SymbolKind::Function),
        "class_definition" => (node_field(inner, "name")?, SymbolKind::Class),
        "type_alias_statement" => (inner.named_child(0)?, SymbolKind::TypeAlias),
        "expression_statement" => (assigned_name(inner)?, SymbolKind::Constant),
        _ => return None,
    };
    let mut mentions = Mentions::default();
    add_mentions(&mut mentions, statement, source);
    Some(Definition {
        name: node_text(name_node, source).to_string(),
        kind,
        line: name_node.start_position().row + 1,
        decorated,
        mentions,
    })
}

/// The name an assignment statement binds: `NAME = …` or `NAME: T = …`.
fn assigned_name(statement: Node) -> Option<Node> {
    let assignment = statement.named_child(0).filter(|child| child.kind() == "assignment")?;
    node_field(assignment, "left").filter(|left| left.kind() == "identifier")
}

/// Add what a piece of code mentions: its names, and its chains of names.
fn add_mentions(mentions: &mut Mentions, node: Node, source: &[u8]) {
    mentions.names.extend(collect_references(node, source));
    for attribute in find_nodes_by_type(node, "attribute") {
        let chain = node_text(attribute, source);
        if is_name_chain(chain) {
            mentions.chains.insert(chain.to_string());
        }
    }
}

/// Whether text is `a.b.c`: names joined by dots, nothing else.
fn is_name_chain(text: &str) -> bool {
    text.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_'))
}

// ── The project ─────────────────────────────────────────────────

/// A definition by its file's index and its name.
type Symbol = (usize, String);

struct Project {
    files: Vec<FileFacts>,
    /// module → file index.
    modules: HashMap<String, usize>,
}

impl Project {
    fn new(files: Vec<FileFacts>) -> Project {
        let modules = files.iter().enumerate().map(|(index, file)| (file.module.to_string(), index)).collect();
        Project { files, modules }
    }

    fn definition(&self, symbol: &Symbol) -> Option<&Definition> {
        self.files[symbol.0].definitions.iter().find(|definition| definition.name == symbol.1)
    }

    fn defines(&self, file: usize, name: &str) -> bool {
        self.definition(&(file, name.to_string())).is_some()
    }

    /// What is alive to begin with: the module-level code, the decorated
    /// definitions and the entry points.
    fn roots(&self, entry_points: &[EntryPoint]) -> Vec<Symbol> {
        let mut roots = Vec::new();
        for (index, file) in self.files.iter().enumerate() {
            roots.extend(self.resolve_mentions(index, &file.module_code));
            let decorated = file.definitions.iter().filter(|definition| definition.decorated);
            roots.extend(decorated.map(|definition| (index, definition.name.to_string())));
        }
        roots.extend(entry_points.iter().filter_map(|entry| self.resolve_in_module(&entry.module, &entry.name, 0)));
        roots
    }

    /// Everything the roots reach.
    fn alive_from(&self, roots: &[Symbol]) -> BTreeSet<Symbol> {
        let mut alive = BTreeSet::new();
        let mut waiting: Vec<Symbol> = roots.to_vec();
        while let Some(symbol) = waiting.pop() {
            if !alive.insert(symbol.clone()) {
                continue;
            }
            if let Some(definition) = self.definition(&symbol) {
                waiting.extend(self.resolve_mentions(symbol.0, &definition.mentions));
            }
        }
        alive
    }

    /// The functions and classes not alive, in (file, line) order.
    fn dead(&self, alive: &BTreeSet<Symbol>) -> Vec<DeadSymbol> {
        let mut dead = Vec::new();
        for (index, file) in self.files.iter().enumerate() {
            let reportable = file.definitions.iter().filter(|definition| is_reportable(definition.kind));
            for definition in reportable.filter(|definition| !alive.contains(&(index, definition.name.to_string()))) {
                dead.push(DeadSymbol {
                    file: file.relative_path.to_string(),
                    name: definition.name.to_string(),
                    kind: definition.kind,
                    line: definition.line,
                });
            }
        }
        dead.sort_by(|left, right| (&left.file, left.line).cmp(&(&right.file, right.line)));
        dead
    }

    /// The definitions some code of a file mentions.
    fn resolve_mentions(&self, file: usize, mentions: &Mentions) -> Vec<Symbol> {
        let by_name = mentions.names.iter().filter_map(|name| self.resolve_name(file, name));
        let by_chain = mentions.chains.iter().filter_map(|chain| self.resolve_chain(file, chain));
        by_name.chain(by_chain).collect()
    }

    /// A bare name: the file's own definition, a name it imports, or a public name
    /// of a module it imports with `*`.
    fn resolve_name(&self, file: usize, name: &str) -> Option<Symbol> {
        if self.defines(file, name) {
            return Some((file, name.to_string()));
        }
        let facts = &self.files[file];
        if let Some((module, original)) = facts.imports.get(name) {
            return self.resolve_in_module(module, original, 0);
        }
        let star_file = facts.star_modules.iter().filter_map(|module| self.modules.get(module)).copied();
        star_file.into_iter().find(|index| self.defines(*index, name)).map(|index| (index, name.to_string()))
    }

    /// `module.name`: the module named through an import alias, an imported module,
    /// or written out whole.
    fn resolve_chain(&self, file: usize, chain: &str) -> Option<Symbol> {
        let (prefix, name) = chain.rsplit_once('.')?;
        let (head, rest) = prefix.split_once('.').unwrap_or((prefix, ""));
        let facts = &self.files[file];
        let head_module = match (facts.module_aliases.get(head), facts.imports.get(head)) {
            (Some(module), _) => module.to_string(),
            (None, Some((module, original))) => format!("{module}.{original}"),
            (None, None) => head.to_string(),
        };
        let module = if rest.is_empty() { head_module } else { format!("{head_module}.{rest}") };
        self.resolve_in_module(&module, name, 0)
    }

    /// `name` in `module`: its definition there, or, when the module only imports
    /// it, the definition it re-exports.
    fn resolve_in_module(&self, module: &str, name: &str, hops: usize) -> Option<Symbol> {
        let index = *self.modules.get(module)?;
        if self.defines(index, name) {
            return Some((index, name.to_string()));
        }
        let (source_module, original) = self.files[index].imports.get(name)?;
        if hops >= REEXPORT_HOPS {
            return None;
        }
        self.resolve_in_module(source_module, original, hops + 1)
    }
}

/// The kinds of definition reported dead: functions and classes.
fn is_reportable(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Function | SymbolKind::Class)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dead_names(files: &[(&str, &str)], entry_points: &[EntryPoint]) -> Vec<String> {
        let project: Vec<ProjectFile> =
            files.iter().map(|(path, source)| ProjectFile { relative_path: path, source: source.as_bytes() }).collect();
        dead_symbols(&project, entry_points).dead.into_iter().map(|dead| format!("{}::{}", dead.file, dead.name)).collect()
    }

    fn main_entry() -> Vec<EntryPoint> {
        vec![EntryPoint { module: "app.cli".to_string(), name: "main".to_string() }]
    }

    const CLI: &str = "from app.logic.run import run\n\ndef main() -> int:\n    return run()\n";

    // ── Dead ────────────────────────────────────────────────────

    #[test]
    fn a_function_nothing_references_is_dead() {
        // The specimen: read_json_document, defined beside the readers and never called.
        let files = [
            ("src/app/cli.py", CLI),
            ("src/app/logic/run.py", "from app.logic.read import read_toml\n\ndef run() -> int:\n    return read_toml()\n"),
            ("src/app/logic/read.py", "def read_toml() -> int:\n    return 1\n\ndef read_json_document() -> int:\n    return 2\n"),
        ];
        assert_eq!(dead_names(&files, &main_entry()), vec!["src/app/logic/read.py::read_json_document"]);
    }

    #[test]
    fn what_only_dead_code_uses_is_dead_too() {
        // The specimen: as_list and reference_name, imported and called by relation_names alone.
        let files = [
            ("src/app/cli.py", CLI),
            ("src/app/logic/run.py", "def run() -> int:\n    return 1\n"),
            (
                "src/app/logic/names.py",
                "from app.logic.parts import as_list, reference_name\n\ndef relation_names(value):\n    return [reference_name(item) for item in as_list(value)]\n",
            ),
            ("src/app/logic/parts.py", "def as_list(value):\n    return [value]\n\ndef reference_name(value):\n    return value\n"),
        ];
        assert_eq!(
            dead_names(&files, &main_entry()),
            vec![
                "src/app/logic/names.py::relation_names",
                "src/app/logic/parts.py::as_list",
                "src/app/logic/parts.py::reference_name",
            ]
        );
    }

    #[test]
    fn a_class_only_dead_code_uses_is_dead() {
        // The specimen: ViewRegistry, ViewEntry and ViewFlavour, reached from read_views alone.
        let files = [
            ("src/app/cli.py", CLI),
            ("src/app/logic/run.py", "def run() -> int:\n    return 1\n"),
            (
                "src/app/logic/views.py",
                "from app.structure.views import ViewRegistry\n\ndef read_views() -> ViewRegistry:\n    return ViewRegistry()\n",
            ),
            (
                "src/app/structure/views.py",
                "class ViewFlavour(StrEnum):\n    plain = \"plain\"\n\nclass ViewEntry(BaseModel):\n    flavour: ViewFlavour\n\nclass ViewRegistry(RootModel[dict[str, ViewEntry]]):\n    pass\n",
            ),
        ];
        assert_eq!(
            dead_names(&files, &main_entry()),
            vec![
                "src/app/logic/views.py::read_views",
                "src/app/structure/views.py::ViewFlavour",
                "src/app/structure/views.py::ViewEntry",
                "src/app/structure/views.py::ViewRegistry",
            ]
        );
    }

    #[test]
    fn an_import_alone_keeps_nothing_alive() {
        let files = [
            ("src/app/cli.py", "from app.logic.run import run, unused\n\ndef main() -> int:\n    return run()\n"),
            ("src/app/logic/run.py", "def run() -> int:\n    return 1\n\ndef unused() -> int:\n    return 2\n"),
        ];
        assert_eq!(dead_names(&files, &main_entry()), vec!["src/app/logic/run.py::unused"]);
    }

    #[test]
    fn with_no_entry_point_an_undecorated_function_nothing_calls_is_dead() {
        let files = [("src/app/cli.py", CLI), ("src/app/logic/run.py", "def run() -> int:\n    return 1\n")];
        assert_eq!(dead_names(&files, &[]), vec!["src/app/cli.py::main", "src/app/logic/run.py::run"]);
    }

    // ── Alive ───────────────────────────────────────────────────

    #[test]
    fn an_entry_point_and_what_it_reaches_are_alive() {
        let files = [
            ("src/app/cli.py", CLI),
            ("src/app/logic/run.py", "from app.logic.step import step\n\ndef run() -> int:\n    return step()\n"),
            ("src/app/logic/step.py", "def step() -> int:\n    return 1\n"),
        ];
        assert!(dead_names(&files, &main_entry()).is_empty());
    }

    #[test]
    fn a_decorated_function_and_what_it_reaches_are_alive() {
        // The yardstick's noise: the functions typer registers in cli.py.
        let files = [
            (
                "src/app/cli.py",
                "import typer\nfrom app.logic.run import run\n\napp = typer.Typer()\n\n@app.command()\ndef release() -> int:\n    return run()\n",
            ),
            ("src/app/logic/run.py", "def run() -> int:\n    return 1\n"),
        ];
        assert!(dead_names(&files, &[]).is_empty());
    }

    #[test]
    fn module_level_code_keeps_what_it_calls_alive() {
        let files = [
            ("src/app/tool.py", "from app.logic.run import run\n\ndef main() -> int:\n    return run()\n\nif __name__ == \"__main__\":\n    main()\n"),
            ("src/app/logic/run.py", "def run() -> int:\n    return 1\n"),
        ];
        assert!(dead_names(&files, &[]).is_empty());
    }

    #[test]
    fn a_function_reached_through_a_dispatch_table_is_alive() {
        let files = [
            ("src/app/cli.py", "from app.logic.route import HANDLERS\n\ndef main(kind) -> int:\n    return HANDLERS[kind]()\n"),
            ("src/app/logic/route.py", "from app.logic.handle import first, second\n\nHANDLERS = {1: first, 2: second}\n"),
            ("src/app/logic/handle.py", "def first() -> int:\n    return 1\n\ndef second() -> int:\n    return 2\n"),
        ];
        assert!(dead_names(&files, &main_entry()).is_empty());
    }

    #[test]
    fn a_function_reached_as_an_attribute_of_its_module_is_alive() {
        for cli in [
            "from app.logic import run\n\ndef main() -> int:\n    return run.run()\n",
            "import app.logic.run as runner\n\ndef main() -> int:\n    return runner.run()\n",
            "import app.logic.run\n\ndef main() -> int:\n    return app.logic.run.run()\n",
        ] {
            let files = [("src/app/cli.py", cli), ("src/app/logic/run.py", "def run() -> int:\n    return 1\n")];
            assert!(dead_names(&files, &main_entry()).is_empty(), "{cli}");
        }
    }

    #[test]
    fn aliased_relative_and_re_exported_imports_are_followed() {
        let files = [
            ("src/app/cli.py", "from .logic.run import run as go\n\ndef main() -> int:\n    return go()\n"),
            ("src/app/logic/run.py", "from app.logic.inner import step\n\ndef run() -> int:\n    return step()\n"),
            ("src/app/logic/inner.py", "from app.logic.deep import step\n"),
            ("src/app/logic/deep.py", "def step() -> int:\n    return 1\n"),
        ];
        assert!(dead_names(&files, &main_entry()).is_empty());
    }

    #[test]
    fn a_function_calling_itself_is_not_kept_alive_by_that() {
        let files = [
            ("src/app/cli.py", CLI),
            ("src/app/logic/run.py", "def run() -> int:\n    return 1\n\ndef walk(node):\n    return walk(node.parent)\n"),
        ];
        assert_eq!(dead_names(&files, &main_entry()), vec!["src/app/logic/run.py::walk"]);
    }

    #[test]
    fn an_entry_point_is_read_as_pyproject_writes_it() {
        assert_eq!(
            EntryPoint::parse("site_tools.cli:main"),
            Some(EntryPoint { module: "site_tools.cli".to_string(), name: "main".to_string() })
        );
        assert_eq!(
            EntryPoint::parse("pkg.app:cli.run"),
            Some(EntryPoint { module: "pkg.app".to_string(), name: "cli".to_string() })
        );
        assert_eq!(EntryPoint::parse("no_colon_here"), None);
    }

    #[test]
    fn a_dead_symbol_carries_its_kind_and_line() {
        let source = "def run() -> int:\n    return 1\n\n\nclass Unused(BaseModel):\n    name: str\n";
        let project = [ProjectFile { relative_path: "src/app/run.py", source: source.as_bytes() }];
        let found = dead_symbols(&project, &[EntryPoint { module: "app.run".to_string(), name: "run".to_string() }]);
        assert_eq!(
            found.dead,
            vec![DeadSymbol { file: "src/app/run.py".to_string(), name: "Unused".to_string(), kind: SymbolKind::Class, line: 5 }]
        );
    }
}
