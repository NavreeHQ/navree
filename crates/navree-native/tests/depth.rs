//! A tree deeper than a client reads is refused with an error, never by
//! running out of stack; one that fits is read whole.
//!
//! A debug build spends the most stack on each level, so the reading runs on a
//! thread of the size Rust gives one by default (2 MiB). A release build reads
//! every tree that fits on a quarter of that.

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

#[test]
fn a_tree_too_deep_to_read_is_refused_with_an_error_and_never_a_crash() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
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
                    let read = serde_json::from_str::<Envelope>(&json);
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
    assert!(serde_json::from_str::<Envelope>(&json).is_ok());
    let deeper = Envelope::screen("b1", Screen::new("Deep", chain(MAX_DEPTH + 1, row)));
    let refused = serde_json::from_str::<Envelope>(&serde_json::to_string(&deeper).unwrap());
    assert!(
        refused
            .unwrap_err()
            .to_string()
            .contains(&format!("more than {MAX_DEPTH} nodes deep"))
    );
}
