//! [`PathTree`]: path patterns to values, matched segment by segment.
//!
//! Matching walks a tree with one level per segment. At each level a
//! segment tries, in this order:
//!
//! 1. the static child with exactly its raw bytes;
//! 2. the parameter child (`{name}`), when the segment is not empty;
//! 3. the catch-all (`{*name}`), which takes the rest of the path when it
//!    is not empty.
//!
//! When a branch finds no route the walk backtracks and tries the next one,
//! so `/users/me/edit` reaches `/users/{id}/edit` even when `/users/me`
//! exists. The first route found in that order wins: precedence is decided
//! segment by segment from the left, static before parameter before
//! catch-all. The walk visits each tree node at most once per request, so
//! its cost is bounded by the routes, never by the path.

use std::fmt;

use super::MAX_PARAMS;
use super::pattern::{PathPattern, PatternError, Segment};

/// Path patterns to values. See the [module documentation](self).
///
/// ```
/// use navree_http::routing::PathTree;
///
/// let mut tree = PathTree::new();
/// tree.insert("/users/{id}", "show").unwrap();
/// tree.insert("/users/me", "me").unwrap();
/// tree.insert("/files/{*path}", "file").unwrap();
///
/// let found = tree.at("/users/42").unwrap();
/// assert_eq!((*found.value, found.params()), ("show", vec![("id", "42")]));
/// assert_eq!(*tree.at("/users/me").unwrap().value, "me");
/// assert_eq!(tree.at("/files/a/b.txt").unwrap().params(), [("path", "a/b.txt")]);
/// assert!(tree.at("/users/42/").is_none());
/// assert!(tree.insert("/users/{user}", "again").is_err());
/// ```
#[derive(Clone)]
pub struct PathTree<T> {
    root: Node,
    routes: Vec<(PathPattern, T)>,
}

/// One level of the tree.
#[derive(Clone, Default)]
struct Node {
    /// The route whose path ends here.
    route: Option<usize>,
    /// Static children, sorted by segment.
    statics: Vec<(Box<str>, Node)>,
    /// The first byte and the length of each static child's segment, in the
    /// same order: scanned before any segment is compared, so a lookup
    /// touches one small array and compares only real candidates.
    keys: Vec<(u8, u32)>,
    /// The `{name}` child.
    param: Option<Box<Node>>,
    /// The route with a `{*name}` here.
    catch_all: Option<usize>,
}

impl Node {
    /// The static child whose segment begins `rest` (the path after a `/`)
    /// and ends there or at the next `/`, with the path after it. No search
    /// for the end of the segment: a child's own length says where to look.
    fn static_child<'p>(&self, rest: &'p str) -> Option<(&Node, Option<&'p str>)> {
        let bytes = rest.as_bytes();
        let first = *bytes.first()?;
        for (index, &(key_first, key_len)) in self.keys.iter().enumerate() {
            if key_first > first {
                // Sorted: no later child starts with `first`.
                break;
            }
            let key_len = key_len as usize;
            if key_first != first || bytes.len() < key_len {
                continue;
            }
            let next = match bytes.get(key_len) {
                None => None,
                Some(b'/') => Some(&rest[key_len + 1..]),
                Some(_) => continue,
            };
            if rest.as_bytes()[..key_len] == *self.statics[index].0.as_bytes() {
                return Some((&self.statics[index].1, next));
            }
        }
        None
    }

    fn static_child_mut(&mut self, segment: &str) -> &mut Node {
        let index = match self
            .statics
            .binary_search_by(|(text, _)| (**text).cmp(segment))
        {
            Ok(index) => index,
            Err(index) => {
                self.statics
                    .insert(index, (segment.into(), Node::default()));
                // Patterns have no empty static segment, and a segment that
                // long could never match a request.
                let first = segment.as_bytes().first().copied().unwrap_or_default();
                let len = u32::try_from(segment.len()).unwrap_or(u32::MAX);
                self.keys.insert(index, (first, len));
                index
            }
        };
        &mut self.statics[index].1
    }
}

/// A match: the value and the raw (not yet decoded) parameter values.
pub struct Matched<'t, 'p, T> {
    /// The matched route's value.
    pub value: &'t T,
    /// The matched route's pattern.
    pub pattern: &'t PathPattern,
    raw: [&'p str; MAX_PARAMS],
    len: usize,
}

impl<'t, 'p, T> Matched<'t, 'p, T> {
    /// The raw parameter values, in pattern order (still percent-encoded).
    pub fn raw_values(&self) -> &[&'p str] {
        &self.raw[..self.len]
    }

    /// `(name, raw value)` pairs.
    pub fn params(&self) -> Vec<(&'t str, &'p str)> {
        self.pattern
            .param_names()
            .iter()
            .map(|name| &**name)
            .zip(self.raw_values().iter().copied())
            .collect()
    }

    /// Whether the last parameter is a catch-all.
    pub fn ends_in_catch_all(&self) -> bool {
        matches!(self.pattern.segments().last(), Some(Segment::CatchAll))
    }
}

impl<T> PathTree<T> {
    /// An empty tree.
    pub fn new() -> Self {
        Self {
            root: Node::default(),
            routes: Vec::new(),
        }
    }

    /// Adds `pattern` with `value`. Fails when the pattern is malformed or
    /// another pattern of the same shape is already in the tree.
    pub fn insert(&mut self, pattern: &str, value: T) -> Result<(), InsertError> {
        let pattern = PathPattern::parse(pattern).map_err(InsertError::Pattern)?;
        self.insert_parsed(pattern, value)
    }

    /// Adds a parsed pattern with `value`.
    pub(crate) fn insert_parsed(
        &mut self,
        pattern: PathPattern,
        value: T,
    ) -> Result<(), InsertError> {
        let id = self.routes.len();
        let mut node = &mut self.root;
        // A catch-all is always the last segment.
        let mut catch_all = false;
        for segment in pattern.segments() {
            match segment {
                Segment::Static(text) => node = node.static_child_mut(text),
                Segment::Param => node = &mut **node.param.get_or_insert_with(Box::default),
                Segment::CatchAll => catch_all = true,
            }
        }
        let slot = if catch_all {
            &mut node.catch_all
        } else {
            &mut node.route
        };
        if let Some(existing) = *slot {
            return Err(InsertError::Conflict {
                pattern: pattern.as_str().to_owned(),
                existing: self.routes[existing].0.as_str().to_owned(),
            });
        }
        *slot = Some(id);
        self.routes.push((pattern, value));
        Ok(())
    }

    /// The value of the route `path` matches, with its raw parameters.
    /// `path` is a request path (`/users/42`, no query); one that does not
    /// start with `/` matches nothing.
    pub fn at<'t, 'p>(&'t self, path: &'p str) -> Option<Matched<'t, 'p, T>> {
        let rest = match path {
            "/" => None,
            _ => Some(path.strip_prefix('/')?),
        };
        let mut raw = [""; MAX_PARAMS];
        let mut len = 0;
        let id = walk(&self.root, rest, &mut raw, &mut len)?;
        let (pattern, value) = &self.routes[id];
        Some(Matched {
            value,
            pattern,
            raw,
            len,
        })
    }

    /// The pattern and value of the route with the same shape as `pattern`
    /// (the same paths), if any.
    pub(crate) fn same_shape_mut(
        &mut self,
        pattern: &PathPattern,
    ) -> Option<&mut (PathPattern, T)> {
        let mut node = &self.root;
        let mut catch_all = false;
        for segment in pattern.segments() {
            match segment {
                Segment::Static(text) => node = node.static_child(text)?.0,
                Segment::Param => node = node.param.as_deref()?,
                Segment::CatchAll => catch_all = true,
            }
        }
        let id = if catch_all {
            node.catch_all
        } else {
            node.route
        }?;
        Some(&mut self.routes[id])
    }

    /// Every pattern and value, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&PathPattern, &T)> {
        self.routes.iter().map(|(pattern, value)| (pattern, value))
    }

    /// Every value, in insertion order.
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.routes.iter_mut().map(|(_, value)| value)
    }

    /// How many routes the tree holds.
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// Whether the tree holds no route.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

impl<T> Default for PathTree<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: fmt::Debug> fmt::Debug for PathTree<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(
                self.routes
                    .iter()
                    .map(|(pattern, value)| (pattern.as_str(), value)),
            )
            .finish()
    }
}

/// Matches `rest` (the path after the `/` that leads to `node`; `None` when
/// the path ends at `node`), recording parameter values in `raw`.
fn walk<'p>(
    node: &Node,
    rest: Option<&'p str>,
    raw: &mut [&'p str; MAX_PARAMS],
    len: &mut usize,
) -> Option<usize> {
    let Some(rest) = rest else {
        return node.route;
    };
    if let Some((child, next)) = node.static_child(rest)
        && let Some(id) = walk(child, next, raw, len)
    {
        return Some(id);
    }
    if let Some(child) = &node.param
        && *len < MAX_PARAMS
    {
        let (segment, next) = match rest.find('/') {
            Some(at) => (&rest[..at], Some(&rest[at + 1..])),
            None => (rest, None),
        };
        if segment.is_empty() {
            return catch_all(node, rest, raw, len);
        }
        raw[*len] = segment;
        *len += 1;
        if let Some(id) = walk(child, next, raw, len) {
            return Some(id);
        }
        *len -= 1;
    }
    catch_all(node, rest, raw, len)
}

/// The catch-all of `node`, taking `rest` when it is not empty.
fn catch_all<'p>(
    node: &Node,
    rest: &'p str,
    raw: &mut [&'p str; MAX_PARAMS],
    len: &mut usize,
) -> Option<usize> {
    if !rest.is_empty()
        && let Some(id) = node.catch_all
        && *len < MAX_PARAMS
    {
        raw[*len] = rest;
        *len += 1;
        return Some(id);
    }
    None
}

/// Why a pattern cannot be added to a [`PathTree`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InsertError {
    /// The pattern is malformed.
    Pattern(PatternError),
    /// A pattern of the same shape (the same paths) is already there.
    Conflict {
        /// The pattern being added.
        pattern: String,
        /// The pattern already in the tree.
        existing: String,
    },
}

impl fmt::Display for InsertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InsertError::Pattern(error) => error.fmt(f),
            InsertError::Conflict { pattern, existing } => write!(
                f,
                "`{pattern}` matches the same paths as `{existing}` (only parameter names differ)"
            ),
        }
    }
}

impl std::error::Error for InsertError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(patterns: &[&str]) -> PathTree<String> {
        let mut tree = PathTree::new();
        for pattern in patterns {
            tree.insert(pattern, (*pattern).to_owned())
                .unwrap_or_else(|error| panic!("{pattern}: {error}"));
        }
        tree
    }

    fn route<'a>(tree: &'a PathTree<String>, path: &str) -> Option<&'a str> {
        tree.at(path).map(|found| found.value.as_str())
    }

    #[test]
    fn statics_win_over_parameters_and_parameters_over_catch_alls() {
        let tree = tree(&[
            "/",
            "/users",
            "/users/me",
            "/users/{id}",
            "/users/{id}/posts",
            "/files/{*path}",
            "/files/readme",
            "/files/{name}",
        ]);
        assert_eq!(route(&tree, "/"), Some("/"));
        assert_eq!(route(&tree, "/users"), Some("/users"));
        assert_eq!(route(&tree, "/users/me"), Some("/users/me"));
        assert_eq!(route(&tree, "/users/42"), Some("/users/{id}"));
        assert_eq!(route(&tree, "/users/me/posts"), Some("/users/{id}/posts"));
        assert_eq!(route(&tree, "/files/readme"), Some("/files/readme"));
        assert_eq!(route(&tree, "/files/a.txt"), Some("/files/{name}"));
        assert_eq!(route(&tree, "/files/a/b.txt"), Some("/files/{*path}"));
        assert_eq!(route(&tree, "/files/readme/x"), Some("/files/{*path}"));
    }

    #[test]
    fn the_walk_backtracks_from_a_static_dead_end() {
        let tree = tree(&["/a/b/d", "/a/{x}/c", "/{y}/b/e"]);
        assert_eq!(route(&tree, "/a/b/d"), Some("/a/b/d"));
        assert_eq!(route(&tree, "/a/b/c"), Some("/a/{x}/c"));
        assert_eq!(route(&tree, "/a/b/e"), Some("/{y}/b/e"));
        let found = tree.at("/a/b/c").unwrap();
        assert_eq!(found.params(), [("x", "b")]);
    }

    #[test]
    fn empty_segments_and_trailing_slashes_match_nothing() {
        let tree = tree(&["/", "/users", "/users/{id}", "/files/{*path}"]);
        for path in [
            "",
            "users",
            "//",
            "/users/",
            "/users//",
            "//users",
            "/users/42/",
            "/files",
            "/files/",
        ] {
            assert_eq!(route(&tree, path), None, "{path}");
        }
        assert_eq!(
            tree.at("/files/a/").unwrap().params(),
            [("path", "a/")],
            "a catch-all keeps a trailing slash"
        );
        assert_eq!(tree.at("/files//a").unwrap().params(), [("path", "/a")]);
    }

    #[test]
    fn parameters_are_raw_and_named_per_route() {
        let tree = tree(&["/deals/{id}/notes/{note}", "/deals/{deal}/files/{*path}"]);
        let found = tree.at("/deals/7/notes/caf%C3%A9").unwrap();
        assert_eq!(found.params(), [("id", "7"), ("note", "caf%C3%A9")]);
        assert!(!found.ends_in_catch_all());
        let found = tree.at("/deals/7/files/a%2Fb/c").unwrap();
        assert_eq!(found.params(), [("deal", "7"), ("path", "a%2Fb/c")]);
        assert!(found.ends_in_catch_all());
        assert_eq!(found.pattern.as_str(), "/deals/{deal}/files/{*path}");
    }

    #[test]
    fn statics_match_raw_bytes_only() {
        let tree = tree(&["/deals", "/caf"]);
        assert_eq!(
            route(&tree, "/d%65als"),
            None,
            "no decoding before matching"
        );
        assert_eq!(route(&tree, "/Deals"), None, "case matters");
        assert_eq!(route(&tree, "/deals%2F"), None);
    }

    #[test]
    fn same_shapes_conflict_and_other_shapes_coexist() {
        let mut tree = tree(&["/a/{x}", "/a/{x}/b", "/a/{*rest}", "/a/b"]);
        let error = tree.insert("/a/{y}", String::new()).unwrap_err();
        assert_eq!(
            error,
            InsertError::Conflict {
                pattern: "/a/{y}".into(),
                existing: "/a/{x}".into()
            }
        );
        assert!(tree.insert("/a/{*other}", String::new()).is_err());
        assert!(tree.insert("/a/b", String::new()).is_err());
        assert!(tree.insert("/", String::new()).is_ok());
        assert!(matches!(
            tree.insert("/a/:x", String::new()),
            Err(InsertError::Pattern(_))
        ));
        assert_eq!(tree.len(), 5);
    }

    #[test]
    fn many_statics_are_found() {
        let patterns: Vec<String> = (0..40).map(|i| format!("/s{i:02}/{{id}}")).collect();
        let refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
        let tree = tree(&refs);
        for (i, pattern) in patterns.iter().enumerate() {
            let path = format!("/s{i:02}/x");
            assert_eq!(route(&tree, &path), Some(pattern.as_str()));
        }
        assert_eq!(route(&tree, "/s40/x"), None);
    }

    #[test]
    fn deep_paths_cost_nothing_beyond_the_routes() {
        let tree = tree(&["/a/{b}/c"]);
        let long = format!("/a/{}", "x/".repeat(30_000));
        assert_eq!(route(&tree, &long), None);
        let tree = tree_with_catch_all();
        assert_eq!(
            tree.at(&long).map(|found| found.raw_values()[0].len()),
            Some(long.len() - 1)
        );
    }

    fn tree_with_catch_all() -> PathTree<String> {
        tree(&["/{*all}"])
    }
}
