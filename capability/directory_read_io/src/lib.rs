//! Directory reading — the one directory walker.
//!
//! Lists every entry under a root directory: its path relative to the root, its
//! kind, a file's size, and — when asked — a file's sha256. What to keep from the
//! listing is the caller's to decide; the walk applies no skip list of its own.
//!
//! What a symlink is to the walk is the caller's choice:
//! - **listed** (the default) — a symlink is an entry of kind `symlink` and is never
//!   followed, so the walk stays under the root and always ends.
//! - **followed** — a link is listed as what it leads to. A link to a file is a file,
//!   with the size of its target; a link to a directory is a directory, its entries
//!   listed beneath it under paths through the link, wherever the target is. A link
//!   that leads to nothing, and a link that leads back into a directory the walk is
//!   inside, are failures: every one of them named, and no listing.
//!
//! What a path the walk cannot read means is the caller's choice too:
//! - **strict** (the default) — every unreadable path is a failure, all of them named in
//!   one error; a listing comes back only when the whole tree could be read.
//! - **permitted only** — the listing is what the caller is permitted to see. A
//!   directory it may not look into is listed with `permitted: false` and nothing beneath
//!   it; a path named and then gone is left out and named in `vanished`. Every other
//!   failure — a disk fault, a name that is not UTF-8 — is a failure, as in the strict walk.
//!
//! `read_directory_json` returns the listing as JSON validated against its schema
//! (`schemas/tools/directory-listing.schema.json`, or `directory-permitted-listing` for
//! the permitted walk); `list_directory` returns it typed, for Rust callers.

use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use failure_mode_core::Failure;
use schemas_embedded::{DIRECTORY_LISTING, DIRECTORY_PERMITTED_LISTING};

/// What to list: how deep, whether hidden entries count, whether files are hashed,
/// which directories the walk stays out of, what an unreadable path means, and what a
/// symlink is. The default enters everything, fails on any path it cannot read, and
/// lists a symlink without following it.
#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    /// `None` lists the whole tree; `Some(1)` lists the root's own entries only.
    pub max_depth: Option<u32>,
    /// Entries whose name starts with `.`; a hidden directory is neither listed nor entered without it.
    pub include_hidden: bool,
    /// A sha256 of every file's content.
    pub with_hashes: bool,
    /// Directory names the walk neither lists nor enters, at any depth.
    pub prune_directories: Vec<String>,
    /// The walk neither lists nor enters a directory whose name starts with `.`,
    /// while `include_hidden` still governs hidden files.
    pub prune_hidden_directories: bool,
    /// The listing is what the caller is permitted to see: a path it may not look into is
    /// marked `permitted: false`, a path gone mid-walk is named in `vanished`.
    pub permitted_only: bool,
    /// A symlink is listed as what it leads to — a file, or a directory with its entries
    /// beneath it — and the walk goes wherever the link leads.
    pub follow_symlinks: bool,
}

/// One entry, by its path relative to the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirectoryEntry {
    pub path: String,
    pub kind: EntryKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// `Some(false)` on an entry the caller may not look into, in the permitted walk only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permitted: Option<bool>,
}

/// What an entry is. `Symlink` is a link the walk listed; a walk that follows links
/// lists none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}

/// Every entry under the root, sorted by path; in the permitted walk, also every path that vanished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirectoryListing {
    pub root: String,
    pub entries: Vec<DirectoryEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vanished: Option<Vec<String>>,
}

/// Every failure a directory read can produce. Its class and message come from the error
/// registry (`failure_mode_core`): `error_type()` is the `error.type` a reader returns,
/// the Display string is the `error.message`.
#[derive(Debug)]
pub enum DirectoryReadError {
    RootUnreadable { path: String, message: String },
    FileUnreadable { path: String, message: String },
    /// Every path the walk could not read, at once.
    EntriesUnreadable { root: String, failures: Vec<String> },
    /// Every followed link that leads to nothing, at once.
    LinksBroken { root: String, links: Vec<String> },
    /// Every followed link that leads back into a directory the walk was inside, at
    /// once, each with where it leads back to.
    LinksLooping { root: String, links: Vec<String> },
    /// The listing did not match its own schema — an engine fault.
    ListingInvalid { root: String, message: String },
}

impl DirectoryReadError {
    /// The registry's failure mode for this error, its slots filled.
    pub fn failure(&self) -> Failure {
        match self {
            DirectoryReadError::RootUnreadable { path, message } => failure_mode_core::directory_unlistable(path, message),
            DirectoryReadError::FileUnreadable { path, message } => failure_mode_core::file_unreadable(path, message),
            DirectoryReadError::EntriesUnreadable { root, failures } => {
                failure_mode_core::directory_partly_unreadable(root, failures.len(), failures.join("\n  "))
            }
            DirectoryReadError::LinksBroken { root, links } => {
                failure_mode_core::directory_links_broken(root, links.len(), links.join("\n  "))
            }
            DirectoryReadError::LinksLooping { root, links } => {
                failure_mode_core::directory_links_looping(root, links.len(), links.join("\n  "))
            }
            DirectoryReadError::ListingInvalid { root, message } => failure_mode_core::directory_listing_invalid(root, message),
        }
    }

    /// The failure class a caller branches on — the `error.type` of the reader result.
    pub fn error_type(&self) -> &'static str {
        self.failure().class
    }
}

impl std::fmt::Display for DirectoryReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.failure())
    }
}

impl std::error::Error for DirectoryReadError {}

/// The walk's working state: the root it lists from, what it found, what vanished, what it
/// could not read, and — while it follows links — where it is and which links it refused.
struct Walk<'a> {
    root: &'a Path,
    options: ListOptions,
    entries: Vec<DirectoryEntry>,
    vanished: Vec<String>,
    failures: Vec<String>,
    /// The directories the walk is inside, outermost first, each by its real path.
    /// Kept only while links are followed.
    inside: Vec<PathBuf>,
    broken_links: Vec<String>,
    looping_links: Vec<String>,
}

/// What a path the walk could not read means, under the caller's choice.
#[derive(Debug, PartialEq, Eq)]
enum Unreadable {
    /// The caller may not look into it.
    NotPermitted,
    /// It was named, and is gone.
    Vanished,
    /// Anything else, and every unreadable path in the strict walk.
    Failure,
}

/// What opening one directory came to.
#[derive(Debug, PartialEq, Eq)]
enum Opened {
    Entered,
    NotPermitted,
    Vanished,
    /// A failure was recorded; the directory stays listed.
    Failed,
}

/// What visiting one entry came to: listed (or recorded), or the directory holding it
/// turned out to be one the caller may not look into.
#[derive(Debug, PartialEq, Eq)]
enum Visited {
    Done,
    ParentNotPermitted,
}

/// What hashing one file came to.
enum Hashed {
    Hash(Option<String>),
    NotPermitted,
    Vanished,
}

/// List the tree under `root`, typed. The strict walk fails naming every path it could not read.
pub fn list_directory(root: &str, options: ListOptions) -> Result<DirectoryListing, DirectoryReadError> {
    let root_path = Path::new(root);
    let unreadable = |message: &str| DirectoryReadError::RootUnreadable { path: root.to_string(), message: message.to_string() };
    let metadata = fs::symlink_metadata(root_path).map_err(|io_err| unreadable(&io_err.to_string()))?;
    if !metadata.is_dir() {
        return Err(unreadable("not a directory"));
    }
    let inside = match options.follow_symlinks {
        true => vec![fs::canonicalize(root_path).map_err(|io_err| unreadable(&io_err.to_string()))?],
        false => Vec::new(),
    };
    let mut walk = Walk {
        root: root_path,
        options,
        entries: Vec::new(),
        vanished: Vec::new(),
        failures: Vec::new(),
        inside,
        broken_links: Vec::new(),
        looping_links: Vec::new(),
    };
    match walk_level(&mut walk, root_path, 1) {
        Opened::NotPermitted => return Err(unreadable("permission denied")),
        Opened::Vanished => return Err(unreadable("it vanished while being listed")),
        Opened::Entered | Opened::Failed => {}
    }
    if !walk.failures.is_empty() {
        return Err(DirectoryReadError::EntriesUnreadable { root: root.to_string(), failures: walk.failures });
    }
    if !walk.broken_links.is_empty() {
        walk.broken_links.sort();
        return Err(DirectoryReadError::LinksBroken { root: root.to_string(), links: walk.broken_links });
    }
    if !walk.looping_links.is_empty() {
        walk.looping_links.sort();
        return Err(DirectoryReadError::LinksLooping { root: root.to_string(), links: walk.looping_links });
    }
    walk.entries.sort_by(|left, right| left.path.cmp(&right.path));
    walk.vanished.sort();
    let vanished = walk.options.permitted_only.then_some(walk.vanished);
    Ok(DirectoryListing { root: root.to_string(), entries: walk.entries, vanished })
}

/// List the tree under `root` as JSON validated against its listing schema.
pub fn read_directory_json(root: &str, options: ListOptions) -> Result<String, DirectoryReadError> {
    let schema = if options.permitted_only { &DIRECTORY_PERMITTED_LISTING } else { &DIRECTORY_LISTING };
    let listing = list_directory(root, options)?;
    let invalid = |message: String| DirectoryReadError::ListingInvalid { root: root.to_string(), message };
    let json = serde_json::to_string_pretty(&listing).map_err(|json_err| invalid(json_err.to_string()))?;
    let result = schema.validate(&json).map_err(|schema_err| invalid(schema_err.to_string()))?;
    if !result.valid {
        return Err(invalid(result.message));
    }
    Ok(json)
}

/// In the permitted walk, a denial and a vanishing are part of the listing; every other
/// unreadable path, and every one in the strict walk, is a failure.
fn classify(options: &ListOptions, io_err: &std::io::Error) -> Unreadable {
    if !options.permitted_only {
        return Unreadable::Failure;
    }
    match io_err.kind() {
        ErrorKind::PermissionDenied => Unreadable::NotPermitted,
        ErrorKind::NotFound => Unreadable::Vanished,
        _ => Unreadable::Failure,
    }
}

/// One directory's entries at `depth` (1 = the root's own), descending while depth allows.
/// A directory whose entries cannot be looked at, in the permitted walk, keeps none of them.
fn walk_level(walk: &mut Walk<'_>, directory: &Path, depth: u32) -> Opened {
    let read_dir = match fs::read_dir(directory) {
        Ok(read_dir) => read_dir,
        Err(io_err) => return unopened(walk, directory, &io_err),
    };
    let entries_before = walk.entries.len();
    let vanished_before = walk.vanished.len();
    for dir_entry in read_dir {
        let visited = match dir_entry {
            Ok(dir_entry) => visit_entry(walk, &dir_entry.path(), depth),
            Err(io_err) => {
                walk.failures.push(format!("{}: an entry could not be read: {io_err}", directory.display()));
                Visited::Done
            }
        };
        if visited == Visited::ParentNotPermitted {
            walk.entries.truncate(entries_before);
            walk.vanished.truncate(vanished_before);
            return Opened::NotPermitted;
        }
    }
    Opened::Entered
}

/// A directory that could not be opened, by what that means.
fn unopened(walk: &mut Walk<'_>, directory: &Path, io_err: &std::io::Error) -> Opened {
    match classify(&walk.options, io_err) {
        Unreadable::NotPermitted => Opened::NotPermitted,
        Unreadable::Vanished => Opened::Vanished,
        Unreadable::Failure => {
            walk.failures.push(format!("{}: {io_err}", directory.display()));
            Opened::Failed
        }
    }
}

/// One entry: listed, hashed when asked, and entered when it is a directory within depth.
fn visit_entry(walk: &mut Walk<'_>, path: &Path, depth: u32) -> Visited {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        walk.failures.push(format!("{}: the name is not UTF-8", path.display()));
        return Visited::Done;
    };
    if name.starts_with('.') && !walk.options.include_hidden {
        return Visited::Done;
    }
    let Some(relative) = path.strip_prefix(walk.root).ok().and_then(|relative| relative.to_str()) else {
        walk.failures.push(format!("{}: the path is not UTF-8", path.display()));
        return Visited::Done;
    };
    let relative = relative.to_string();
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(io_err) => return uninspected(walk, path, relative, &io_err),
    };
    if metadata.file_type().is_symlink() {
        match walk.options.follow_symlinks {
            true => visit_followed_link(walk, path, name, relative, depth),
            false => walk.entries.push(entry(relative, EntryKind::Symlink, None, None)),
        }
    } else if metadata.is_dir() {
        visit_directory(walk, path, name, relative, depth);
    } else {
        visit_file(walk, path, relative, metadata.len());
    }
    Visited::Done
}

/// An entry whose kind could not be looked at, by what that means: a denial here says the
/// directory holding it cannot be looked into.
fn uninspected(walk: &mut Walk<'_>, path: &Path, relative: String, io_err: &std::io::Error) -> Visited {
    match classify(&walk.options, io_err) {
        Unreadable::NotPermitted => return Visited::ParentNotPermitted,
        Unreadable::Vanished => walk.vanished.push(relative),
        Unreadable::Failure => walk.failures.push(format!("{}: {io_err}", path.display())),
    }
    Visited::Done
}

/// A link the walk follows: listed as what it leads to. A link that leads to nothing is
/// broken; one that leads to a directory the walk is inside would never end.
fn visit_followed_link(walk: &mut Walk<'_>, path: &Path, name: &str, relative: String, depth: u32) {
    let target = match fs::metadata(path) {
        Ok(target) => target,
        Err(io_err) if io_err.kind() == ErrorKind::NotFound => return walk.broken_links.push(relative),
        Err(io_err) => return walk.failures.push(format!("{}: {io_err}", path.display())),
    };
    if !target.is_dir() {
        return visit_file(walk, path, relative, target.len());
    }
    match fs::canonicalize(path) {
        Ok(real_path) if walk.inside.contains(&real_path) => {
            walk.looping_links.push(format!("{relative} leads back to {}", real_path.display()));
        }
        Ok(_) => visit_directory(walk, path, name, relative, depth),
        Err(io_err) => walk.failures.push(format!("{}: {io_err}", path.display())),
    }
}

/// A directory: left out when pruned; listed, then entered within depth.
fn visit_directory(walk: &mut Walk<'_>, path: &Path, name: &str, relative: String, depth: u32) {
    let pruned = (walk.options.prune_hidden_directories && name.starts_with('.'))
        || walk.options.prune_directories.iter().any(|pruned_name| pruned_name == name);
    if pruned {
        return;
    }
    let index = walk.entries.len();
    walk.entries.push(entry(relative, EntryKind::Directory, None, None));
    if walk.options.max_depth.is_some_and(|max_depth| depth >= max_depth) {
        return;
    }
    if walk.options.follow_symlinks {
        match fs::canonicalize(path) {
            Ok(real_path) => walk.inside.push(real_path),
            Err(io_err) => return walk.failures.push(format!("{}: {io_err}", path.display())),
        }
    }
    let opened = walk_level(walk, path, depth + 1);
    if walk.options.follow_symlinks {
        walk.inside.pop();
    }
    match opened {
        Opened::NotPermitted => walk.entries[index].permitted = Some(false),
        Opened::Vanished => {
            let gone = walk.entries.remove(index);
            walk.vanished.push(gone.path);
        }
        Opened::Entered | Opened::Failed => {}
    }
}

/// A file: listed with its size, and its hash when asked.
fn visit_file(walk: &mut Walk<'_>, path: &Path, relative: String, size: u64) {
    match file_hash(walk, path) {
        Hashed::Hash(sha256) => walk.entries.push(entry(relative, EntryKind::File, Some(size), sha256)),
        Hashed::NotPermitted => {
            let mut not_permitted = entry(relative, EntryKind::File, Some(size), None);
            not_permitted.permitted = Some(false);
            walk.entries.push(not_permitted);
        }
        Hashed::Vanished => walk.vanished.push(relative),
    }
}

fn entry(path: String, kind: EntryKind, size: Option<u64>, sha256: Option<String>) -> DirectoryEntry {
    DirectoryEntry { path, kind, size, sha256, permitted: None }
}

/// The file's sha256 when hashes were asked for; a file that cannot be read, by what that means.
fn file_hash(walk: &mut Walk<'_>, path: &Path) -> Hashed {
    if !walk.options.with_hashes {
        return Hashed::Hash(None);
    }
    match sha256_of(path) {
        Ok(digest) => Hashed::Hash(Some(digest)),
        Err(io_err) => match classify(&walk.options, &io_err) {
            Unreadable::NotPermitted => Hashed::NotPermitted,
            Unreadable::Vanished => Hashed::Vanished,
            Unreadable::Failure => {
                walk.failures.push(format!("{}: cannot be hashed: {io_err}", path.display()));
                Hashed::Hash(None)
            }
        },
    }
}

/// One file's sha256 — the same hash a listing gives — or why the file could not be read.
pub fn file_sha256(path: &str) -> Result<String, DirectoryReadError> {
    sha256_of(Path::new(path))
        .map_err(|io_err| DirectoryReadError::FileUnreadable { path: path.to_string(), message: io_err.to_string() })
}

fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            return Ok(format!("{:x}", hasher.finalize()));
        }
        hasher.update(&buffer[..bytes_read]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::PathBuf;

    fn temp_tree(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nornir_directory_read_io_{test_name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("people/Ada")).unwrap();
        fs::write(dir.join("people/Ada/definition.md"), "abc").unwrap();
        fs::write(dir.join("top.txt"), "").unwrap();
        fs::create_dir_all(dir.join(".hidden")).unwrap();
        fs::write(dir.join(".hidden/secret"), "x").unwrap();
        dir
    }

    fn paths(listing: &DirectoryListing) -> Vec<&str> {
        listing.entries.iter().map(|entry| entry.path.as_str()).collect()
    }

    fn permitted_only() -> ListOptions {
        ListOptions { permitted_only: true, ..ListOptions::default() }
    }

    #[test]
    fn lists_the_whole_tree_sorted_with_kinds_and_sizes() {
        let dir = temp_tree("whole");
        let listing = list_directory(dir.to_str().unwrap(), ListOptions::default()).unwrap();
        assert_eq!(paths(&listing), ["people", "people/Ada", "people/Ada/definition.md", "top.txt"]);
        assert_eq!(listing.entries[0].kind, EntryKind::Directory);
        assert_eq!(listing.entries[2].size, Some(3));
        assert_eq!(listing.entries[3].size, Some(0));
        assert_eq!(listing.vanished, None);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn max_depth_stops_the_descent() {
        let dir = temp_tree("depth");
        let options = ListOptions { max_depth: Some(1), ..ListOptions::default() };
        let listing = list_directory(dir.to_str().unwrap(), options).unwrap();
        assert_eq!(paths(&listing), ["people", "top.txt"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hidden_entries_are_listed_only_when_asked() {
        let dir = temp_tree("hidden");
        let options = ListOptions { include_hidden: true, ..ListOptions::default() };
        let listing = list_directory(dir.to_str().unwrap(), options).unwrap();
        assert!(paths(&listing).contains(&".hidden/secret"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_pruned_directory_is_neither_listed_nor_entered_at_any_depth() {
        let dir = temp_tree("prune_named");
        let options = ListOptions { prune_directories: vec!["Ada".to_string()], ..ListOptions::default() };
        let listing = list_directory(dir.to_str().unwrap(), options).unwrap();
        assert_eq!(paths(&listing), ["people", "top.txt"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hidden_directories_can_be_pruned_while_hidden_files_are_kept() {
        let dir = temp_tree("prune_hidden");
        fs::write(dir.join("people/.Ada.qa"), "{}").unwrap();
        let options = ListOptions { include_hidden: true, prune_hidden_directories: true, ..ListOptions::default() };
        let listing = list_directory(dir.to_str().unwrap(), options).unwrap();
        assert!(paths(&listing).contains(&"people/.Ada.qa"));
        assert!(!paths(&listing).iter().any(|path| path.starts_with(".hidden")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_pruned_directory_that_cannot_be_read_is_no_failure() {
        let dir = temp_tree("prune_unreadable");
        let locked = dir.join("people/Ada");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let options = ListOptions { prune_directories: vec!["Ada".to_string()], ..ListOptions::default() };
        let listing = list_directory(dir.to_str().unwrap(), options);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(paths(&listing.unwrap()), ["people", "top.txt"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_symlink_is_listed_and_never_followed() {
        let dir = temp_tree("symlink");
        symlink(&dir, dir.join("loop")).unwrap();
        let listing = list_directory(dir.to_str().unwrap(), ListOptions::default()).unwrap();
        let link = listing.entries.iter().find(|entry| entry.path == "loop").unwrap();
        assert_eq!(link.kind, EntryKind::Symlink);
        assert!(!paths(&listing).iter().any(|path| path.starts_with("loop/")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hashes_are_the_sha256_of_each_file_when_asked() {
        let dir = temp_tree("hashes");
        let options = ListOptions { with_hashes: true, ..ListOptions::default() };
        let listing = list_directory(dir.to_str().unwrap(), options).unwrap();
        let file = listing.entries.iter().find(|entry| entry.path == "top.txt").unwrap();
        assert_eq!(file.sha256.as_deref(), Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
        assert!(listing.entries.iter().find(|entry| entry.path == "people").unwrap().sha256.is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unreadable_directory_is_a_read_error_naming_it() {
        let dir = temp_tree("unreadable");
        let locked = dir.join("people/Ada");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let error = list_directory(dir.to_str().unwrap(), ListOptions::default()).unwrap_err();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(error.error_type(), "read_error");
        assert!(error.to_string().contains("people/Ada"), "{error}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_root_and_a_file_root_are_read_errors() {
        let dir = temp_tree("bad_root");
        for root in [dir.join("absent"), dir.join("top.txt")] {
            let error = list_directory(root.to_str().unwrap(), ListOptions::default()).unwrap_err();
            assert_eq!(error.error_type(), "read_error");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_json_matches_its_schema() {
        let dir = temp_tree("json");
        let options = ListOptions { with_hashes: true, ..ListOptions::default() };
        let json = read_directory_json(dir.to_str().unwrap(), options).unwrap();
        assert!(DIRECTORY_LISTING.is_valid(&json).unwrap());
        assert!(json.contains("\"kind\": \"directory\""), "{json}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_locked_directory_is_listed_not_permitted_with_nothing_beneath_it() {
        let dir = temp_tree("permitted_locked");
        let locked = dir.join("people/Ada");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let listing = list_directory(dir.to_str().unwrap(), permitted_only());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let listing = listing.unwrap();
        assert_eq!(paths(&listing), ["people", "people/Ada", "top.txt"]);
        let ada = listing.entries.iter().find(|entry| entry.path == "people/Ada").unwrap();
        assert_eq!(ada.permitted, Some(false));
        assert_eq!(listing.vanished, Some(vec![]));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_directory_whose_entries_cannot_be_looked_at_keeps_none_of_them() {
        let dir = temp_tree("permitted_no_search");
        let readable_only = dir.join("people");
        fs::set_permissions(&readable_only, fs::Permissions::from_mode(0o444)).unwrap();
        let listing = list_directory(dir.to_str().unwrap(), permitted_only());
        fs::set_permissions(&readable_only, fs::Permissions::from_mode(0o755)).unwrap();
        let listing = listing.unwrap();
        assert_eq!(paths(&listing), ["people", "top.txt"]);
        assert_eq!(listing.entries[0].permitted, Some(false));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_root_the_caller_may_not_enter_is_a_read_error_in_the_permitted_walk() {
        let dir = temp_tree("permitted_root");
        let locked = dir.join("people/Ada");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let error = list_directory(locked.to_str().unwrap(), permitted_only());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(error.unwrap_err().error_type(), "read_error");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn only_the_permitted_walk_reads_a_denial_or_a_vanishing_as_part_of_the_listing() {
        let denied = std::io::Error::from(ErrorKind::PermissionDenied);
        let gone = std::io::Error::from(ErrorKind::NotFound);
        let fault = std::io::Error::from(ErrorKind::Other);
        assert_eq!(classify(&permitted_only(), &denied), Unreadable::NotPermitted);
        assert_eq!(classify(&permitted_only(), &gone), Unreadable::Vanished);
        assert_eq!(classify(&permitted_only(), &fault), Unreadable::Failure);
        for io_err in [&denied, &gone, &fault] {
            assert_eq!(classify(&ListOptions::default(), io_err), Unreadable::Failure);
        }
    }

    // ── Links followed ───────────────────────────────────────────

    fn followed() -> ListOptions {
        ListOptions { follow_symlinks: true, ..ListOptions::default() }
    }

    /// A project whose `atom` level is a link into a core tree beside it, outside the root.
    fn project_linked_into_core(test_name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("nornir_directory_read_io_{test_name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("core/source/atom")).unwrap();
        fs::write(dir.join("core/source/atom/path_absolute.yaml"), "kind: atom").unwrap();
        let project = dir.join("project/source");
        fs::create_dir_all(project.join("section")).unwrap();
        fs::write(project.join("section/identity.yaml"), "x").unwrap();
        symlink("../../core/source/atom", project.join("atom")).unwrap();
        (dir, project)
    }

    #[test]
    fn a_followed_link_to_a_directory_is_a_directory_with_its_entries_beneath_it() {
        let (dir, project) = project_linked_into_core("followed_directory");
        let listing = list_directory(project.to_str().unwrap(), followed()).unwrap();
        assert_eq!(paths(&listing), ["atom", "atom/path_absolute.yaml", "section", "section/identity.yaml"]);
        assert_eq!(listing.entries[0].kind, EntryKind::Directory);
        assert_eq!(listing.entries[1].kind, EntryKind::File);
        assert_eq!(listing.entries[1].size, Some(10));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_followed_link_to_a_file_is_a_file_with_the_size_of_its_target() {
        let (dir, project) = project_linked_into_core("followed_file");
        symlink("../../core/source/atom/path_absolute.yaml", project.join("shared.yaml")).unwrap();
        let listing = list_directory(project.to_str().unwrap(), followed()).unwrap();
        let shared = listing.entries.iter().find(|entry| entry.path == "shared.yaml").unwrap();
        assert_eq!(shared.kind, EntryKind::File);
        assert_eq!(shared.size, Some(10));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_walk_that_follows_links_lists_no_symlink() {
        let (dir, project) = project_linked_into_core("followed_no_symlink");
        symlink("../../core/source/atom/path_absolute.yaml", project.join("shared.yaml")).unwrap();
        let listing = list_directory(project.to_str().unwrap(), followed()).unwrap();
        assert!(listing.entries.iter().all(|entry| entry.kind != EntryKind::Symlink));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_same_tree_without_following_lists_the_link_and_nothing_beneath_it() {
        let (dir, project) = project_linked_into_core("not_followed");
        let listing = list_directory(project.to_str().unwrap(), ListOptions::default()).unwrap();
        assert_eq!(paths(&listing), ["atom", "section", "section/identity.yaml"]);
        assert_eq!(listing.entries[0].kind, EntryKind::Symlink);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_links_to_one_directory_list_it_under_each() {
        let (dir, project) = project_linked_into_core("followed_twice");
        symlink("../../core/source/atom", project.join("atom_again")).unwrap();
        let listing = list_directory(project.to_str().unwrap(), followed()).unwrap();
        assert!(paths(&listing).contains(&"atom/path_absolute.yaml"));
        assert!(paths(&listing).contains(&"atom_again/path_absolute.yaml"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_hidden_link_is_left_out_like_any_hidden_entry() {
        let (dir, project) = project_linked_into_core("followed_hidden");
        symlink("../../core/source/atom", project.join(".atom")).unwrap();
        let listing = list_directory(project.to_str().unwrap(), followed()).unwrap();
        assert!(!paths(&listing).iter().any(|path| path.starts_with(".atom")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_followed_link_that_leads_to_nothing_is_refused_by_name() {
        let (dir, project) = project_linked_into_core("followed_broken");
        symlink("../../core/source/absent", project.join("enum")).unwrap();
        symlink("nowhere.yaml", project.join("section/lost.yaml")).unwrap();
        let error = list_directory(project.to_str().unwrap(), followed()).unwrap_err();
        assert_eq!(error.failure().mode, "directory_links_broken");
        assert_eq!(error.error_type(), "read_error");
        let message = error.to_string();
        assert!(message.contains("2 link(s)"), "{message}");
        assert!(message.contains("\n  enum\n  section/lost.yaml"), "{message}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_followed_link_that_leads_back_into_the_walk_is_refused_with_where_it_leads() {
        let (dir, project) = project_linked_into_core("followed_loop");
        symlink("..", project.join("section/up")).unwrap();
        let error = list_directory(project.to_str().unwrap(), followed()).unwrap_err();
        assert_eq!(error.failure().mode, "directory_links_looping");
        assert_eq!(error.error_type(), "validation_error");
        let leads_back_to = fs::canonicalize(&project).unwrap();
        let message = error.to_string();
        assert!(message.contains(&format!("section/up leads back to {}", leads_back_to.display())), "{message}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_loop_inside_a_followed_directory_is_refused_too() {
        let (dir, project) = project_linked_into_core("followed_loop_outside");
        symlink(".", dir.join("core/source/atom/self")).unwrap();
        let error = list_directory(project.to_str().unwrap(), followed()).unwrap_err();
        assert_eq!(error.failure().mode, "directory_links_looping");
        assert!(error.to_string().contains("atom/self leads back to"), "{error}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_root_that_is_a_link_is_not_a_directory_to_list() {
        let (dir, project) = project_linked_into_core("followed_root_link");
        let error = list_directory(project.join("atom").to_str().unwrap(), followed()).unwrap_err();
        assert_eq!(error.failure().mode, "directory_unlistable");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_followed_json_matches_the_listing_schema() {
        let (dir, project) = project_linked_into_core("followed_json");
        let json = read_directory_json(project.to_str().unwrap(), followed()).unwrap();
        assert!(DIRECTORY_LISTING.is_valid(&json).unwrap());
        assert!(!json.contains("symlink"), "{json}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_permitted_json_matches_its_schema() {
        let dir = temp_tree("permitted_json");
        let json = read_directory_json(dir.to_str().unwrap(), permitted_only()).unwrap();
        assert!(DIRECTORY_PERMITTED_LISTING.is_valid(&json).unwrap());
        assert!(json.contains("\"vanished\": []"), "{json}");
        fs::remove_dir_all(&dir).ok();
    }
}
