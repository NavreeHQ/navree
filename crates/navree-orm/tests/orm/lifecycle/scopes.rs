//! Global scopes registered by modules, removed by name, inspected.

use navree_kernel::{Module, ModuleBuilder};
use navree_orm::{Dialect, Model, ModelLifecycle, ModuleBuilderOrmExt};

use super::models::{Gadget, NotArchived, create_tables};
use crate::models::{Article, Deal};
use crate::support::{app, sqlite};

struct GadgetScopes;

impl Module for GadgetScopes {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global_scope::<Gadget>("active", |q| {
            q.where_(Gadget::ACTIVE.eq(true))
                .or_where(Gadget::NAME.eq("pinned"))
        })
        .global_scope_of::<Gadget>(NotArchived);
    }
}

fn names(gadgets: navree_orm::Collection<Gadget>) -> Vec<String> {
    let mut names = gadgets.map(|g| g.name.clone());
    names.sort();
    names
}

#[tokio::test]
async fn modules_register_global_scopes_that_queries_can_leave_out() {
    let (_app, db, _clock) = app(vec![Box::new(GadgetScopes)]).await;
    create_tables(&db).await;
    for (name, active, archived) in [
        ("on", true, false),
        ("off", false, false),
        ("pinned", false, false),
        ("old", true, true),
    ] {
        Gadget::create(&*db, Gadget::new(name, active, archived))
            .await
            .unwrap();
    }

    assert_eq!(names(Gadget::all(&*db).await.unwrap()), ["on", "pinned"]);
    assert_eq!(
        names(
            Gadget::query()
                .without_global_scope_named("active")
                .get(&*db)
                .await
                .unwrap()
        ),
        ["off", "on", "pinned"]
    );
    assert_eq!(
        names(
            Gadget::query()
                .without_global_scope::<NotArchived>()
                .get(&*db)
                .await
                .unwrap()
        ),
        ["old", "on", "pinned"]
    );
    assert_eq!(
        names(
            Gadget::query()
                .without_global_scopes_except(["NotArchived"])
                .get(&*db)
                .await
                .unwrap()
        ),
        ["off", "on", "pinned"]
    );
    assert_eq!(
        Gadget::query()
            .without_global_scopes()
            .count(&*db)
            .await
            .unwrap(),
        4
    );

    // Inspection.
    assert!(Gadget::has_global_scope("active"));
    assert!(Gadget::has_global_scope("NotArchived"));
    assert!(!Gadget::has_global_scope("SoftDeletes"));
    assert_eq!(Gadget::global_scope_names(), ["active", "NotArchived"]);
    assert!(Gadget::global_scope("active").is_some());
    assert_eq!(
        Gadget::query()
            .without_global_scope_named("active")
            .removed_scopes(),
        ["active"]
    );
    assert_eq!(
        Gadget::query()
            .without_global_scopes_except(["active"])
            .removed_scopes(),
        ["NotArchived"]
    );
    assert!(Gadget::query().removed_scopes().is_empty());

    // The scope's `or` is grouped, and the caller's too.
    let sql = Gadget::query()
        .where_(Gadget::NAME.eq("x"))
        .or_where(Gadget::NAME.eq("y"))
        .to_sql(Dialect::Postgres)
        .unwrap()
        .sql;
    assert!(
        sql.ends_with(
            r#"WHERE ("gadgets"."name" = $1 OR "gadgets"."name" = $2) AND ("gadgets"."active" = $3 OR "gadgets"."name" = $4) AND "gadgets"."archived" = $5"#
        ),
        "{sql}"
    );
}

#[test]
fn declared_scopes_and_soft_deletes_have_names_too() {
    assert_eq!(Article::global_scope_names(), ["Published", "ByAuthor"]);
    assert_eq!(Deal::global_scope_names(), ["SoftDeletes"]);
    assert!(Deal::has_global_scope("SoftDeletes"));
    assert_eq!(
        Deal::query().with_trashed().removed_scopes(),
        ["SoftDeletes"]
    );
    assert_eq!(
        Deal::query().only_trashed().removed_scopes(),
        ["SoftDeletes"]
    );
    assert!(Deal::query().removed_scopes().is_empty());
    assert_eq!(
        Article::query()
            .without_global_scope_named("ByAuthor")
            .removed_scopes(),
        ["ByAuthor"]
    );
}

#[tokio::test]
async fn declared_scopes_are_removed_by_name() {
    let t = sqlite().await;
    for (title, published, author) in [("Mine", true, "anu"), ("Theirs", true, "bob")] {
        Article::create(
            &t.db,
            Article {
                title: title.into(),
                published,
                author: author.into(),
                ..Article::default()
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(Article::query().count(&t.db).await.unwrap(), 1);
    assert_eq!(
        Article::query()
            .without_global_scope_named("ByAuthor")
            .count(&t.db)
            .await
            .unwrap(),
        2
    );
}
