//! Properties: the reader takes any bytes without panicking, and content
//! operations written read back as the same operations.

use navree_pdf::content::{Operation, encode, operations};
use navree_pdf::{Document, Object, Objects, ReadLimits, StringFormat, TextLimits};
use proptest::prelude::*;

const LIMITS: ReadLimits = ReadLimits {
    memory: 16 * 1024 * 1024,
    work: 16 * 1024 * 1024,
    object_stream: 256 * 1024,
    sections: 16,
};

fn operand() -> impl Strategy<Value = Object> {
    let leaf = prop_oneof![
        Just(Object::Null),
        any::<bool>().prop_map(Object::Boolean),
        any::<i32>().prop_map(|v| Object::Integer(i64::from(v))),
        (-1.0e6f32..1.0e6f32).prop_map(Object::Real),
        proptest::collection::vec(any::<u8>(), 0..12).prop_map(|bytes| Object::Name(bytes.into())),
        proptest::collection::vec(any::<u8>(), 0..24)
            .prop_map(|bytes| Object::String(bytes, StringFormat::Literal)),
        proptest::collection::vec(any::<u8>(), 0..24)
            .prop_map(|bytes| Object::String(bytes, StringFormat::Hexadecimal)),
    ];
    leaf.prop_recursive(3, 24, 6, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..6).prop_map(Object::Array),
            proptest::collection::vec(("[A-Za-z][A-Za-z0-9]{0,6}", inner), 0..4)
                .prop_map(|entries| Object::Dictionary(entries.into_iter().collect())),
        ]
    })
}

fn operation() -> impl Strategy<Value = Operation> {
    (
        "[A-Za-z'\"*][A-Za-z0-9*]{0,3}".prop_filter("not an operand or an inline image", |op| {
            !matches!(op.as_str(), "true" | "false" | "null" | "BI")
        }),
        proptest::collection::vec(operand(), 0..5),
    )
        .prop_map(|(operator, operands)| Operation { operator, operands })
}

/// The same object, with strings compared by their bytes only.
fn same(a: &Object, b: &Object) -> bool {
    match (a, b) {
        (Object::String(x, _), Object::String(y, _)) => x == y,
        (Object::Array(x), Object::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Object::Dictionary(x), Object::Dictionary(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, value)| y.get(key).is_some_and(|other| same(value, other)))
        }
        _ => a == b,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn any_bytes_are_read_without_panicking(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let mut file = b"%PDF-1.7\n".to_vec();
        file.extend_from_slice(&data);
        if let Ok(document) = Document::load(file, LIMITS) {
            let _ = document.pages();
            let _ = navree_pdf::text::page_texts_within(&document, TextLimits::default());
            let _ = Objects::read(document);
        }
    }

    #[test]
    fn operations_read_back_as_written(ops in proptest::collection::vec(operation(), 0..8)) {
        let read: Vec<Operation> = operations(&encode(&ops)).collect::<Result<_, _>>().unwrap();
        prop_assert_eq!(read.len(), ops.len());
        for (read, written) in read.iter().zip(&ops) {
            prop_assert_eq!(&read.operator, &written.operator);
            prop_assert_eq!(read.operands.len(), written.operands.len());
            for (a, b) in read.operands.iter().zip(&written.operands) {
                prop_assert!(same(a, b), "{a:?} != {b:?}");
            }
        }
    }
}
