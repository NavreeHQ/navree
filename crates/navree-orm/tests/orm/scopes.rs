//! Local scopes (`#[scopes]`, `Builder::scope`) and global scopes.

use navree_orm::{Builder, Dialect, Model};

use crate::models::{Article, ByAuthor, Deal, DealScopes, Published, Stage};
use crate::support::sqlite;

#[tokio::test]
async fn scope_methods_chain_on_the_builder() {
    let t = sqlite().await;
    for (title, cents, stage) in [
        ("Small", 50, Stage::Lead),
        ("Big", 90_000, Stage::Qualified),
        ("Won", 80_000, Stage::ClosedWon),
    ] {
        Deal::create(
            &t.db,
            Deal {
                stage,
                ..Deal::new(title, cents)
            },
        )
        .await
        .unwrap();
    }
    let open_and_big = Deal::query().open().worth(10_000).get(&t.db).await.unwrap();
    assert_eq!(open_and_big.map(|d| d.title.clone()), ["Big"]);
}

#[test]
fn a_scope_with_an_or_is_grouped() {
    let sql = Deal::query()
        .where_(Deal::TITLE.eq("x"))
        .doubtful()
        .to_sql(Dialect::Sqlite)
        .unwrap()
        .sql;
    assert!(
        sql.ends_with(
            r#"WHERE "deals"."title" = ? AND ("deals"."stage" = ? OR "deals"."amount_cents" < ?) AND "deals"."deleted_at" IS NULL"#
        ),
        "{sql}"
    );
}

#[test]
fn plain_functions_are_scopes_too() {
    fn recent(query: Builder<Deal>) -> Builder<Deal> {
        query.latest().limit(10)
    }
    let sql = Deal::query()
        .scope(recent)
        .to_sql(Dialect::Postgres)
        .unwrap()
        .sql;
    assert!(
        sql.ends_with(r#"ORDER BY "deals"."created_at" DESC LIMIT $1"#),
        "{sql}"
    );
}

#[tokio::test]
async fn global_scopes_apply_unless_removed() {
    let t = sqlite().await;
    for (title, published, author) in [
        ("Mine", true, "anu"),
        ("Draft", false, "anu"),
        ("House", true, "house"),
        ("Theirs", true, "bal"),
    ] {
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
    let titles = |articles: navree_orm::Collection<Article>| {
        let mut titles = articles.map(|a| a.title.clone());
        titles.sort();
        titles
    };
    assert_eq!(
        titles(Article::all(&t.db).await.unwrap()),
        ["House", "Mine"]
    );
    assert_eq!(
        titles(
            Article::query()
                .without_global_scope::<ByAuthor>()
                .get(&t.db)
                .await
                .unwrap()
        ),
        ["House", "Mine", "Theirs"]
    );
    assert_eq!(
        titles(
            Article::query()
                .without_global_scope::<Published>()
                .get(&t.db)
                .await
                .unwrap()
        ),
        ["Draft", "House", "Mine"]
    );
    assert_eq!(
        Article::query()
            .without_global_scopes()
            .count(&t.db)
            .await
            .unwrap(),
        4
    );
    // The scope's `OR` does not leak into the caller's conditions.
    assert_eq!(
        titles(
            Article::query()
                .where_(Article::TITLE.eq("Theirs"))
                .get(&t.db)
                .await
                .unwrap()
        ),
        Vec::<String>::new()
    );
    // Scopes apply to counts and subqueries too.
    assert_eq!(Article::query().count(&t.db).await.unwrap(), 2);
}
