//! The matcher against a reference: every route that matches a path is
//! found, the one chosen is the best by precedence (static before
//! parameter before catch-all, segment by segment from the left), and the
//! raw parameters put back into the pattern give the path again.
//!
//! Then against the previous router's matcher (`matchit`, behind the
//! routes of crates not yet moved): with one route, every path Navree
//! matches the previous matcher matches too, with the same parameters,
//! which is what running those routes behind it relies on. The one
//! difference: the previous matcher lets a `{name}` match an empty segment
//! inside a path (`//a` for `/{a}/{b}`, with `a` empty); Navree never
//! matches a parameter to nothing, so a `//` reaches no route. With several
//! routes, they choose the same route whenever both match.

use navree_http::routing::{PathTree, check_path};
use proptest::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Seg {
    Static(&'static str),
    Param,
    CatchAll,
}

const STATICS: [&str; 5] = ["a", "b", "c", "me", "v1"];
const PATH_SEGMENTS: [&str; 11] = [
    "a", "b", "c", "me", "v1", "", "zz", "%41", "a%2Fb", "é", "a.b",
];

fn pattern_of(segs: &[Seg]) -> String {
    if segs.is_empty() {
        return "/".to_owned();
    }
    let mut out = String::new();
    for (index, seg) in segs.iter().enumerate() {
        out.push('/');
        match seg {
            Seg::Static(text) => out.push_str(text),
            Seg::Param => out.push_str(&format!("{{p{index}}}")),
            Seg::CatchAll => out.push_str(&format!("{{*p{index}}}")),
        }
    }
    out
}

/// Whether `segs` matches `path`: its precedence (0 static, 1 parameter,
/// 2 catch-all, per segment) and raw parameters.
fn reference(segs: &[Seg], path: &str) -> Option<(Vec<u8>, Vec<String>)> {
    let parts: Vec<&str> = if path == "/" {
        Vec::new()
    } else {
        path.strip_prefix('/')?.split('/').collect()
    };
    let mut precedence = Vec::new();
    let mut params = Vec::new();
    for (index, seg) in segs.iter().enumerate() {
        match seg {
            Seg::Static(text) => {
                if parts.get(index) != Some(text) {
                    return None;
                }
                precedence.push(0);
            }
            Seg::Param => {
                let part = parts.get(index).filter(|part| !part.is_empty())?;
                precedence.push(1);
                params.push((*part).to_owned());
            }
            Seg::CatchAll => {
                if index >= parts.len() {
                    return None;
                }
                let rest = parts[index..].join("/");
                if rest.is_empty() {
                    return None;
                }
                precedence.push(2);
                params.push(rest);
                return Some((precedence, params));
            }
        }
    }
    (segs.len() == parts.len()).then_some((precedence, params))
}

fn route() -> impl Strategy<Value = Vec<Seg>> {
    (
        prop::collection::vec(
            prop_oneof![
                3 => prop::sample::select(STATICS.to_vec()).prop_map(Seg::Static),
                1 => Just(Seg::Param),
            ],
            0..5,
        ),
        prop::bool::weighted(0.2),
    )
        .prop_map(|(mut segs, catch_all)| {
            if catch_all {
                segs.push(Seg::CatchAll);
            }
            segs
        })
}

fn path() -> impl Strategy<Value = String> {
    prop_oneof![
        8 => prop::collection::vec(prop::sample::select(PATH_SEGMENTS.to_vec()), 0..6)
            .prop_map(|segments| format!("/{}", segments.join("/"))),
        1 => any::<String>(),
        1 => "[a-c/%.]{0,12}",
    ]
}

/// The routes that went in (conflicts left out), with their tree.
fn build(routes: &[Vec<Seg>]) -> (Vec<Vec<Seg>>, PathTree<usize>) {
    let mut tree = PathTree::new();
    let mut kept = Vec::new();
    for segs in routes {
        if tree.insert(&pattern_of(segs), kept.len()).is_ok() {
            kept.push(segs.clone());
        }
    }
    (kept, tree)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn the_best_matching_route_is_chosen(
        routes in prop::collection::vec(route(), 1..12),
        paths in prop::collection::vec(path(), 1..8),
    ) {
        let (kept, tree) = build(&routes);
        for path in &paths {
            let expected = kept
                .iter()
                .enumerate()
                .filter_map(|(id, segs)| reference(segs, path).map(|(rank, params)| (rank, id, params)))
                .min_by(|a, b| a.0.cmp(&b.0));
            let found = tree.at(path);
            match (&expected, &found) {
                (None, None) => {}
                (Some((_, id, params)), Some(found)) => {
                    prop_assert_eq!(*found.value, *id, "path {:?}", path);
                    let raw: Vec<String> = found.raw_values().iter().map(|v| (*v).to_owned()).collect();
                    prop_assert_eq!(&raw, params, "path {:?}", path);
                }
                _ => prop_assert!(false, "path {path:?}: expected {expected:?}, found {:?}", found.map(|f| *f.value)),
            }
        }
    }

    #[test]
    fn raw_parameters_put_back_give_the_path(
        routes in prop::collection::vec(route(), 1..12),
        path in path(),
    ) {
        let (kept, tree) = build(&routes);
        if let Some(found) = tree.at(&path) {
            let segs = &kept[*found.value];
            let mut values = found.raw_values().iter();
            let mut rebuilt = String::new();
            for seg in segs {
                rebuilt.push('/');
                match seg {
                    Seg::Static(text) => rebuilt.push_str(text),
                    Seg::Param | Seg::CatchAll => rebuilt.push_str(values.next().unwrap()),
                }
            }
            if segs.is_empty() {
                rebuilt.push('/');
            }
            prop_assert_eq!(rebuilt, path);
        }
    }

    #[test]
    fn one_route_matches_what_the_previous_matcher_matches(segs in route(), path in path()) {
        let pattern = pattern_of(&segs);
        let mut tree = PathTree::new();
        tree.insert(&pattern, ()).unwrap();
        let mut previous = matchit::Router::new();
        previous.insert(&pattern, ()).unwrap();
        let ours = tree.at(&path).map(|found| found.params().into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect::<Vec<_>>());
        let theirs = previous.at(&path).ok().map(|found| found.params.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect::<Vec<_>>());
        match (&ours, &theirs) {
            (None, Some(params)) => prop_assert!(
                params.iter().any(|(_, value)| value.is_empty()),
                "only an empty parameter may differ: pattern {pattern:?} path {path:?} {theirs:?}"
            ),
            _ => prop_assert_eq!(&ours, &theirs, "pattern {:?} path {:?}", pattern, path),
        }
    }

    #[test]
    fn several_routes_choose_like_the_previous_matcher_when_both_match(
        routes in prop::collection::vec(route(), 1..10),
        path in path(),
    ) {
        let (kept, tree) = build(&routes);
        let mut previous = matchit::Router::new();
        let mut insertable = true;
        for (id, segs) in kept.iter().enumerate() {
            // The previous matcher refuses some sets ours accepts (two
            // parameter names at one position); skip those.
            insertable &= previous.insert(pattern_of(segs), id).is_ok();
        }
        prop_assume!(insertable);
        if let (Some(ours), Ok(theirs)) = (tree.at(&path), previous.at(&path)) {
            prop_assert_eq!(*ours.value, *theirs.value, "path {:?}", path);
        }
        if let Ok(theirs) = previous.at(&path)
            && !theirs.params.iter().any(|(_, value)| value.is_empty())
        {
            prop_assert!(tree.at(&path).is_some(), "the previous matcher matched {path:?}");
        }
    }

    #[test]
    fn checking_any_path_never_panics(path in any::<String>()) {
        let _ = check_path(&path);
    }
}

#[test]
fn backtracking_finds_routes_behind_a_static_dead_end() {
    let mut tree = PathTree::new();
    tree.insert("/a/b/d", "static").unwrap();
    tree.insert("/a/{x}/c", "param").unwrap();
    assert_eq!(tree.at("/a/b/c").map(|found| *found.value), Some("param"));
}
