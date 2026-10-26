//! `search:import`, `search:flush`, `search:settings` and `search:status`,
//! and the fresh-import swap.

use navree_db::{Db, Repo};
use navree_search::Search;
use sea_orm::ActiveValue::Set;
use sea_orm::IntoActiveModel;
use uuid::Uuid;

use crate::support::{self, Contact, Deal, deal, insert_contacts, new_contact};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn import_indexes_every_row_and_skips_filtered_ones() {
    for engine in ["memory", "postgres"] {
        let app = support::app(engine, "off").await;
        let db = app.get::<Db>().await;
        let search = app.get::<Search>().await;
        let mut gone = new_contact("Old", "Record");
        gone.deleted_at = Set(Some(support::now()));
        let rows = insert_contacts(
            &db,
            vec![
                new_contact("Ada", "Lovelace"),
                new_contact("Grace", "Hopper"),
                gone,
            ],
        )
        .await;
        app.run_command("search:import", &["Contact"])
            .await
            .unwrap();
        search.assert_indexed::<Contact>(rows[0].id).await;
        search.assert_indexed::<Contact>(rows[1].id).await;
        search.assert_not_indexed::<Contact>(rows[2].id).await;
        let status = search.status().await.unwrap();
        let contacts = status.iter().find(|s| s.name == "Contact").unwrap();
        assert_eq!((contacts.documents, contacts.rows), (2, 3), "{engine}");
        assert_eq!(contacts.engine, engine);

        app.run_command("search:flush", &["Contact"]).await.unwrap();
        search.assert_not_indexed::<Contact>(rows[0].id).await;
        app.run_command("search:import", &["--all"]).await.unwrap();
        search.assert_indexed::<Contact>(rows[0].id).await;
        app.run_command("search:settings", &[]).await.unwrap();
        app.run_command("search:status", &[]).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn imports_run_in_chunks() {
    let app = support::builder("memory", "off")
        .config("search.chunk_size", 2)
        .build()
        .await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let rows: Vec<_> = (0..5)
        .map(|i| new_contact(&format!("Person{i}"), "Smith"))
        .collect();
    insert_contacts(&db, rows).await;
    let report = search.import::<Contact>().await.unwrap();
    assert_eq!((report.rows, report.documents), (5, 5));
    assert_eq!(report.index, "contacts");
    assert_eq!(search.query::<Contact>("smith").count().await.unwrap(), 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_imports_swap_in_a_rebuilt_index() {
    for engine in ["memory", "postgres"] {
        let app = support::builder(engine, "off")
            .config("search.prefix", "crm_")
            .build()
            .await;
        let db = app.get::<Db>().await;
        let search = app.get::<Search>().await;
        let deal = support::in_uow(&db, async |uow| {
            Repo::<Deal>::insert(
                uow,
                deal::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    title: Set("Lovelace renewal".into()),
                    company_id: Set(None),
                    stage: Set("won".into()),
                    amount_cents: Set(1),
                    closes_on: Set(None),
                    archived: Set(false),
                },
            )
            .await
            .unwrap()
        })
        .await;
        search.import::<Deal>().await.unwrap();
        // The row is archived while syncing is off; the fresh import
        // rebuilds the index from the rows and swaps it in.
        let mut row = deal.clone().into_active_model();
        row.archived = Set(true);
        support::in_uow(&db, async |uow| {
            Repo::<Deal>::update(uow, row).await.unwrap()
        })
        .await;
        search.assert_indexed::<Deal>(deal.id).await;
        app.run_command("search:import", &["Deal", "--fresh"])
            .await
            .unwrap();
        search.assert_not_indexed::<Deal>(deal.id).await;
        assert_eq!(
            search.engine().count("crm_deals__fresh").await.unwrap(),
            0,
            "{engine}: the staged index is gone"
        );
        let report = search.import_fresh::<Deal>().await.unwrap();
        assert_eq!(
            (report.rows, report.skipped, report.index.as_str()),
            (1, 1, "crm_deals")
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commands_name_unknown_entities() {
    let app = support::app("memory", "off").await;
    let error = app
        .run_command("search:import", &["Invoice"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Invoice"), "{error}");
    let error = app.run_command("search:import", &[]).await.unwrap_err();
    assert!(error.to_string().contains("usage"), "{error}");
}
