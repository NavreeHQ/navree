//! Which configuration keys were read, and which were not.
//!
//! A key that nothing reads is nearly always a typo (`databse`,
//! `max_conections`). [`Config`](crate::Config) notes every read: a key read
//! with `get_string` or `get_bool`, and a section read into a type, with the
//! keys that type has no field for. [`Config::unused`](crate::Config::unused)
//! sets the notes against the tree.
//!
//! A key counts as read when any read covers it, so two types that read
//! different fields of one table (`database` and `database.mongodb`) do not
//! report each other's keys.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::{Mutex, PoisonError};

use crate::de::Sink;
use crate::origin::ValueOrigin;
use crate::value::Value;

/// What the reads of a [`Config`](crate::Config) have covered so far. Shared
/// by every clone.
#[derive(Default)]
pub(crate) struct Usage {
    keys: Mutex<BTreeSet<String>>,
    sections: Mutex<Vec<SectionRead>>,
}

/// A section deserialized into a type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SectionRead {
    /// The dotted key of the section.
    pub(crate) key: String,
    /// Keys (full dotted paths) the type has no field for.
    pub(crate) unknown: Vec<String>,
    /// The fields each struct declares, by the path of its table.
    pub(crate) tables: Vec<(String, &'static [&'static str])>,
}

impl Usage {
    /// Notes the read of the scalar (or list) at `key`.
    pub(crate) fn key(&self, key: &str) {
        let mut keys = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        if !keys.contains(key) {
            keys.insert(key.to_owned());
        }
    }

    /// Notes the read of the section at `key`.
    pub(crate) fn section(&self, key: &str, sink: Sink) {
        let mut unknown = sink.unknown.into_inner();
        unknown.sort();
        unknown.dedup();
        let read = SectionRead {
            key: key.to_owned(),
            unknown,
            tables: sink.tables.into_inner(),
        };
        let mut sections = self.sections.lock().unwrap_or_else(PoisonError::into_inner);
        if !sections.contains(&read) {
            sections.push(read);
        }
    }

    /// The notes so far.
    fn snapshot(&self) -> (BTreeSet<String>, Vec<SectionRead>) {
        let keys = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        let sections = self.sections.lock().unwrap_or_else(PoisonError::into_inner);
        (keys.clone(), sections.clone())
    }
}

/// A key or section in the configuration that nothing reads.
///
/// ```
/// use navree_config::{Config, ConfigSources};
///
/// let config = Config::load(&ConfigSources::new().set("databse.url", "sqlite://app.db"))?;
/// let unused = config.unused();
/// assert_eq!(unused[0].key, "databse");
/// assert!(unused[0].message().contains("unknown configuration section `databse`"));
/// # Ok::<(), navree_config::ConfigError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnusedKey {
    /// The dotted key: a section (`databse`) or a key below one
    /// (`database.max_conections`). A whole table is reported once.
    pub key: String,
    /// Where it is set.
    pub origin: ValueOrigin,
    /// The closest name that is read, when one is close enough to be a
    /// likely typo.
    pub suggestion: Option<String>,
}

impl UnusedKey {
    /// Whether the key is a whole section (it has no dot).
    pub fn is_section(&self) -> bool {
        !self.key.contains(['.', '['])
    }

    /// The warning shown at boot and by `config:check`. It names the key and
    /// where it is set, never a value.
    pub fn message(&self) -> String {
        let what = if self.is_section() { "section" } else { "key" };
        let mut message = format!(
            "unknown configuration {what} `{}` (set by {}): nothing reads it, so it has no effect.",
            self.key, self.origin
        );
        if let Some(suggestion) = &self.suggestion {
            message.push_str(&format!(" Did you mean `{suggestion}`?"));
        }
        message
    }
}

impl fmt::Display for UnusedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

/// Whether `inner` is `outer` or lies below it (`a.b` and `a.b[0].c` lie
/// below `a`; `ab` does not).
fn covers(outer: &str, inner: &str) -> bool {
    outer.is_empty()
        || inner == outer
        || inner
            .strip_prefix(outer)
            .is_some_and(|rest| rest.starts_with(['.', '[']))
}

/// The keys of `root` that no read covers. `claims` are keys (and the
/// subtrees below them) that something reads in a way the notes cannot see.
pub(crate) fn unused(root: &Value, usage: &Usage, claims: &[&str]) -> Vec<UnusedKey> {
    let (keys, sections) = usage.snapshot();

    // Whether a read covers the leaf at `path`. A claim on a key below a
    // section (`observe.tracing`) always applies. A claim on a section
    // applies only where no section read covers the key: when one does, that
    // read decides, which is how a typo in a section that is read is found.
    let consumed = |path: &str| -> bool {
        if keys.contains(path)
            || claims
                .iter()
                .any(|claim| claim.contains('.') && covers(claim, path))
        {
            return true;
        }
        let mut covering = sections.iter().filter(|read| covers(&read.key, path));
        match covering.next() {
            None => claims.iter().any(|claim| covers(claim, path)),
            Some(first) => std::iter::once(first)
                .chain(covering)
                .any(|read| !read.unknown.iter().any(|u| covers(u, path))),
        }
    };

    let mut used_leaves: Vec<String> = Vec::new();
    let mut unused_leaves: Vec<String> = Vec::new();
    root.for_each_leaf("", &mut |path, _| {
        if consumed(path) {
            used_leaves.push(path.to_owned());
        } else {
            unused_leaves.push(path.to_owned());
        }
    });

    // Name each unread leaf once, as the key a person should fix. Inside a
    // section that was read, that is the key the type has no field for (the
    // most specific section that covers the leaf decides). Elsewhere it is
    // the largest subtree with no read leaf in it: `[databse]` once, not
    // every key below it.
    let mut reported: BTreeSet<String> = BTreeSet::new();
    for leaf in &unused_leaves {
        let covering = sections
            .iter()
            .filter(|read| covers(&read.key, leaf))
            .max_by_key(|read| read.key.len());
        if let Some(read) = covering {
            let shortest = read
                .unknown
                .iter()
                .filter(|u| covers(u, leaf))
                .min_by_key(|u| u.len());
            reported.insert(shortest.cloned().unwrap_or_else(|| leaf.clone()));
            continue;
        }
        let mut prefix = String::new();
        for segment in leaf.split('.') {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(segment);
            if !used_leaves.iter().any(|used| covers(&prefix, used)) {
                break;
            }
        }
        reported.insert(prefix);
    }

    // Keys inside lists are not leaves of the tree: take them from the reads.
    for read in &sections {
        for path in read.unknown.iter().filter(|u| u.contains('[')) {
            let covered_elsewhere = sections.iter().any(|other| {
                covers(&other.key, path) && !other.unknown.iter().any(|u| covers(u, path))
            }) || claims
                .iter()
                .any(|claim| claim.contains('.') && covers(claim, path));
            if !covered_elsewhere {
                reported.insert(path.clone());
            }
        }
    }

    // A reported key below a reported key is part of it.
    let all: Vec<String> = reported.iter().cloned().collect();
    all.iter()
        .filter(|key| !all.iter().any(|other| other != *key && covers(other, key)))
        .map(|key| UnusedKey {
            origin: origin_of(root, key),
            suggestion: suggestion(key, &sections, claims),
            key: key.clone(),
        })
        .collect()
}

/// Where the value at `key` (or the first value below it) is set.
fn origin_of(root: &Value, key: &str) -> ValueOrigin {
    // Keys inside a list are set by the list.
    let table_key = key.split('[').next().unwrap_or(key);
    let mut found = None;
    if let Some(node) = root.lookup(table_key) {
        node.for_each_leaf(table_key, &mut |_, leaf| {
            if found.is_none() {
                found = leaf.origin.as_ref().map(|o| o.source().clone());
            }
        });
    }
    found.unwrap_or(ValueOrigin::Default)
}

/// The name that `key` most likely meant, from the fields the structs read
/// from its table, or (at the top) from the sections that are read.
fn suggestion(key: &str, sections: &[SectionRead], claims: &[&str]) -> Option<String> {
    let (parent, name) = match key.rsplit_once('.') {
        Some((parent, name)) => (parent, name),
        None => ("", key),
    };
    if name.contains('[') {
        return None;
    }
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    if parent.is_empty() {
        for read in sections {
            candidates.extend(read.key.split('.').next().map(str::to_owned));
        }
        candidates.extend(
            claims
                .iter()
                .filter_map(|claim| claim.split('.').next().map(str::to_owned)),
        );
    }
    for read in sections {
        for (table, fields) in &read.tables {
            if table == parent {
                candidates.extend(fields.iter().map(|field| (*field).to_owned()));
            }
        }
        // A section registered below this table (`database.mongodb`).
        if let Some(rest) = read.key.strip_prefix(parent).filter(|_| !parent.is_empty())
            && let Some(child) = rest.strip_prefix('.')
        {
            candidates.extend(child.split('.').next().map(str::to_owned));
        }
    }
    candidates.remove(name);
    let best = candidates
        .iter()
        .map(|candidate| (distance(name, candidate), candidate))
        .min_by_key(|(distance, _)| *distance)?;
    let allowed = (name.chars().count() / 3).clamp(1, 3);
    (best.0 <= allowed).then(|| {
        if parent.is_empty() {
            best.1.clone()
        } else {
            format!("{parent}.{}", best.1)
        }
    })
}

/// The edit distance between two names (insert, delete, replace, and
/// swapping two neighbours each cost one).
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut table = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in table.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        table[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (table[i - 1][j] + 1)
                .min(table[i][j - 1] + 1)
                .min(table[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(table[i - 2][j - 2] + 1);
            }
            table[i][j] = best;
        }
    }
    table[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_matches_whole_segments() {
        assert!(covers("a", "a"));
        assert!(covers("a", "a.b"));
        assert!(covers("a", "a[0].b"));
        assert!(!covers("a", "ab"));
        assert!(!covers("a.b", "a"));
        assert!(covers("", "anything"));
    }

    #[test]
    fn distance_counts_edits_and_swaps() {
        assert_eq!(distance("databse", "database"), 1);
        assert_eq!(distance("max_conections", "max_connections"), 1);
        assert_eq!(distance("worker", "workers"), 1);
        assert_eq!(distance("abc", "xyz"), 3);
        assert_eq!(distance("", "ab"), 2);
    }
}
