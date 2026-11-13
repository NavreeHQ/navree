//! A tree deeper than a client reads is refused with an error, never by
//! running out of stack; one that fits is read whole.
//!
//! How much stack a reading takes depends on the build. A release build reads
//! every tree that fits on 512 KiB, a quarter of what Rust gives a thread by
//! default. A debug build spends many times that on each level, and most on
//! Windows, where the deepest tree that fits takes nearly 2 MiB: so a debug
//! build is given 8 MiB here. The trees themselves, some far deeper than any
//! answer may be, are built and written on a thread with room to spare:
//! writing a tree no server sends is not what is measured here.

use navree_native::node::{Row, Stack, Table, TableRow};
use navree_native::{Envelope, MAX_DEPTH, MAX_NESTING, Node, Screen, nesting};

/// Makes a node around the one inside it.
type Wrap = fn(Node) -> Node;

/// A chain `depth` nodes deep, each node of the shape `wrap` makes around the
/// one inside it.
fn chain(depth: usize, wrap: Wrap) -> Node {
    let mut tree = Node::text("the deepest");
    for _ in 1..depth {
        tree = wrap(tree);
    }
    tree
}

/// One more JSON level for each node: a row and what leads it.
fn row(inside: Node) -> Node {
    Node::Row(Row {
        title: "Order".to_owned(),
        leading: Some(Box::new(inside)),
        ..Row::default()
    })
}

/// Two: a stack and its list of children.
fn stack(inside: Node) -> Node {
    Node::Stack(Stack {
        children: vec![inside],
        ..Stack::default()
    })
}

/// Four: a table, its rows, a row, its cells.
fn table(inside: Node) -> Node {
    Node::Table(Table {
        rows: vec![TableRow {
            cells: vec![inside],
            ..TableRow::default()
        }],
        ..Table::default()
    })
}

/// The stack a reading gets: 512 KiB in a release build, 8 MiB in a debug
/// build.
const READING_STACK: usize = if cfg!(debug_assertions) {
    8 * 1024 * 1024
} else {
    512 * 1024
};

/// The stack the test's own trees are built, written and dropped on.
const BUILDING_STACK: usize = 64 * 1024 * 1024;

/// Reads `json` on a thread of [`READING_STACK`].
fn read(json: &str) -> Result<Envelope, serde_json::Error> {
    let json = json.to_owned();
    std::thread::Builder::new()
        .stack_size(READING_STACK)
        .spawn(move || serde_json::from_str::<Envelope>(&json))
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn a_tree_too_deep_to_read_is_refused_with_an_error_and_never_a_crash() {
    std::thread::Builder::new()
        .stack_size(BUILDING_STACK)
        .spawn(|| {
            let shapes: [(&str, Wrap); 3] = [("rows", row), ("stacks", stack), ("tables", table)];
            for (shape, wrap) in shapes {
                let mut read_one = false;
                for depth in [1, 10, 30, 31, 63, MAX_DEPTH, MAX_DEPTH + 1, 200, 500] {
                    let tree = chain(depth, wrap);
                    assert_eq!(tree.depth(), depth, "{shape}");
                    let envelope = Envelope::screen("b1", Screen::new("Deep", tree));
                    let json = serde_json::to_string(&envelope).unwrap();
                    let fits = depth <= MAX_DEPTH && nesting(&json) <= MAX_NESTING;
                    let read = read(&json);
                    assert_eq!(
                        read.is_ok(),
                        fits,
                        "{shape} {depth} deep, nesting {}: {:?}",
                        nesting(&json),
                        read.as_ref().err()
                    );
                    if let Ok(read) = read {
                        assert_eq!(read, envelope, "{shape} {depth}");
                        read_one = true;
                    }
                }
                assert!(read_one, "{shape}");
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn the_deepest_tree_that_fits_is_the_limit_of_both_readers() {
    // The node limit binds a chain of rows; the JSON limit binds a chain of
    // stacks first.
    let rows = Envelope::screen("b1", Screen::new("Deep", chain(MAX_DEPTH, row)));
    let json = serde_json::to_string(&rows).unwrap();
    assert!(nesting(&json) <= MAX_NESTING);
    assert!(read(&json).is_ok());
    let deeper = Envelope::screen("b1", Screen::new("Deep", chain(MAX_DEPTH + 1, row)));
    let refused = read(&serde_json::to_string(&deeper).unwrap());
    assert!(
        refused
            .unwrap_err()
            .to_string()
            .contains(&format!("more than {MAX_DEPTH} nodes deep"))
    );
}
