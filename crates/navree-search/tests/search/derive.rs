//! `#[derive(Searchable)]` and hand-written `Searchable` impls.

use navree_search::{FieldSettings, Searchable};
use serde_json::json;
use uuid::Uuid;

use crate::support::{Company, Contact, Deal, contact, now};

fn field<'a>(fields: &'a [FieldSettings], name: &str) -> &'a FieldSettings {
    fields
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no field {name}"))
}

#[test]
fn the_derive_reads_field_and_model_attributes() {
    let settings = Contact::settings();
    assert_eq!(settings.name(), "Contact");
    assert_eq!(
        settings.index(),
        "contacts",
        "the index defaults to the table"
    );
    assert_eq!(settings.soft_delete_column(), Some("deleted_at"));
    assert_eq!(settings.workspace_column(), None);
    let fields = settings.fields();
    assert_eq!(field(fields, "first_name").boost, Some(3));
    let email = field(fields, "email_address");
    assert_eq!((email.boost, email.column.as_str()), (Some(2), "email"));
    assert_eq!(field(fields, "title").boost, Some(1), "plain #[searchable]");
    let status = field(fields, "status");
    assert!(status.filter && !status.is_text() && !status.sort);
    let score = field(fields, "score");
    assert!(score.filter && score.sort);
    assert!(field(fields, "created_at").sort);
    let notes = field(fields, "notes");
    assert!(notes.embed && !notes.is_text());
    assert!(settings.fields().iter().all(|f| f.name != "deleted_at"));
    assert_eq!(settings.title_field(), Some("first_name"));
    settings.validate().unwrap();
}

#[test]
fn the_derive_finds_the_workspace_column_of_workspace_scoped_entities() {
    let settings = Company::settings();
    assert_eq!(settings.name(), "Company");
    assert_eq!(settings.index(), "companies");
    assert_eq!(settings.workspace_column(), Some("workspace_id"));
    assert!(settings.field("workspace_id").is_none());
}

#[test]
fn hand_written_settings_are_used_as_they_are() {
    let settings = Deal::settings();
    assert_eq!(settings.name(), "Deal");
    assert!(settings.is_sortable("amount_cents") && settings.is_filterable("stage"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_derived_document_holds_the_key_and_marked_fields() {
    let app = crate::support::app("memory", "off").await;
    let search = app.get::<navree_search::Search>().await;
    let id = Uuid::now_v7();
    let model = contact::Model {
        id,
        first_name: "Ada".into(),
        last_name: "Lovelace".into(),
        email: Some("ada@lovelace.io".into()),
        title: None,
        status: "customer".into(),
        owner_id: Uuid::nil(),
        score: 7,
        created_at: now(),
        notes: Some("struggled with onboarding".into()),
        deleted_at: None,
    };
    let db = app.get::<navree_db::Db>().await;
    let tx = std::sync::Arc::new(sea_orm::TransactionTrait::begin(db.conn()).await.unwrap());
    let ctx = search_context(&app, tx).await;
    let document = Contact::document(&model, &ctx).await.unwrap().unwrap();
    assert_eq!(document.id(), id.to_string());
    assert_eq!(
        document.get("email_address"),
        Some(&json!("ada@lovelace.io"))
    );
    assert_eq!(document.get("title"), Some(&json!(null)));
    assert_eq!(document.get("score"), Some(&json!(7)));
    // Filter and sort fields hold the value as access rules see it: a
    // timestamp in UTC with microseconds.
    assert_eq!(
        document.get("created_at"),
        Some(&json!("2026-10-26T12:00:00.000000Z"))
    );
    assert_eq!(document.get("deleted_at"), None);
    drop(search);
}

mod ticket {
    use navree_search::Searchable;
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize)]
    #[sea_orm(rs_type = "String", db_type = "Text")]
    pub enum Priority {
        #[sea_orm(string_value = "p1")]
        Urgent,
        #[sea_orm(string_value = "p3")]
        Normal,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Searchable)]
    #[sea_orm(table_name = "tickets")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: i64,
        #[searchable(boost = 2, filter)]
        pub priority: Priority,
        #[searchable]
        pub label: Priority,
        #[sea_orm(column_name = "assignee")]
        #[searchable(filter)]
        pub owner: Option<Uuid>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn filter_fields_hold_database_values_and_text_fields_serde_values() {
    let settings = ticket::Entity::settings();
    let owner = settings.field("owner").expect("indexed by its field name");
    assert_eq!(owner.column, "assignee", "column_name is the column");
    assert_eq!(settings.field_for_column("assignee").unwrap().name, "owner");

    let app = crate::support::app("memory", "off").await;
    let db = app.get::<navree_db::Db>().await;
    let tx = std::sync::Arc::new(sea_orm::TransactionTrait::begin(db.conn()).await.unwrap());
    let ctx = search_context(&app, tx).await;
    let assignee = Uuid::now_v7();
    let model = ticket::Model {
        id: 7,
        priority: ticket::Priority::Urgent,
        label: ticket::Priority::Normal,
        owner: Some(assignee),
    };
    let document = ticket::Entity::document(&model, &ctx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(document.id(), "7");
    // Access rules compare `priority = "p1"`, the stored value.
    assert_eq!(document.get("priority"), Some(&json!("p1")));
    assert_eq!(document.get("label"), Some(&json!("Normal")));
    assert_eq!(document.get("owner"), Some(&json!(assignee.to_string())));
}

/// A search context the way the service builds one.
async fn search_context(
    app: &navree_testing::TestApp,
    tx: std::sync::Arc<sea_orm::DatabaseTransaction>,
) -> navree_search::SearchContext {
    navree_search::SearchContext::new(tx, app.ctx().clone(), None)
}
