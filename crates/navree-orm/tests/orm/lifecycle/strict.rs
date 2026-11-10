//! Strict modes, chosen for a task with `strict::scope` (apps choosing
//! them through `[orm]` are tested in `tests/strict_apps.rs`, a process
//! of their own: an app installs its strictness for the whole process).

use std::sync::{Arc, Mutex};

use navree_orm::strict::{self, Strictness, Violation};
use navree_orm::{Model, OrmError, Record};

use super::models::{Level, Probe, create_tables};
use crate::support::sqlite;

async fn seeded() -> crate::support::TestDb {
    let t = sqlite().await;
    create_tables(&t.db).await;
    Probe::create(
        &t.db,
        Probe {
            id: 0,
            name: "north".into(),
            reading: 21,
            note: Some("calibrated".into()),
            level: Level::High,
        },
    )
    .await
    .unwrap();
    t
}

#[tokio::test]
async fn a_partial_select_is_an_error_in_strict_mode() {
    let t = seeded().await;
    let error = Probe::query()
        .select(["id", "name"])
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(
        matches!(error, OrmError::MissingAttribute { .. }),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "cannot read column `reading` of Probe as i64: the row has no such column (it has: id, name); select it, or mark the field #[model(default)]"
    );
}

#[tokio::test]
async fn lenient_mode_reads_the_defaults_of_left_out_columns() {
    let t = seeded().await;
    let slim = strict::scope(
        Strictness::lenient(),
        Probe::query().select(["id", "name", "level"]).get(&t.db),
    )
    .await
    .unwrap();
    assert_eq!(slim[0].name, "north");
    assert_eq!(slim[0].reading, 0, "i64's default");
    assert_eq!(slim[0].note, None);
    assert_eq!(slim[0].level, Level::High);

    // A field without a default (an enum without `Default`) is an error
    // whatever the mode.
    let error = strict::scope(
        Strictness::lenient(),
        Probe::query().select(["id", "name"]).get(&t.db),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("cannot read column `level` of Probe as String"),
        "{error}"
    );
}

#[tokio::test]
async fn a_handler_decides_about_missing_attributes() {
    let t = seeded().await;
    let reported = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = reported.clone();
    let logging = Strictness::strict().on_missing_attribute(move |violation| {
        seen.lock().unwrap().push(violation.to_string());
        Ok(())
    });
    let probes = strict::scope(
        logging,
        Probe::query().select(["id", "name", "level"]).get(&t.db),
    )
    .await
    .unwrap();
    assert_eq!(probes[0].reading, 0);
    assert_eq!(
        *reported.lock().unwrap(),
        [
            "column `reading` of Probe is missing from the row",
            "column `note` of Probe is missing from the row",
        ]
    );

    let refusing = Strictness::strict()
        .on_missing_attribute(|violation| Err(OrmError::InvalidQuery(format!("no: {violation}"))));
    let error = strict::scope(
        refusing,
        Probe::query().select(["id", "name", "level"]).get(&t.db),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: no: column `reading` of Probe is missing from the row"
    );
}

#[tokio::test]
async fn whole_rows_are_unaffected_and_defaulted_fields_always_default() {
    let t = seeded().await;
    let probes = Probe::all(&t.db).await.unwrap();
    assert_eq!(probes[0].reading, 21);
    // `#[model(default)]` fields read their default in every mode (core's
    // `Account::score`).
    let account = strict::scope(Strictness::strict(), async {
        crate::models::Account::from_row(&navree_orm::Row::from_values(vec![
            ("account_id".into(), navree_orm::Value::I32(1)),
            ("display_name".into(), navree_orm::Value::from("Anu")),
        ]))
    })
    .await
    .unwrap();
    assert_eq!(account.score, 0);
}

#[test]
fn discarded_attributes_are_errors_unless_tolerated() {
    let violation = Violation::DiscardedAttributes {
        model: "Probe",
        columns: vec!["reading".into()],
    };
    let error = strict::current().check(violation.clone()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot mass-assign `reading` on Probe: not fillable; add it to #[model(fillable(..))], or use force_fill"
    );
    assert!(Strictness::lenient().check(violation).is_ok());
    let record = Record::new(Probe {
        id: 0,
        name: "unsaved".into(),
        reading: 1,
        note: None,
        level: Level::Low,
    });
    assert!(!record.exists());
}
