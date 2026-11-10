//! The symbol-level engine: from an entry, follow references until nothing
//! project-local is left, then order the result canonically.
//!
//! A reference inside a definition resolves, in this order, to
//!   a top-level definition in the same file,
//!   a name the file imports with `from … import …` (alias-aware),
//!   a public name of a module the file imports with `from … import *`.
//! Anything else — builtins, locals, third-party names — is not followed.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use tree_sitter::Tree;

use crate::extract::{extract_all_top_level_names, extract_symbols, find_top_level};
use crate::order::canonical_order;
use crate::references::collect_references;
use crate::ts::{
    extract_import_bindings, extract_module_info, extract_plain_import_modules,
    find_nodes_by_type, parse_python,
};
use crate::{Closure, Finding, ResolveError, ResolveMode, ResolvedSymbol};

/// Configuration for dependency resolution.
pub struct ResolveConfig {
    /// What to render: signatures, hybrid or full bodies.
    pub mode: ResolveMode,
    /// Reference hops followed from the entry. Symbols at this depth are
    /// included; their references are not followed.
    pub max_depth: usize,
}

/// The file an entry starts from, as the caller already holds it.
pub struct Target<'a> {
    /// Absolute path of the file.
    pub file_path: &'a str,
    /// Its bytes.
    pub source: &'a [u8],
    /// Absolute path of the project root that module paths resolve under.
    pub project_root: &'a str,
}

/// Where a name imported into a file comes from.
#[derive(Debug, Clone)]
struct ImportTarget {
    file: PathBuf,
    name: String,
}

/// A parsed file and its import table, loaded once per resolution.
struct LoadedFile {
    relative_path: String,
    source: Vec<u8>,
    tree: Tree,
    /// local name → (file, original name), for `from … import …`
    imports: HashMap<String, ImportTarget>,
    /// files imported with `from … import *`, in source order
    star_imports: Vec<PathBuf>,
}

impl LoadedFile {
    fn defines(&self, name: &str) -> bool {
        find_top_level(&self.tree, &self.source, name).is_some()
    }

    fn public_names(&self) -> Vec<String> {
        extract_all_top_level_names(&self.source, &self.tree)
    }
}

/// A reference resolved to the symbol it names.
struct Reference {
    file: PathBuf,
    name: String,
}

struct Engine<'a, F: Fn(&str) -> Option<Vec<u8>>> {
    project_root: &'a Path,
    read_file: &'a F,
    config: &'a ResolveConfig,
    /// Loaded files; `None` records a file that could not be read or parsed.
    files: HashMap<PathBuf, Option<Rc<LoadedFile>>>,
    visited: BTreeSet<(PathBuf, String)>,
    symbols: Vec<ResolvedSymbol>,
    edges: Vec<(String, String)>,
    findings: Vec<Finding>,
}

/// Resolve the closure of one top-level definition: the definition itself
/// (depth 0), what it references (depth 1), and so on.
pub fn resolve_function<F>(
    target: &Target,
    name: &str,
    read_file: F,
    config: &ResolveConfig,
) -> Result<Closure, ResolveError>
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    let mut engine = Engine::new(Path::new(target.project_root), &read_file, config);
    let target_path = PathBuf::from(target.file_path);
    let loaded = engine.load_entry(&target_path, target.source)?;
    if !loaded.defines(name) {
        return Err(ResolveError::NotTopLevel {
            file: loaded.relative_path.to_string(),
            name: name.to_string(),
        });
    }
    engine.walk_symbol(&target_path, name, 0);
    Ok(engine.finish(format!("{}::{name}", loaded.relative_path)))
}

/// Resolve the closure of a file's imports: every name it imports (depth 1),
/// what those reference (depth 2), and so on. The file's own definitions are
/// not part of the result.
pub fn resolve_file<F>(
    target: &Target,
    read_file: F,
    config: &ResolveConfig,
) -> Result<Closure, ResolveError>
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    let mut engine = Engine::new(Path::new(target.project_root), &read_file, config);
    let target_path = PathBuf::from(target.file_path);
    let loaded = engine.load_entry(&target_path, target.source)?;
    let mut entries: Vec<(&Path, &str)> = loaded
        .imports
        .values()
        .map(|import| (import.file.as_path(), import.name.as_str()))
        .collect();
    let star_names: Vec<(&Path, String)> = loaded
        .star_imports
        .iter()
        .filter_map(|star_file| engine.load(star_file).map(|star| (star_file.as_path(), star)))
        .flat_map(|(star_file, star)| star.public_names().into_iter().map(move |n| (star_file, n)))
        .collect();
    entries.extend(star_names.iter().map(|(file, name)| (*file, name.as_str())));
    entries.sort();
    for (file, name) in entries {
        engine.walk_symbol(file, name, 1);
    }
    Ok(engine.finish(loaded.relative_path.to_string()))
}

impl<'a, F: Fn(&str) -> Option<Vec<u8>>> Engine<'a, F> {
    fn new(project_root: &'a Path, read_file: &'a F, config: &'a ResolveConfig) -> Self {
        Engine {
            project_root,
            read_file,
            config,
            files: HashMap::new(),
            visited: BTreeSet::new(),
            symbols: Vec::new(),
            edges: Vec::new(),
            findings: Vec::new(),
        }
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(self.project_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string())
    }

    /// Load the entry file from bytes the caller already has.
    fn load_entry(&mut self, path: &Path, source: &[u8]) -> Result<Rc<LoadedFile>, ResolveError> {
        let loaded = self
            .parse_loaded(path, source.to_vec())
            .ok_or_else(|| ResolveError::Parse { file: self.relative(path) })?;
        let loaded = Rc::new(loaded);
        self.files.insert(path.to_path_buf(), Some(Rc::clone(&loaded)));
        Ok(loaded)
    }

    /// Load a dependency file through the callback; `None` (once, with a
    /// finding) when it cannot be read or parsed.
    fn load(&mut self, path: &Path) -> Option<Rc<LoadedFile>> {
        if !self.files.contains_key(path) {
            let bytes = (self.read_file)(&path.to_string_lossy());
            let loaded = bytes.and_then(|b| self.parse_loaded(path, b)).map(Rc::new);
            if loaded.is_none() {
                self.findings.push(Finding::Unreadable { file: self.relative(path) });
            }
            self.files.insert(path.to_path_buf(), loaded);
        }
        self.files.get(path).and_then(|entry| entry.as_ref().map(Rc::clone))
    }

    fn parse_loaded(&mut self, path: &Path, source: Vec<u8>) -> Option<LoadedFile> {
        let tree = parse_python(&source).ok()?;
        let relative_path = self.relative(path);
        let (imports, star_imports) = self.import_table(path, &source, &tree);
        self.note_plain_project_imports(&relative_path, &source, &tree);
        Some(LoadedFile { relative_path, source, tree, imports, star_imports })
    }

    fn import_table(
        &self,
        file_path: &Path,
        source: &[u8],
        tree: &Tree,
    ) -> (HashMap<String, ImportTarget>, Vec<PathBuf>) {
        let mut imports = HashMap::new();
        let mut star_imports = Vec::new();
        for node in find_nodes_by_type(tree.root_node(), "import_from_statement") {
            let (module_path, level) = extract_module_info(node, source);
            let dotted = if level > 0 {
                resolve_relative_module(file_path, &module_path, level)
            } else {
                module_path
            };
            let Some(file) = resolve_module_to_file(&dotted, self.project_root) else {
                continue;
            };
            for binding in extract_import_bindings(node, source) {
                if binding.local == "*" {
                    star_imports.push(file.to_path_buf());
                } else {
                    let import = ImportTarget { file: file.to_path_buf(), name: binding.original };
                    imports.insert(binding.local, import);
                }
            }
        }
        (imports, star_imports)
    }

    fn note_plain_project_imports(&mut self, relative_path: &str, source: &[u8], tree: &Tree) {
        for node in find_nodes_by_type(tree.root_node(), "import_statement") {
            for module in extract_plain_import_modules(node, source) {
                if resolve_module_to_file(&module, self.project_root).is_some() {
                    self.findings.push(Finding::PlainProjectImport {
                        file: relative_path.to_string(),
                        line: node.start_position().row + 1,
                        module,
                    });
                }
            }
        }
    }

    /// Record `name` from `file` at `depth`, then follow its references.
    fn walk_symbol(&mut self, file: &Path, name: &str, depth: usize) {
        if !self.visited.insert((file.to_path_buf(), name.to_string())) {
            return;
        }
        let Some(loaded) = self.load(file) else {
            return;
        };
        if !loaded.defines(name) {
            self.follow_reexport_or_report(&loaded, name, depth);
            return;
        }
        let mode = self.config.mode.at_depth(depth);
        let mut definitions = extract_symbols(&loaded.source, &loaded.tree, &[name.to_string()], mode);
        let Some(definition) = definitions.pop() else {
            self.findings.push(Finding::Unresolved {
                file: loaded.relative_path.to_string(),
                name: name.to_string(),
            });
            return;
        };
        let symbol = ResolvedSymbol {
            file: loaded.relative_path.to_string(),
            name: name.to_string(),
            kind: definition.kind,
            depth,
            rendered: definition.rendered,
        };
        let key = symbol.key();
        self.symbols.push(symbol);
        if depth >= self.config.max_depth {
            return;
        }
        for reference in self.references_of(&loaded, name) {
            if reference.file == file && reference.name == name {
                continue;
            }
            self.walk_symbol(&reference.file, &reference.name, depth + 1);
            let referenced_key = format!("{}::{}", self.relative(&reference.file), reference.name);
            self.edges.push((key.to_string(), referenced_key));
        }
    }

    /// A name imported from a file that does not define it: follow it if the
    /// file imports it in turn, otherwise report it.
    fn follow_reexport_or_report(&mut self, loaded: &LoadedFile, name: &str, depth: usize) {
        match loaded.imports.get(name) {
            Some(import) => self.walk_symbol(&import.file, &import.name, depth),
            None => self.findings.push(Finding::Unresolved {
                file: loaded.relative_path.to_string(),
                name: name.to_string(),
            }),
        }
    }

    /// The project-local symbols the definition of `name` references, in a
    /// deterministic order.
    fn references_of(&mut self, loaded: &LoadedFile, name: &str) -> Vec<Reference> {
        let Some(node) = find_top_level(&loaded.tree, &loaded.source, name) else {
            return Vec::new();
        };
        let own_file = self.project_root.join(&loaded.relative_path);
        let mut references = Vec::new();
        for referenced in collect_references(node, &loaded.source) {
            if loaded.defines(&referenced) {
                references.push(Reference { file: own_file.to_path_buf(), name: referenced });
            } else if let Some(import) = loaded.imports.get(&referenced) {
                references.push(Reference { file: import.file.to_path_buf(), name: import.name.to_string() });
            } else if let Some(star_file) = self.star_source_of(loaded, &referenced) {
                references.push(Reference { file: star_file, name: referenced });
            }
        }
        references
    }

    /// The first `from … import *` module of `loaded` that defines `name`.
    fn star_source_of(&mut self, loaded: &LoadedFile, name: &str) -> Option<PathBuf> {
        loaded
            .star_imports
            .iter()
            .find(|star_file| self.load(star_file).is_some_and(|star| star.defines(name)))
            .map(|star_file| star_file.to_path_buf())
    }

    fn finish(self, entry: String) -> Closure {
        let (symbols, mut cycle_findings) = canonical_order(self.symbols, &self.edges);
        let mut findings = self.findings;
        findings.append(&mut cycle_findings);
        findings.sort_by_key(ToString::to_string);
        findings.dedup();
        Closure { entry, symbols, findings }
    }
}

// ── Module path → file path resolution ──────────────────────────

fn resolve_module_to_file(dotted: &str, project_root: &Path) -> Option<PathBuf> {
    let parts: Vec<&str> = dotted.split('.').collect();

    // Try progressively shorter prefixes as file paths
    for split in (1..=parts.len()).rev() {
        let rel_path: PathBuf = parts[..split].iter().collect();

        let candidate = project_root.join(&rel_path).with_extension("py");
        if candidate.is_file() {
            return Some(candidate);
        }

        let candidate = project_root.join(&rel_path).join("__init__.py");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // Also try with src/ prefix (common layout)
    for split in (1..=parts.len()).rev() {
        let rel_path: PathBuf = parts[..split].iter().collect();

        let candidate = project_root.join("src").join(&rel_path).with_extension("py");
        if candidate.is_file() {
            return Some(candidate);
        }

        let candidate = project_root
            .join("src")
            .join(&rel_path)
            .join("__init__.py");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn resolve_relative_module(file_path: &Path, module: &str, level: usize) -> String {
    let parent = file_path.parent().unwrap_or(Path::new(""));

    // Walk up `level` directories (level=1 means current package)
    let mut base = parent.to_path_buf();
    for _ in 1..level {
        base = base.parent().unwrap_or(Path::new("")).to_path_buf();
    }

    let base_dotted = base
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(".");

    if module.is_empty() {
        base_dotted
    } else if base_dotted.is_empty() {
        module.to_string()
    } else {
        format!("{base_dotted}.{module}")
    }
}
