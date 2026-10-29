//! `#[derive(Searchable)]` and hand-written `Searchable` impls.

use navree_orm::Record;
use navree_search::{FieldSettings, SearchContext, Searchable};
use serde_json::json;
use uuid::Uuid;

use crate::support::{Company, Contact, Deal, now};

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
fn the_derive_finds_the_workspace_column_of_workspace_scoped_models() {
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
    let id = Uuid::now_v7();
    let record = Record::from(Contact {
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
    });
    let ctx = search_context(&app).await;
    let document = Contact::document(&record, &ctx).await.unwrap().unwrap();
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
}

mod ticket {
    use navree_orm::Model;
    use navree_search::Searchable;
    use serde::Serialize;
    use uuid::Uuid;

    /// Stored as a number (`int_enum`), serialized as its name.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
    pub enum Priority {
        Urgent,
        #[default]
        Normal,
    }

    impl From<Priority> for i64 {
        fn from(priority: Priority) -> i64 {
            match priority {
                Priority::Urgent => 1,
                Priority::Normal => 3,
            }
        }
    }

    impl TryFrom<i64> for Priority {
        type Error = String;

        fn try_from(value: i64) -> Result<Self, String> {
            match value {
                1 => Ok(Priority::Urgent),
                3 => Ok(Priority::Normal),
                other => Err(format!("no priority {other}")),
            }
        }
    }

    #[derive(Model, Searchable, Clone, Debug, Default, PartialEq, Serialize)]
    #[model(table = "tickets")]
    pub struct Ticket {
        pub id: i64,
        #[model(cast = "int_enum")]
        #[searchable(boost = 2, filter)]
        pub priority: Priority,
        #[model(cast = "int_enum")]
        #[searchable]
        pub label: Priority,
        #[model(column = "assignee")]
        #[searchable(filter)]
        pub owner: Option<Uuid>,
        /// Not a column: a text field only.
        #[model(skip)]
        #[searchable(rename = "summary")]
        pub computed: String,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn filter_fields_hold_database_values_and_text_fields_serde_values() {
    use ticket::{Priority, Ticket};

    let settings = Ticket::settings();
    assert_eq!(settings.index(), "tickets");
    let owner = settings.field("owner").expect("indexed by its field name");
    assert_eq!(owner.column, "assignee", "#[model(column)] is the column");
    assert_eq!(settings.field_for_column("assignee").unwrap().name, "owner");
    assert_eq!(settings.field("summary").unwrap().boost, Some(1));

    let app = crate::support::app("memory", "off").await;
    let ctx = search_context(&app).await;
    let assignee = Uuid::now_v7();
    let record = Record::from(Ticket {
        id: 7,
        priority: Priority::Urgent,
        label: Priority::Normal,
        owner: Some(assignee),
        computed: "printer on fire".into(),
    });
    let document = Ticket::document(&record, &ctx).await.unwrap().unwrap();
    assert_eq!(document.id(), "7");
    // Access rules compare `priority = 1`, the stored value.
    assert_eq!(document.get("priority"), Some(&json!(1)));
    assert_eq!(document.get("label"), Some(&json!("Normal")));
    assert_eq!(document.get("owner"), Some(&json!(assignee.to_string())));
    assert_eq!(document.get("summary"), Some(&json!("printer on fire")));
}

/// A search context on a unit of work of the app's database.
async fn search_context(app: &navree_testing::TestApp) -> SearchContext {
    let db = app.get::<navree_db::Db>().await;
    SearchContext::new(db.uow(None, None).await, app.ctx().clone(), None)
}

mod note {
    use navree_orm::Model;
    use navree_search::Searchable;
    use serde::Serialize;
    use uuid::Uuid;

    /// Scoped to the current workspace by the ORM (`#[model(workspace)]`),
    /// without `WorkspaceScoped`.
    #[derive(Model, Searchable, Clone, Debug, Default, PartialEq, Serialize)]
    #[model(table = "notes", workspace = "team_id")]
    pub struct Note {
        pub id: i64,
        pub team_id: Uuid,
        #[searchable]
        pub body: String,
    }
}

#[test]
fn the_derive_finds_the_workspace_column_of_the_model() {
    let settings = note::Note::settings();
    assert_eq!(settings.workspace_column(), Some("team_id"));
    assert_eq!(
        Deal::settings().workspace_column(),
        None,
        "a model without a workspace"
    );
}
