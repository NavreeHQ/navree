//! Soft deletes: trashing, restoring and force deleting, one by one and in
//! bulk, with their events.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_kernel::{Module, ModuleBuilder};
use navree_orm::hooks::Flow;
use navree_orm::{
    ExecContext, Model, ModelLifecycle, ModuleBuilderOrmExt, Observer, Record, Result,
};

use super::models::{Member, create_tables};
use crate::models::{Company, Deal};
use crate::support::{app, count_rows, sqlite};

/// What deal listeners heard, with `force` when the record said it was
/// being force deleted.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<String>>>);

impl Heard {
    fn push(&self, event: &str, deal: &Record<Deal>) {
        let force = if deal.is_force_deleting() {
            " force"
        } else {
            ""
        };
        self.0
            .lock()
            .unwrap()
            .push(format!("{event} {}{force}", deal.title));
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// Hears every deal event; refuses whatever the deal's title says.
struct DealObserver(Heard);

impl DealObserver {
    fn flow(&self, refuse: &str, event: &str, deal: &mut Record<Deal>) -> Result<Flow> {
        self.0.push(event, deal);
        Ok(if deal.title == refuse {
            Flow::Cancel
        } else {
            Flow::Continue
        })
    }
}

impl Observer<Deal> for DealObserver {
    async fn saving(&self, deal: &mut Record<Deal>, _ctx: &ExecContext) -> Result<Flow> {
        self.flow("no saving", "saving", deal)
    }

    async fn updating(&self, deal: &mut Record<Deal>, _ctx: &ExecContext) -> Result<Flow> {
        self.flow("no updating", "updating", deal)
    }

    async fn updated(&self, deal: &Record<Deal>, _ctx: &ExecContext) -> Result<()> {
        self.0.push("updated", deal);
        Ok(())
    }

    async fn saved(&self, deal: &Record<Deal>, _ctx: &ExecContext) -> Result<()> {
        self.0.push("saved", deal);
        Ok(())
    }

    async fn deleting(&self, deal: &mut Record<Deal>, _ctx: &ExecContext) -> Result<Flow> {
        self.flow("no deleting", "deleting", deal)
    }

    async fn deleted(&self, deal: &Record<Deal>, _ctx: &ExecContext) -> Result<()> {
        self.0.push("deleted", deal);
        Ok(())
    }

    async fn trashed(&self, deal: &Record<Deal>, _ctx: &ExecContext) -> Result<()> {
        self.0.push("trashed", deal);
        Ok(())
    }

    async fn force_deleting(&self, deal: &mut Record<Deal>, _ctx: &ExecContext) -> Result<Flow> {
        self.flow("no force deleting", "force_deleting", deal)
    }

    async fn force_deleted(&self, deal: &Record<Deal>, _ctx: &ExecContext) -> Result<()> {
        self.0.push("force_deleted", deal);
        Ok(())
    }

    async fn restoring(&self, deal: &mut Record<Deal>, _ctx: &ExecContext) -> Result<Flow> {
        self.flow("no restoring", "restoring", deal)
    }

    async fn restored(&self, deal: &Record<Deal>, _ctx: &ExecContext) -> Result<()> {
        self.0.push("restored", deal);
        Ok(())
    }
}

struct Observing(Heard);

impl Module for Observing {
    fn register(&self, m: &mut ModuleBuilder) {
        m.observe::<Deal>(DealObserver(self.0.clone()));
    }
}

#[tokio::test]
async fn trashing_fires_deleting_trashed_deleted() {
    let heard = Heard::default();
    let (_app, db, clock) = app(vec![Box::new(Observing(heard.clone()))]).await;
    let mut deal = Deal::create_quietly(&*db, Deal::new("Old", 1))
        .await
        .unwrap();

    clock.advance(Duration::from_secs(30));
    assert!(deal.delete(&*db).await.unwrap());
    assert_eq!(heard.take(), ["deleting Old", "trashed Old", "deleted Old"]);
    assert!(deal.trashed());
    assert!(deal.exists());
    assert_eq!(deal.deleted_at, Some(deal.updated_at));
    assert_eq!(count_rows(&db, "deals").await, 1, "the row stays");

    let mut kept = Deal::create_quietly(&*db, Deal::new("no deleting", 1))
        .await
        .unwrap();
    assert!(!kept.delete(&*db).await.unwrap());
    assert!(!kept.trashed());
}

#[tokio::test]
async fn force_deleting_removes_the_row_and_says_so() {
    let heard = Heard::default();
    let (_app, db, _clock) = app(vec![Box::new(Observing(heard.clone()))]).await;

    let mut live = Deal::create_quietly(&*db, Deal::new("Live", 1))
        .await
        .unwrap();
    assert!(live.force_delete(&*db).await.unwrap());
    assert_eq!(
        heard.take(),
        [
            "force_deleting Live",
            "deleting Live force",
            "deleted Live force",
            "force_deleted Live",
        ],
        "deleting and deleted know it is for good; trashed is not fired"
    );
    assert!(!live.exists());
    assert!(!live.is_force_deleting());
    assert_eq!(count_rows(&db, "deals").await, 0);

    // A trashed record is removed for good too.
    let mut trashed = Deal::create_quietly(&*db, Deal::new("Trashed", 1))
        .await
        .unwrap();
    trashed.delete_quietly(&*db).await.unwrap();
    assert!(trashed.force_delete_quietly(&*db).await.unwrap());
    assert!(heard.take().is_empty(), "quietly");
    assert_eq!(count_rows(&db, "deals").await, 0);

    // Either cancellable step keeps the row.
    for title in ["no force deleting", "no deleting"] {
        let mut kept = Deal::create_quietly(&*db, Deal::new(title, 1))
            .await
            .unwrap();
        assert!(!kept.force_delete(&*db).await.unwrap(), "{title}");
        assert!(kept.exists());
        assert!(!kept.is_force_deleting(), "{title}: the flag is reset");
    }
    assert_eq!(count_rows(&db, "deals").await, 2);
    heard.take();

    // A record that is not stored: nothing to do.
    let mut new = Record::new(Deal::new("New", 1));
    assert!(!new.force_delete(&*db).await.unwrap());
    assert!(heard.take().is_empty());
}

#[tokio::test]
async fn restoring_saves_the_cleared_column_between_its_events() {
    let heard = Heard::default();
    let (_app, db, clock) = app(vec![Box::new(Observing(heard.clone()))]).await;
    let mut deal = Deal::create_quietly(&*db, Deal::new("Back", 1))
        .await
        .unwrap();
    deal.delete_quietly(&*db).await.unwrap();
    let trashed_at = deal.updated_at;

    clock.advance(Duration::from_secs(60));
    assert!(deal.restore(&*db).await.unwrap());
    assert_eq!(
        heard.take(),
        [
            "restoring Back",
            "saving Back",
            "updating Back",
            "updated Back",
            "saved Back",
            "restored Back",
        ]
    );
    assert!(!deal.trashed());
    assert!(deal.updated_at > trashed_at, "a restore is an update");
    assert!(deal.is_clean());
    let stored = Deal::find(&*db, deal.id)
        .await
        .unwrap()
        .expect("visible again");
    assert_eq!(stored.deleted_at, None);

    // Quietly, and cancelled.
    deal.delete_quietly(&*db).await.unwrap();
    assert!(deal.restore_quietly(&*db).await.unwrap());
    assert!(heard.take().is_empty());
    let mut refused = Deal::create_quietly(&*db, Deal::new("no restoring", 1))
        .await
        .unwrap();
    refused.delete_quietly(&*db).await.unwrap();
    assert!(!refused.restore(&*db).await.unwrap());
    assert!(refused.trashed(), "a cancelled restore changes nothing");
    assert_eq!(Deal::query().only_trashed().count(&*db).await.unwrap(), 1);
}

#[tokio::test]
async fn queries_choose_between_live_and_trashed_rows() {
    let t = sqlite().await;
    for (title, trash) in [("A", false), ("B", true), ("C", true)] {
        let mut deal = Deal::create(&t.db, Deal::new(title, 1)).await.unwrap();
        if trash {
            deal.delete(&t.db).await.unwrap();
        }
    }
    let titles = |deals: navree_orm::Collection<Deal>| {
        let mut titles = deals.map(|d| d.title.clone());
        titles.sort();
        titles
    };
    assert_eq!(titles(Deal::all(&t.db).await.unwrap()), ["A"]);
    assert_eq!(
        titles(Deal::query().with_trashed().get(&t.db).await.unwrap()),
        ["A", "B", "C"]
    );
    assert_eq!(
        titles(
            Deal::query()
                .with_trashed_if(true)
                .get(&t.db)
                .await
                .unwrap()
        ),
        ["A", "B", "C"]
    );
    assert_eq!(
        titles(
            Deal::query()
                .with_trashed_if(false)
                .get(&t.db)
                .await
                .unwrap()
        ),
        ["A"]
    );
    assert_eq!(
        titles(
            Deal::query()
                .with_trashed()
                .without_trashed()
                .get(&t.db)
                .await
                .unwrap()
        ),
        ["A"]
    );
    assert_eq!(
        titles(Deal::query().only_trashed().get(&t.db).await.unwrap()),
        ["B", "C"]
    );
    assert_eq!(
        titles(
            Deal::query()
                .only_trashed()
                .where_(Deal::TITLE.eq("B"))
                .or_where(Deal::TITLE.eq("A"))
                .get(&t.db)
                .await
                .unwrap()
        ),
        ["B"],
        "the caller's `or` stays inside its group"
    );
}

#[tokio::test]
async fn mass_restores_touch_only_trashed_rows() {
    let (_app, db, clock) = app(Vec::new()).await;
    let mut created = Vec::new();
    for title in ["A", "B", "C"] {
        created.push(Deal::create(&*db, Deal::new(title, 1)).await.unwrap());
    }
    for deal in &mut created[1..] {
        deal.delete(&*db).await.unwrap();
    }
    clock.advance(Duration::from_secs(60));

    let restored = Deal::query()
        .where_(Deal::TITLE.ne("C"))
        .restore(&*db)
        .await
        .unwrap();
    assert_eq!(restored, 1, "B was trashed and matches; A was not trashed");
    let a = Deal::find_or_fail(&*db, created[0].id).await.unwrap();
    assert_eq!(
        a.updated_at, created[0].updated_at,
        "live rows are left alone"
    );
    let b = Deal::find_or_fail(&*db, created[1].id).await.unwrap();
    assert!(b.updated_at > created[1].updated_at);
    assert!(Deal::find(&*db, created[2].id).await.unwrap().is_none());

    assert_eq!(Deal::query().only_trashed().restore(&*db).await.unwrap(), 1);
    assert_eq!(Deal::query().count(&*db).await.unwrap(), 3);
}

#[tokio::test]
async fn mass_force_deletes_reach_trashed_rows() {
    let t = sqlite().await;
    for (title, cents, trash) in [
        ("A", 1, false),
        ("B", 1, true),
        ("C", 500, true),
        ("D", 500, false),
    ] {
        let mut deal = Deal::create(&t.db, Deal::new(title, cents)).await.unwrap();
        if trash {
            deal.delete(&t.db).await.unwrap();
        }
    }
    let removed = Deal::query()
        .where_(Deal::AMOUNT_CENTS.lt(100))
        .force_delete(&t.db)
        .await
        .unwrap();
    assert_eq!(removed, 2, "A and the trashed B");
    assert_eq!(count_rows(&t.db, "deals").await, 2);
    assert_eq!(
        Deal::query()
            .without_trashed()
            .force_delete(&t.db)
            .await
            .unwrap(),
        1,
        "D; C stays trashed"
    );
    assert_eq!(Deal::query().only_trashed().count(&t.db).await.unwrap(), 1);

    // A mass delete trashes, a mass force delete removes: no events.
    assert_eq!(
        Deal::query()
            .only_trashed()
            .force_delete(&t.db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(count_rows(&t.db, "deals").await, 0);
}

#[tokio::test]
async fn force_destroy_loads_and_removes_each_record() {
    let heard = Heard::default();
    let (_app, db, _clock) = app(vec![Box::new(Observing(heard.clone()))]).await;
    let a = Deal::create_quietly(&*db, Deal::new("A", 1)).await.unwrap();
    let mut b = Deal::create_quietly(&*db, Deal::new("B", 1)).await.unwrap();
    b.delete_quietly(&*db).await.unwrap();
    let c = Deal::create_quietly(&*db, Deal::new("C", 1)).await.unwrap();

    assert_eq!(Deal::force_destroy(&*db, [a.id, b.id]).await.unwrap(), 2);
    assert_eq!(
        heard.take(),
        [
            "force_deleting A",
            "deleting A force",
            "deleted A force",
            "force_deleted A",
            "force_deleting B",
            "deleting B force",
            "deleted B force",
            "force_deleted B",
        ]
    );
    assert_eq!(count_rows(&db, "deals").await, 1);
    assert_eq!(
        Deal::force_destroy(&*db, Vec::<uuid::Uuid>::new())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        Deal::destroy(&*db, [c.id]).await.unwrap(),
        1,
        "destroy trashes"
    );
    assert_eq!(count_rows(&db, "deals").await, 1);
}

#[tokio::test]
async fn models_without_soft_deletes_force_delete_plainly() {
    let t = sqlite().await;
    let mut acme = Company::create(
        &t.db,
        Company {
            name: "Acme".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    assert!(acme.force_delete(&t.db).await.unwrap());
    assert!(!acme.exists());
    let ids: Vec<i64> = {
        let mut ids = Vec::new();
        for name in ["A", "B"] {
            let company = Company::create(
                &t.db,
                Company {
                    name: name.into(),
                    ..Company::default()
                },
            )
            .await
            .unwrap();
            ids.push(company.id);
        }
        ids
    };
    assert_eq!(Company::force_destroy(&t.db, ids).await.unwrap(), 2);
    assert_eq!(count_rows(&t.db, "companies").await, 0);
    for name in ["C", "D"] {
        Company::create(
            &t.db,
            Company {
                name: name.into(),
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(
        Company::query()
            .with_trashed_if(false)
            .without_trashed()
            .force_delete(&t.db)
            .await
            .unwrap(),
        2,
        "the trashed filters do nothing without soft deletes"
    );
}

fn member(email: &str, name: &str) -> Member {
    Member {
        email: email.into(),
        name: name.into(),
        ..Member::default()
    }
}

#[tokio::test]
async fn restore_or_create_and_create_or_restore_bring_records_back() {
    let t = sqlite().await;
    create_tables(&t.db).await;
    let mut gone = Member::create(&t.db, member("ada@example.com", "Ada"))
        .await
        .unwrap();
    gone.delete(&t.db).await.unwrap();

    let back = Member::restore_or_create(&t.db, [Member::EMAIL.set("ada@example.com")], |m| {
        m.name = "Someone else".into();
    })
    .await
    .unwrap();
    assert_eq!(back.id, gone.id, "the trashed record, restored");
    assert!(!back.trashed());
    assert_eq!(back.name, "Ada", "found, so the values are not applied");

    let new = Member::restore_or_create(&t.db, [Member::EMAIL.set("grace@example.com")], |m| {
        m.name = "Grace".into();
    })
    .await
    .unwrap();
    assert!(new.was_recently_created());
    assert_eq!(new.name, "Grace");

    // Create first; the unique email finds the trashed record, restored.
    let mut again = Member::find_or_fail(&t.db, new.id).await.unwrap();
    again.delete(&t.db).await.unwrap();
    let restored =
        Member::create_or_restore(&t.db, [Member::EMAIL.set("grace@example.com")], |m| {
            m.name = "Grace".into();
        })
        .await
        .unwrap();
    assert_eq!(restored.id, new.id);
    assert!(!restored.trashed());
    let created = Member::query()
        .create_or_restore(&t.db, [Member::EMAIL.set("linus@example.com")], |m| {
            m.name = "Linus".into();
        })
        .await
        .unwrap();
    assert!(created.was_recently_created());
    assert_eq!(Member::query().count(&t.db).await.unwrap(), 3);
    assert_eq!(count_rows(&t.db, "members").await, 3);
}
