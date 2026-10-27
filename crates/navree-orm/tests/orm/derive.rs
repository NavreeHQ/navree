//! What `#[derive(Model)]` writes: metadata, column constants, and reading
//! and writing columns.

use chrono::{TimeZone, Utc};
use navree_orm::{KeyKind, Model, OrmError, Row, Ulid, Value};
use uuid::Uuid;

use crate::models::{Account, Article, Company, Deal, DealMeta, Event, Priority, Setting, Stage, Tag};

#[test]
fn metadata_names_the_table_key_columns_and_timestamps() {
    let meta = Deal::meta();
    assert_eq!(meta.name(), "Deal");
    assert_eq!(meta.table(), "deals");
    assert_eq!(meta.key_column(), "id");
    assert_eq!(meta.key_kind(), KeyKind::UuidV7);
    assert_eq!(meta.created_at(), Some("created_at"));
    assert_eq!(meta.updated_at(), Some("updated_at"));
    assert_eq!(meta.deleted_at(), Some("deleted_at"));
    assert!(meta.soft_deletes_enabled());
    assert_eq!(meta.default_per_page(), 2);
    let names: Vec<&str> = meta.all_columns().iter().map(|c| c.name()).collect();
    assert_eq!(
        names,
        [
            "id",
            "company_id",
            "title",
            "amount_cents",
            "stage",
            "priority",
            "meta",
            "tags",
            "probability",
            "value",
            "closes_on",
            "closed_at",
            "created_at",
            "updated_at",
            "deleted_at"
        ],
        "skipped fields are not columns"
    );
    let stage = meta.column("stage").unwrap();
    assert_eq!(stage.cast_name(), Some("string_enum"));
    assert_eq!(stage.rust_type(), "Stage");
    let closed_at = meta.column("closed_at").unwrap();
    assert!(closed_at.is_nullable());
    assert_eq!(closed_at.rust_type(), "Option<DateTime<Utc>>");
}

#[test]
fn defaults_follow_the_struct() {
    assert_eq!(Company::meta().table(), "companies");
    assert_eq!(Tag::meta().table(), "tags");
    assert_eq!(Article::meta().table(), "articles");
    assert_eq!(Company::meta().key_kind(), KeyKind::Increments);
    assert_eq!(Event::meta().key_kind(), KeyKind::Ulid);
    assert_eq!(Setting::meta().key_kind(), KeyKind::Manual);
    assert_eq!(Setting::meta().key_column(), "key");
    assert_eq!(Setting::meta().created_at(), None);
    assert_eq!(Setting::meta().updated_at(), Some("updated_at"));
    assert!(!Event::meta().uses_timestamps());
    assert_eq!(Company::meta().default_per_page(), 15);
}

#[test]
fn column_constants_carry_table_and_column() {
    assert_eq!(Deal::TITLE.name(), "title");
    assert_eq!(Deal::TITLE.table(), Some("deals"));
    assert_eq!(Account::NAME.name(), "display_name");
    assert_eq!(Account::ACCOUNT_ID.name(), "account_id");
    assert_eq!(Account::meta().key_column(), "account_id");
    assert_eq!(Account::meta().column("display_name").unwrap().field(), "name");
}

#[test]
fn typed_columns_encode_values_as_their_fields_are_stored() {
    assert_eq!(Deal::STAGE.encode(&Stage::ClosedWon).unwrap(), Value::from("closed_won"));
    assert_eq!(Deal::PRIORITY.encode(&Priority::High).unwrap(), Value::BigInt(Some(3)));
    assert_eq!(
        Deal::META
            .encode(&DealMeta {
                source: "web".into(),
                score: 4
            })
            .unwrap(),
        Value::Json(Some(Box::new(serde_json::json!({"source": "web", "score": 4}))))
    );
    assert_eq!(Deal::CLOSED_AT.encode(&None).unwrap(), Value::ChronoDateTimeUtc(None));
}

fn deal_row(stage: &str) -> Row {
    let at = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
    Row::from_values(vec![
        ("id".into(), Value::Uuid(Some(Uuid::nil()))),
        ("company_id".into(), Value::BigInt(None)),
        ("title".into(), Value::from("Renewal")),
        ("amount_cents".into(), Value::BigInt(Some(1200))),
        ("stage".into(), Value::from(stage)),
        ("priority".into(), Value::BigInt(Some(2))),
        ("meta".into(), Value::from(r#"{"source":"ads","score":7}"#)),
        ("tags".into(), Value::from(r#"["vip","q4"]"#)),
        ("probability".into(), Value::Double(Some(0.5))),
        ("value".into(), Value::from("10.25")),
        ("closes_on".into(), Value::from("2026-12-31")),
        ("closed_at".into(), Value::ChronoDateTimeUtc(None)),
        ("created_at".into(), Value::ChronoDateTimeUtc(Some(at))),
        ("updated_at".into(), Value::from("2026-01-02 03:04:05")),
        ("deleted_at".into(), Value::String(None)),
    ])
}

#[test]
fn rows_decode_into_models_through_their_casts() {
    let deal = Deal::from_row(&deal_row("qualified")).unwrap();
    assert_eq!(deal.title, "Renewal");
    assert_eq!(deal.stage, Stage::Qualified);
    assert_eq!(deal.priority, Priority::Normal);
    assert_eq!(deal.meta.score, 7);
    assert_eq!(deal.tags, ["vip", "q4"]);
    assert_eq!(deal.value.to_string(), "10.25");
    assert_eq!(deal.closes_on.unwrap().to_string(), "2026-12-31");
    assert_eq!(deal.created_at, deal.updated_at);
    assert_eq!(deal.company_id, None);
    assert_eq!(deal.scratch, "", "skipped fields start empty");
}

#[test]
fn decoding_errors_name_the_model_the_column_and_the_cause() {
    let error = Deal::from_row(&deal_row("won")).unwrap_err();
    assert!(matches!(error, OrmError::Cast { .. }), "{error:?}");
    assert_eq!(
        error.to_string(),
        "cannot cast column `stage` of Deal: `won` is not a variant of Stage"
    );

    let partial = Row::from_values(vec![("id".into(), Value::BigInt(Some(1)))]);
    let error = Company::from_row(&partial).unwrap_err().to_string();
    assert!(error.starts_with("cannot read column `name` of Company as String: the row has no such column (it has: id)"), "{error}");
    assert!(error.ends_with("select it, or mark the field #[model(default)]"), "{error}");
}

#[test]
fn defaulted_fields_read_their_default_when_the_row_lacks_them() {
    let row = Row::from_values(vec![
        ("account_id".into(), Value::Int(Some(4))),
        ("display_name".into(), Value::from("Ada")),
    ]);
    let account = Account::from_row(&row).unwrap();
    assert_eq!(account, Account { account_id: 4, name: "Ada".into(), score: 0 });
}

#[test]
fn values_and_set_value_round_trip() {
    let mut deal = Deal::from_row(&deal_row("lead")).unwrap();
    let values = deal.values().unwrap();
    assert_eq!(values.len(), 15);
    assert_eq!(values[4], ("stage", Value::from("lead")));
    assert_eq!(deal.value("priority").unwrap(), Some(Value::BigInt(Some(2))));
    assert_eq!(deal.value("scratch").unwrap(), None, "not a column");

    deal.set_value("stage", Value::from("closed_lost")).unwrap();
    deal.set_value("amount_cents", Value::Int(Some(7))).unwrap();
    assert_eq!(deal.stage, Stage::ClosedLost);
    assert_eq!(deal.amount_cents, 7);
    let error = deal.set_value("nope", Value::Int(Some(1))).unwrap_err();
    assert_eq!(error.to_string(), "invalid query: Deal has no column `nope`");
}

#[test]
fn keys_and_timestamps_are_reachable_by_column() {
    let mut event = Event {
        id: Ulid::nil(),
        ..Event::default()
    };
    assert!(event.key().is_nil());
    let id = Ulid::new();
    event.set_key(id);
    assert_eq!(event.id, id);
    assert!(event.touch_timestamp("occurred_at", Utc::now()).is_err(), "not a timestamp column");

    let mut deal = Deal::default();
    let now = Utc.with_ymd_and_hms(2026, 10, 27, 8, 0, 0).unwrap();
    assert!(deal.timestamp_is_empty("created_at"));
    deal.touch_timestamp("created_at", now).unwrap();
    assert_eq!(deal.created_at, now);
    assert!(!deal.timestamp_is_empty("created_at"));
    deal.clear_timestamp("created_at");
    assert!(deal.timestamp_is_empty("created_at"));
    assert_eq!(
        Deal::timestamp_value("deleted_at", now),
        Some(Value::ChronoDateTimeUtc(Some(now)))
    );
    assert_eq!(Deal::timestamp_value("title", now), None);
}

#[test]
fn global_scopes_come_from_the_attribute() {
    assert_eq!(Article::global_scopes().len(), 2);
    assert!(Deal::global_scopes().is_empty());
}
