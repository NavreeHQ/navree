//! `Authorized::check`: the record check outside a route.

use std::sync::Arc;

use chrono::Utc;
use navree_access::{Authorized, Rules, Update, field};
use uuid::Uuid;

use crate::support::post;

fn post(author: Uuid) -> post::Model {
    post::Model {
        id: Uuid::now_v7(),
        author_id: Some(author),
        title: "Hello".into(),
        status: "draft".into(),
        published: false,
        created_at: Utc::now(),
    }
}

#[test]
fn check_allows_a_record_the_ability_allows() {
    let me = Uuid::now_v7();
    let mut rules = Rules::new();
    rules.can("update", "Post").when(field("author_id").eq(me));
    let ability = Arc::new(rules.build());

    let authorized = Authorized::<post::Entity, Update>::check(post(me), ability).unwrap();

    assert_eq!(authorized.author_id, Some(me));
    assert_eq!(authorized.record()["title"], "Hello");
}

#[test]
fn check_refuses_a_record_the_ability_does_not_allow() {
    let me = Uuid::now_v7();
    let mut rules = Rules::new();
    rules.can("update", "Post").when(field("author_id").eq(me));
    let ability = Arc::new(rules.build());

    let Err(error) = Authorized::<post::Entity, Update>::check(post(Uuid::now_v7()), ability)
    else {
        panic!("someone else's post is refused");
    };

    assert_eq!(error.status.as_u16(), 403);
}
