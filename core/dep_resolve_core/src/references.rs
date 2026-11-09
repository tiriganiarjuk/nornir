//! Reference collection: the names a definition mentions.
//!
//! A reference is any `identifier` inside a definition node — decorators,
//! parameter annotations and defaults, return type, body. Two positions are
//! excluded by syntax because they never name a module-level symbol:
//!
//!   the attribute side of `obj.attr`   (`attr` is looked up on `obj`)
//!   the name of a keyword argument     (`name` in `f(name=value)`)
//!
//! Nothing is excluded by scope: parameters, locals and the definition's own
//! name are all returned. The caller matches the names against what the file
//! defines and imports, so a local that shadows a module-level name pulls
//! that symbol in. The collector over-includes; it never omits.

use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::ts::node_text;

/// Collect every name mentioned inside `definition`, sorted and deduplicated.
///
/// Pass the outer `decorated_definition` node when there is one, so that the
/// decorators are part of what is searched.
pub fn collect_references(definition: Node, source: &[u8]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_into(definition, source, &mut names);
    names
}

fn collect_into(node: Node, source: &[u8], names: &mut BTreeSet<String>) {
    if node.kind() == "identifier" {
        if !is_excluded_position(node) {
            names.insert(node_text(node, source).to_string());
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_into(child, source, names);
    }
}

/// True when the identifier sits where it cannot name a module-level symbol.
fn is_excluded_position(identifier: Node) -> bool {
    let Some(parent) = identifier.parent() else {
        return false;
    };
    let excluded_field = match parent.kind() {
        "attribute" => "attribute",
        "keyword_argument" => "name",
        _ => return false,
    };
    parent
        .child_by_field_name(excluded_field)
        .is_some_and(|field_node| field_node.id() == identifier.id())
}
