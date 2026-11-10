//! Canonical ordering of a closure: dependencies before dependents, ties by key.
//!
//! Kahn's algorithm with a min-heap on `file::name` yields the lexicographically
//! smallest topological order, so the same graph always gives the same sequence
//! however it was discovered. Symbols caught in a cycle cannot be ordered; they
//! are reported as findings and placed by key so that output is still produced.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};

use crate::{Finding, ResolvedSymbol};

/// Order `symbols` so that every symbol follows the symbols it references.
///
/// `edges` are `(from, to)` keys meaning "from references to"; edges to keys
/// outside `symbols`, and self-edges, are ignored. Returns the ordered symbols
/// and one `Finding::Cycle` per group of mutually dependent symbols.
pub fn canonical_order(
    symbols: Vec<ResolvedSymbol>,
    edges: &[(String, String)],
) -> (Vec<ResolvedSymbol>, Vec<Finding>) {
    let mut by_key: BTreeMap<String, ResolvedSymbol> =
        symbols.into_iter().map(|s| (s.key(), s)).collect();
    let graph = Graph::new(by_key.keys().cloned().collect(), edges);

    let placed = graph.kahn_order();
    let findings = graph.cycles(&placed);

    let mut ordered = Vec::with_capacity(by_key.len());
    for key in placed {
        if let Some(symbol) = by_key.remove(&key) {
            ordered.push(symbol);
        }
    }
    (ordered, findings)
}

struct Graph {
    nodes: BTreeSet<String>,
    /// key → keys it references (within the closure, excluding itself)
    references: BTreeMap<String, BTreeSet<String>>,
    /// key → keys that reference it
    referenced_by: BTreeMap<String, BTreeSet<String>>,
}

impl Graph {
    fn new(nodes: BTreeSet<String>, edges: &[(String, String)]) -> Graph {
        let mut references: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut referenced_by: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (from, to) in edges {
            if from == to || !nodes.contains(from) || !nodes.contains(to) {
                continue;
            }
            references.entry(from.clone()).or_default().insert(to.clone());
            referenced_by.entry(to.clone()).or_default().insert(from.clone());
        }
        Graph { nodes, references, referenced_by }
    }

    fn out_degree(&self, key: &str) -> usize {
        self.references.get(key).map_or(0, BTreeSet::len)
    }

    /// Kahn's algorithm, smallest ready key first. When nothing is ready but
    /// nodes remain (a cycle), the cycle containing the smallest remaining
    /// key is placed as a unit, its members by key.
    fn kahn_order(&self) -> Vec<String> {
        let mut pending: BTreeMap<&str, usize> =
            self.nodes.iter().map(|k| (k.as_str(), self.out_degree(k))).collect();
        let mut ready: BinaryHeap<Reverse<&str>> = pending
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(key, _)| Reverse(*key))
            .collect();
        let mut forced: VecDeque<&str> = VecDeque::new();
        let mut placed: Vec<String> = Vec::with_capacity(self.nodes.len());

        loop {
            let next = if let Some(key) = forced.pop_front() {
                key
            } else if let Some(Reverse(key)) = ready.pop() {
                key
            } else if let Some(key) = pending.keys().next() {
                forced.extend(self.mutual_closure(key));
                continue;
            } else {
                break;
            };
            if pending.remove(next).is_none() {
                continue;
            }
            placed.push(next.to_string());
            for dependent in self.referenced_by.get(next).into_iter().flatten() {
                if let Some(degree) = pending.get_mut(dependent.as_str()) {
                    *degree = degree.saturating_sub(1);
                    if *degree == 0 {
                        ready.push(Reverse(dependent.as_str()));
                    }
                }
            }
        }
        placed
    }

    /// One finding per strongly connected component with more than one member.
    fn cycles(&self, placed: &[String]) -> Vec<Finding> {
        let mut findings = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for key in placed {
            if seen.contains(key.as_str()) {
                continue;
            }
            let members = self.mutual_closure(key);
            if members.len() > 1 {
                for member in &members {
                    seen.insert(member);
                }
                findings.push(Finding::Cycle {
                    members: members.iter().map(|m| m.to_string()).collect(),
                });
            }
        }
        findings
    }

    /// `start` together with every key that both reaches it and is reached
    /// from it (its strongly connected component), by key.
    fn mutual_closure<'a>(&'a self, start: &'a str) -> BTreeSet<&'a str> {
        let forward = self.reachable(start, &self.references);
        let backward = self.reachable(start, &self.referenced_by);
        let mut members: BTreeSet<&str> = forward.intersection(&backward).copied().collect();
        members.insert(start);
        members
    }

    fn reachable<'a>(
        &'a self,
        start: &'a str,
        adjacency: &'a BTreeMap<String, BTreeSet<String>>,
    ) -> BTreeSet<&'a str> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut stack: Vec<&str> = vec![start];
        while let Some(key) = stack.pop() {
            for next in adjacency.get(key).into_iter().flatten() {
                if seen.insert(next.as_str()) {
                    stack.push(next.as_str());
                }
            }
        }
        seen
    }
}
