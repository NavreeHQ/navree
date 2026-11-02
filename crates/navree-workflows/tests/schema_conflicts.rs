//! Two different types of one name under a schema: a `$ref` can't tell them apart,
//! so asking for the schema panics and names both.

#![allow(dead_code)]

use navree_openapi::ToSchema;

mod first {
    use super::ToSchema;

    #[derive(ToSchema)]
    pub struct Direct {
        pub in_first: i32,
    }

    #[derive(ToSchema)]
    pub struct Item {
        pub child: Direct,
    }
}

mod second {
    use super::ToSchema;

    #[derive(ToSchema)]
    pub struct Direct {
        pub in_second: String,
    }

    #[derive(ToSchema)]
    pub struct Item {
        pub child: Direct,
    }
}

#[derive(ToSchema)]
struct DirectBoth {
    first: first::Direct,
    second: second::Direct,
}

#[derive(ToSchema)]
struct NestedBoth {
    first: first::Item,
    second: second::Item,
}

#[derive(ToSchema)]
struct Fine {
    only: first::Item,
}

#[test]
#[should_panic(expected = "`Direct` names two different schemas")]
fn two_types_of_one_name_refuse() {
    let _ = navree_workflows::json_schema::<DirectBoth>();
}

#[test]
#[should_panic(expected = "`Direct` names two different schemas")]
fn two_types_of_one_name_refuse_below_two_that_look_the_same() {
    let _ = navree_workflows::json_schema::<NestedBoth>();
}

#[test]
fn one_type_of_a_name_is_fine() {
    let schema = navree_workflows::json_schema::<Fine>();
    assert!(schema["$defs"]["Direct"].is_object(), "{schema}");
}
