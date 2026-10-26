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
    assert_eq!(
        document.get("created_at"),
        Some(&json!("2026-10-26T12:00:00Z"))
    );
    assert_eq!(document.get("deleted_at"), None);
    drop(search);
}

/// A search context the way the service builds one.
async fn search_context(
    app: &navree_testing::TestApp,
    tx: std::sync::Arc<sea_orm::DatabaseTransaction>,
) -> navree_search::SearchContext {
    navree_search::SearchContext::new(tx, app.ctx().clone(), None)
}
