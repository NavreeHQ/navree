//! The query builder: pages, hits, filters, sorts, access, workspaces and
//! `across`.

use std::sync::Arc;

use navree_access::{Accessible, READ, Read, Rules, field};
use navree_db::{Db, PageQuery};
use navree_search::{Search, SearchError};
use uuid::Uuid;

use crate::support::{
    self, Contact, Deal, Company, ids, insert, insert_contacts, new_company, new_contact, settle,
};

/// Ada, Grace and Alan; Ada and Alan owned by `owner`, Grace a customer.
async fn seeded(engine: &str) -> (navree_testing::TestApp, Uuid, Vec<Contact>) {
    let app = support::app(engine, "inline").await;
    let db = app.get::<Db>().await;
    let owner = Uuid::now_v7();
    let mut ada = new_contact("Ada", "Lovelace");
    ada.owner_id = owner;
    ada.score = 30;
    ada.title = Some("Countess of Lovelace".into());
    let mut grace = new_contact("Grace", "Hopper");
    grace.status = "customer".into();
    grace.score = 50;
    let mut alan = new_contact("Alan", "Turing");
    alan.owner_id = owner;
    alan.score = 10;
    alan.email = Some("alan@lovelace-labs.io".into());
    let rows = insert_contacts(&db, vec![ada, grace, alan]).await;
    settle(&app).await;
    (app, owner, rows)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paginate_returns_models_in_relevance_order() {
    for &engine in support::engines() {
        let (app, _, rows) = seeded(engine).await;
        let search = app.get::<Search>().await;
        let page = search
            .query::<Contact>("lovelace")
            .paginate(PageQuery::new(1, 15))
            .await
            .unwrap();
        assert_eq!(
            ids(&page),
            [rows[0].id.to_string(), rows[2].id.to_string()],
            "{engine}: the last name outranks the email"
        );
        assert_eq!(page.meta.total, 2);
        assert_eq!(
            (
                page.meta.current_page,
                page.meta.per_page,
                page.meta.last_page
            ),
            (1, 15, 1)
        );

        let second = search
            .query::<Contact>("")
            .sort_desc("score")
            .paginate(PageQuery::new(2, 2))
            .await
            .unwrap();
        assert_eq!(ids(&second), [rows[2].id.to_string()], "{engine}");
        assert_eq!((second.meta.total, second.meta.last_page), (3, 2));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hits_carry_scores_and_highlights() {
    let (app, _, rows) = seeded("memory").await;
    let search = app.get::<Search>().await;
    let page = search
        .query::<Contact>("lovlace")
        .hits(PageQuery::default())
        .await
        .unwrap();
    assert_eq!(page.data[0].record.id, rows[0].id);
    assert!(page.data[0].score > 0.0 && page.data[0].score <= 1.0);
    assert_eq!(
        page.data[0].highlights.get("last_name").map(String::as_str),
        Some("<mark>Lovelace</mark>")
    );
    let json = serde_json::to_value(&page).unwrap();
    assert_eq!(json["data"][0]["first_name"], "Ada");
    assert!(json["data"][0]["_score"].is_number());
    assert_eq!(
        json["data"][0]["_highlights"]["last_name"],
        "<mark>Lovelace</mark>"
    );

    let page = search
        .query::<Contact>("countess")
        .highlight_tags("<em>", "</em>")
        .hits(PageQuery::default())
        .await
        .unwrap();
    assert_eq!(
        page.data[0].highlights["title"],
        "<em>Countess</em> of Lovelace"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn filters_and_sorts_use_declared_fields() {
    let (app, owner, rows) = seeded(support::database_engine()).await;
    let search = app.get::<Search>().await;
    let customers = search
        .query::<Contact>("")
        .filter("status", "customer")
        .ids(10)
        .await
        .unwrap();
    assert_eq!(customers, [rows[1].id.to_string()]);
    let mine = search
        .query::<Contact>("")
        .filter_in("owner_id", [owner])
        .sort_asc("score")
        .ids(10)
        .await
        .unwrap();
    assert_eq!(mine, [rows[2].id.to_string(), rows[0].id.to_string()]);
    let not_leads = search
        .query::<Contact>("")
        .filter_not("status", "lead")
        .count()
        .await
        .unwrap();
    assert_eq!(not_leads, 1);
    let scored = search
        .query::<Contact>("")
        .range("score", 20..)
        .count()
        .await
        .unwrap();
    assert_eq!(scored, 2);

    let error = search
        .query::<Contact>("ada")
        .filter("first_name", "Ada")
        .ids(1)
        .await
        .unwrap_err();
    assert!(matches!(error, SearchError::NotFilterable { ref field, .. } if field == "first_name"));
    assert_eq!(error.to_app_error().status.as_u16(), 400);
    let error = search
        .query::<Contact>("")
        .sort_desc("status")
        .ids(1)
        .await
        .unwrap_err();
    assert!(matches!(error, SearchError::NotSortable { .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accessible_scopes_become_engine_filters() {
    let (app, owner, rows) = seeded("memory").await;
    let search = app.get::<Search>().await;
    let mut rules = Rules::new();
    rules
        .can(READ, "Contact")
        .when(field("owner_id").eq(owner.to_string()));
    let scope = Accessible::<Contact, Read>::new(Arc::new(rules.build()));
    let page = search
        .query::<Contact>("")
        .accessible(scope)
        .sort_desc("score")
        .paginate(PageQuery::new(1, 1))
        .await
        .unwrap();
    assert_eq!(ids(&page), [rows[0].id.to_string()]);
    assert_eq!(page.meta.total, 2, "the total counts only readable records");

    // No rule: nothing.
    let none = Accessible::<Contact, Read>::new(Arc::new(Rules::new().build()));
    let page = search
        .query::<Contact>("")
        .accessible(none)
        .paginate(PageQuery::default())
        .await
        .unwrap();
    assert_eq!(page.meta.total, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rules_on_unindexed_columns_filter_the_loaded_rows() {
    let (app, _, rows) = seeded("memory").await;
    let search = app.get::<Search>().await;
    let mut rules = Rules::new();
    rules
        .can(READ, "Contact")
        .when(field("last_name").eq("Hopper"));
    let scope = Accessible::<Contact, Read>::new(Arc::new(rules.build()));
    let page = search
        .query::<Contact>("")
        .accessible(scope)
        .paginate(PageQuery::default())
        .await
        .unwrap();
    assert_eq!(ids(&page), [rows[1].id.to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_scoped_indexes_need_a_workspace() {
    let app = support::app("memory", "inline").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let (w1, w2) = (Uuid::now_v7(), Uuid::now_v7());
    let companies = insert(
        &db,
        vec![new_company(w1, "Acme"), new_company(w2, "Acme Europe")],
    )
    .await;
    settle(&app).await;
    let error = search.query::<Company>("acme").ids(10).await.unwrap_err();
    assert!(
        matches!(error, SearchError::WorkspaceRequired { .. }),
        "{error}"
    );
    let found = search
        .query::<Company>("acme")
        .workspace(w2)
        .ids(10)
        .await
        .unwrap();
    assert_eq!(found, [companies[1].id.to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn date_ranges_and_sorts_compare_dates() {
    let app = support::app(support::database_engine(), "inline").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let date = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    let deals = insert(
        &db,
        [
            ("Q4 renewal", Some("2026-12-01")),
            ("Q1 renewal", Some("2027-02-01")),
            ("Open renewal", None),
        ]
        .into_iter()
        .map(|(title, closes)| Deal {
            id: Uuid::now_v7(),
            title: title.into(),
            company_id: None,
            stage: "negotiation".into(),
            amount_cents: 100,
            closes_on: closes.map(date),
            archived: false,
        })
        .collect(),
    )
    .await;
    settle(&app).await;
    let this_year = search
        .query::<Deal>("renewal")
        .range("closes_on", ..date("2027-01-01"))
        .ids(10)
        .await
        .unwrap();
    assert_eq!(this_year, [deals[0].id.to_string()]);
    let by_close = search
        .query::<Deal>("")
        .sort_desc("closes_on")
        .ids(10)
        .await
        .unwrap();
    assert_eq!(
        by_close,
        [
            deals[1].id.to_string(),
            deals[0].id.to_string(),
            deals[2].id.to_string()
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn across_merges_indexes_by_score() {
    let (app, _, rows) = seeded("memory").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let [lovelace_deal] = insert(
        &db,
        vec![Deal {
            id: Uuid::now_v7(),
            title: "Lovelace renewal".into(),
            company_id: None,
            stage: "won".into(),
            amount_cents: 5,
            closes_on: None,
            archived: false,
        }],
    )
    .await
    .try_into()
    .unwrap();
    settle(&app).await;
    let all = |subject: &str| {
        let mut rules = Rules::new();
        rules.can(READ, subject);
        Arc::new(rules.build())
    };
    let results = search
        .across("lovelace")
        .include::<Contact>(Accessible::<Contact, Read>::new(all("Contact")))
        .include::<Deal>(Accessible::<Deal, Read>::new(all("Deal")))
        .limit(10)
        .run()
        .await
        .unwrap();
    let kinds: Vec<(&str, &str)> = results
        .iter()
        .map(|h| (h.kind.as_str(), h.id.as_str()))
        .collect();
    assert_eq!(results.len(), 3, "{kinds:?}");
    assert!(kinds.contains(&("Deal", lovelace_deal.id.to_string().as_str())));
    assert!(results.windows(2).all(|w| w[0].score >= w[1].score));
    let ada = results
        .iter()
        .find(|h| h.id == rows[0].id.to_string())
        .unwrap();
    assert_eq!(ada.title.as_deref(), Some("Ada"));
    let uow = db.uow(None, None).await;
    let loaded = ada.load::<Contact>(&uow).await.unwrap().unwrap();
    assert_eq!(loaded.last_name, "Lovelace");
    assert!(
        ada.load::<Deal>(&uow).await.unwrap().is_none(),
        "another kind"
    );
    uow.rollback().await.unwrap();

    let limited = search
        .across("lovelace")
        .include(Accessible::<Contact, Read>::new(all("Contact")))
        .include(Accessible::<Deal, Read>::new(Arc::new(
            Rules::new().build(),
        )))
        .limit(1)
        .run()
        .await
        .unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].kind, "Contact");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_synonyms_and_stop_words_apply() {
    for &engine in support::engines() {
        let app = support::builder(engine, "inline")
            .config(
                "search.indexes.contacts.synonyms",
                serde_json::json!({ "bob": ["robert"] }),
            )
            .config("search.indexes.contacts.stop_words", "the,of")
            .build()
            .await;
        let db = app.get::<Db>().await;
        let rows = insert_contacts(&db, vec![new_contact("Robert", "Noyce")]).await;
        settle(&app).await;
        let search = app.get::<Search>().await;
        assert_eq!(
            search.query::<Contact>("bob ").ids(10).await.unwrap(),
            [rows[0].id.to_string()],
            "{engine}: synonyms"
        );
        assert_eq!(
            search.query::<Contact>("the noyce").ids(10).await.unwrap(),
            [rows[0].id.to_string()],
            "{engine}: stop words"
        );
    }
}
