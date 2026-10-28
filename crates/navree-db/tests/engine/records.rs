//! Records: typed rows on navree-db's statements alone (`navree_db::record`),
//! on every database: inserts, reads with filters and pages, updates by
//! key and by filter, a claim that only one of two racing writers wins,
//! upserts, deletes, change events through a unit of work, and where
//! `Db::read_executor` reads.

use chrono::{DateTime, Duration, SubsecRound, Utc};
use navree_db::record::{self, Filter};
use navree_db::{Backend, ChangeOp, Db, ErrorKind, PageQuery, Record};
use serde::Serialize;
use uuid::Uuid;

use crate::support::{Scratch, script};

navree_db::record! {
    /// A contact.
    #[derive(Clone, Debug, PartialEq, Serialize)]
    pub struct Contact in "contacts" {
        pub id: Uuid,
        pub workspace_id: Option<Uuid>,
        pub email: String,
        pub name: String,
        pub active: bool,
        pub visits: i32,
        pub tags: serde_json::Value,
        #[serde(skip_serializing)]
        pub secret: String,
        pub seen_at: Option<DateTime<Utc>>,
        pub created_at: DateTime<Utc>,
    }
}

async fn contacts(db: &Db, backend: Backend) {
    let (uuid, json, time) = match backend {
        Backend::Postgres => ("UUID", "JSON", "TIMESTAMP WITH TIME ZONE"),
        Backend::MySql => ("BINARY(16)", "JSON", "TIMESTAMP(6)"),
        Backend::Sqlite => ("VARCHAR", "TEXT", "DATETIME"),
    };
    script(
        db,
        &format!(
            "CREATE TABLE contacts (
               id {uuid} NOT NULL PRIMARY KEY,
               workspace_id {uuid} NULL,
               email VARCHAR(191) NOT NULL,
               name VARCHAR(100) NOT NULL,
               active BOOLEAN NOT NULL,
               visits INTEGER NOT NULL,
               tags {json} NOT NULL,
               secret TEXT NOT NULL,
               seen_at {time} NULL,
               created_at {time} NOT NULL,
               UNIQUE (email)
             )"
        ),
    )
    .await;
}

fn now() -> DateTime<Utc> {
    // Microseconds: what MySQL's TIMESTAMP(6) and PostgreSQL keep.
    Utc::now().trunc_subsecs(6)
}

fn contact(email: &str, workspace: Option<Uuid>, at: DateTime<Utc>) -> Contact {
    Contact {
        id: Uuid::now_v7(),
        workspace_id: workspace,
        email: email.to_owned(),
        name: email.split('@').next().unwrap_or_default().to_owned(),
        active: true,
        visits: 0,
        tags: serde_json::json!(["lead"]),
        secret: "sealed".to_owned(),
        seen_at: None,
        created_at: at,
    }
}

/// Inserts, reads, updates, upserts and deletes round-trip every type.
pub async fn records_round_trip(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    contacts(&db, backend).await;
    let workspace = Uuid::now_v7();
    let start = now();

    let ada = contact("ada@example.com", Some(workspace), start);
    record::insert(&db, &ada).await.unwrap();
    let found = record::find::<Contact, _>(&db, ada.id).await.unwrap();
    assert_eq!(found.as_ref(), Some(&ada), "every column reads back");
    assert!(
        record::find::<Contact, _>(&db, Uuid::now_v7())
            .await
            .unwrap()
            .is_none()
    );

    // Ten more, one a second apart, half in no workspace.
    for n in 0..10 {
        let workspace = (n % 2 == 0).then_some(workspace);
        let row = contact(
            &format!("c{n}@example.com"),
            workspace,
            start + Duration::seconds(n + 1),
        );
        record::insert(&db, &row).await.unwrap();
    }
    let duplicate = record::insert(&db, &contact("ada@example.com", None, start))
        .await
        .unwrap_err();
    assert_eq!(duplicate.kind(), ErrorKind::UniqueViolation);

    // Filters: `eq` with `None` is `IS NULL`, groups, `IN`, comparisons.
    let unscoped = Contact::select().eq("workspace_id", None::<Uuid>);
    assert_eq!(unscoped.count(&db).await.unwrap(), 5);
    let visible = Contact::select().filter(
        Filter::any()
            .is_null("workspace_id")
            .eq("workspace_id", workspace),
    );
    assert_eq!(visible.count(&db).await.unwrap(), 11);
    let some = Contact::select()
        .filter(Filter::all().is_in("email", ["ada@example.com", "c3@example.com", "nobody@x"]))
        .order_by("email")
        .all(&db)
        .await
        .unwrap();
    assert_eq!(
        some.iter().map(|c| c.email.as_str()).collect::<Vec<_>>(),
        ["ada@example.com", "c3@example.com"]
    );
    assert_eq!(
        Contact::select()
            .filter(Filter::all().is_in::<&str>("email", []))
            .count(&db)
            .await
            .unwrap(),
        0
    );
    let later = Contact::select()
        .filter(Filter::all().compare(
            "created_at",
            navree_db::Compare::Gt,
            start + Duration::seconds(8),
        ))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(later, 2);

    // Pages, newest first.
    let page = Contact::select()
        .order_by_desc("created_at")
        .order_by_desc("id")
        .page(&db, PageQuery::new(2, 4))
        .await
        .unwrap();
    assert_eq!(page.meta.total, 11);
    assert_eq!(page.meta.last_page, 3);
    assert_eq!(
        page.data
            .iter()
            .map(|c| c.email.as_str())
            .collect::<Vec<_>>(),
        [
            "c5@example.com",
            "c4@example.com",
            "c3@example.com",
            "c2@example.com"
        ]
    );
    let offset_only = Contact::select()
        .order_by("created_at")
        .offset(9)
        .all(&db)
        .await
        .unwrap();
    assert_eq!(offset_only.len(), 2);

    // Every column by key.
    let mut changed = ada.clone();
    changed.name = "Ada Lovelace".to_owned();
    changed.active = false;
    changed.visits = 3;
    changed.tags = serde_json::json!({"vip": true});
    changed.seen_at = Some(start + Duration::milliseconds(1500));
    changed.workspace_id = None;
    assert_eq!(record::update(&db, &changed).await.unwrap(), 1);
    assert_eq!(
        record::find::<Contact, _>(&db, ada.id).await.unwrap(),
        Some(changed.clone())
    );
    assert_eq!(
        record::update(&db, &contact("ghost@example.com", None, start))
            .await
            .unwrap(),
        0,
        "no row has its key"
    );

    // Set-based: one column of the matching rows.
    let touched = Contact::update_where(Filter::all().eq("workspace_id", workspace))
        .set("visits", 7)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(touched, 5);
    assert_eq!(
        Contact::select().eq("visits", 7).count(&db).await.unwrap(),
        5
    );
    let typo = Contact::update_where(Filter::all())
        .set("vistis", 1)
        .execute(&db)
        .await
        .unwrap_err();
    assert_eq!(typo.kind(), ErrorKind::InvalidStatement);

    // Upserts by the unique email: a new row, then an update of two
    // columns of the existing one (its id and other columns stay).
    let grace = contact("grace@example.com", None, start);
    record::upsert(&db, &grace, &["email"], &["name", "visits"])
        .await
        .unwrap();
    let mut again = contact("grace@example.com", Some(workspace), start);
    again.name = "Grace Hopper".to_owned();
    again.visits = 9;
    record::upsert(&db, &again, &["email"], &["name", "visits"])
        .await
        .unwrap();
    let stored = Contact::select()
        .eq("email", "grace@example.com")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.id, grace.id);
    assert_eq!(stored.workspace_id, None);
    assert_eq!((stored.name.as_str(), stored.visits), ("Grace Hopper", 9));
    record::upsert(
        &db,
        &contact("grace@example.com", None, start),
        &["email"],
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        Contact::select()
            .eq("email", "grace@example.com")
            .count(&db)
            .await
            .unwrap(),
        1,
        "an upsert that updates nothing keeps the row"
    );

    // Deletes by key and by filter.
    assert_eq!(record::delete(&db, &stored).await.unwrap(), 1);
    assert_eq!(record::delete(&db, &stored).await.unwrap(), 0);
    let removed = Contact::delete_where(Filter::all().is_null("workspace_id"))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(removed, 6);
    assert_eq!(Contact::select().count(&db).await.unwrap(), 5);
}

/// Through a unit of work: writes commit or roll back with it, record
/// change events (without the fields `Serialize` skips), and a claim
/// (`UPDATE .. WHERE status = ..`) lets only one of two units of work win.
pub async fn records_in_units_of_work(scratch: Scratch) {
    for native in [false, true] {
        let db = scratch.db().await.with_native_units_of_work(native);
        let backend = scratch.server.backend();
        script(&db, "DROP TABLE IF EXISTS contacts").await;
        contacts(&db, backend).await;
        let start = now();

        let uow = db.uow(None, None).await;
        let ada = contact("ada@example.com", None, start);
        record::insert_recorded(&uow, &ada).await.unwrap();
        let mut renamed = ada.clone();
        renamed.name = "Ada Lovelace".to_owned();
        renamed.secret = "resealed".to_owned();
        record::update_recorded(&uow, &ada, &renamed).await.unwrap();
        record::update_recorded(&uow, &renamed, &renamed)
            .await
            .unwrap();
        let changes = uow.changes();
        assert_eq!(
            changes.len(),
            2,
            "an update that changes nothing records nothing"
        );
        assert_eq!(changes[0].op, ChangeOp::Created);
        assert_eq!(changes[0].entity, "contacts");
        assert_eq!(changes[0].id, ada.id.to_string());
        let created = changes[0].new.as_ref().unwrap();
        assert_eq!(created["email"], "ada@example.com");
        assert!(created.get("secret").is_none(), "skipped fields stay out");
        assert_eq!(changes[1].op, ChangeOp::Updated);
        assert_eq!(
            changes[1].new.as_ref().unwrap(),
            &serde_json::json!({"name": "Ada Lovelace"})
        );
        // Reads in the unit of work see its writes.
        assert!(
            record::find::<Contact, _>(&uow, ada.id)
                .await
                .unwrap()
                .is_some()
        );
        uow.rollback().await.unwrap();
        assert!(
            record::find::<Contact, _>(&db, ada.id)
                .await
                .unwrap()
                .is_none(),
            "rolled back with the unit of work"
        );

        let uow = db.uow(None, None).await;
        record::insert_recorded(&uow, &ada).await.unwrap();
        uow.commit().await.unwrap();
        let ghost = contact("ghost@example.com", None, start);
        let uow = db.uow(None, None).await;
        let missing = record::update_recorded(&uow, &ghost, &ghost)
            .await
            .unwrap_err();
        assert_eq!(missing.kind(), ErrorKind::NotFound);
        let missing = record::delete_recorded(&uow, &ghost).await.unwrap_err();
        assert_eq!(missing.kind(), ErrorKind::NotFound);
        record::delete_recorded(&uow, &ada).await.unwrap();
        assert_eq!(uow.changes()[0].op, ChangeOp::Deleted);
        uow.commit().await.unwrap();
        assert_eq!(Contact::select().count(&db).await.unwrap(), 0);

        // Two units of work claim the same row; the second waits for the
        // first's lock (PostgreSQL, MySQL) or its write lock (SQLite) and
        // then matches nothing.
        let row = contact("claim@example.com", None, start);
        record::insert(&db, &row).await.unwrap();
        let claim = |db: Db, id: Uuid| async move {
            let uow = db.uow(None, None).await;
            let claimed = Contact::update_where(Filter::all().eq("id", id).eq("active", true))
                .set("active", false)
                .execute(&uow)
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            uow.commit().await.unwrap();
            claimed
        };
        let (a, b) = tokio::join!(
            tokio::spawn(claim(db.clone(), row.id)),
            tokio::spawn(claim(db.clone(), row.id))
        );
        let mut claimed = [a.unwrap(), b.unwrap()];
        claimed.sort_unstable();
        assert_eq!(claimed, [0, 1], "exactly one claim wins (native: {native})");

        // `FOR UPDATE` reads inside a unit of work.
        let uow = db.uow(None, None).await;
        let locked = Contact::select()
            .eq("id", row.id)
            .for_update()
            .one(&uow)
            .await
            .unwrap();
        assert!(locked.is_some_and(|c| !c.active));
        uow.commit().await.unwrap();
    }
}

/// `Db::read_executor` reads in the task's open unit of work (seeing its
/// writes), and on the pool otherwise.
pub async fn read_executor_follows_the_unit_of_work(scratch: Scratch) {
    let db = scratch.db().await;
    contacts(&db, scratch.server.backend()).await;
    let ada = contact("ada@example.com", None, now());
    let uow = db.uow(None, None).await;
    let inner = db.clone();
    let row = ada.clone();
    uow.scope(async move {
        record::insert(&inner, &row).await.unwrap();
        let seen = record::find::<Contact, _>(&inner.read_executor().await, row.id)
            .await
            .unwrap();
        assert!(seen.is_some(), "the open unit of work's write is visible");
    })
    .await;
    uow.rollback().await.unwrap();
    assert!(
        record::find::<Contact, _>(&db.read_executor().await, ada.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(db.same_as(&db.clone()));
    assert!(!db.same_as(&scratch.db().await));
}
