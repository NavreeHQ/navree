//! Relationships on real servers: eager loads (batched, nested, limited
//! per parent), existence and aggregate subqueries, pivots, through and
//! one-of-many relations, polymorphic relations and touches, on tables of
//! their own (`rel_*`).

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use navree_db::Db;
use navree_db::Executor as _;
use navree_orm::prelude::*;
use navree_orm::relations::{
    BelongsTo, BelongsToMany, HasMany, HasManyThrough, HasOne, MorphMany, MorphTo, MorphTypes,
};
use serde::Serialize;
use uuid::Uuid;

use crate::support::Server;

/// PostgreSQL / TimescaleDB tables.
const POSTGRES: &str = r#"
CREATE TABLE rel_teams (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL
);
CREATE TABLE rel_users (
    id BIGSERIAL PRIMARY KEY,
    team_id BIGINT NULL,
    name TEXT NOT NULL,
    deleted_at TIMESTAMPTZ NULL
);
CREATE TABLE rel_companies (
    id BIGSERIAL PRIMARY KEY,
    parent_id BIGINT NULL,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE TABLE rel_deals (
    id UUID PRIMARY KEY,
    company_id BIGINT NULL,
    owner_id BIGINT NULL,
    title TEXT NOT NULL,
    amount_cents BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ NULL
);
CREATE TABLE rel_activities (
    id BIGSERIAL PRIMARY KEY,
    deal_id UUID NULL,
    kind TEXT NOT NULL,
    minutes BIGINT NOT NULL,
    due_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE TABLE rel_tags (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE TABLE rel_deal_tag (
    deal_id UUID NOT NULL,
    tag_id BIGINT NOT NULL,
    role TEXT NULL,
    created_at TIMESTAMPTZ NULL,
    updated_at TIMESTAMPTZ NULL,
    PRIMARY KEY (deal_id, tag_id)
);
CREATE TABLE rel_notes (
    id BIGSERIAL PRIMARY KEY,
    notable_type TEXT NULL,
    notable_id BIGINT NULL,
    body TEXT NOT NULL
)
"#;

/// MySQL tables.
const MYSQL: &str = r#"
CREATE TABLE rel_teams (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL
) ENGINE = InnoDB;
CREATE TABLE rel_users (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    team_id BIGINT NULL,
    name VARCHAR(255) NOT NULL,
    deleted_at DATETIME(6) NULL
) ENGINE = InnoDB;
CREATE TABLE rel_companies (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    parent_id BIGINT NULL,
    name VARCHAR(255) NOT NULL,
    created_at DATETIME(6) NOT NULL,
    updated_at DATETIME(6) NOT NULL
) ENGINE = InnoDB;
CREATE TABLE rel_deals (
    id BINARY(16) PRIMARY KEY,
    company_id BIGINT NULL,
    owner_id BIGINT NULL,
    title VARCHAR(255) NOT NULL,
    amount_cents BIGINT NOT NULL,
    created_at DATETIME(6) NOT NULL,
    updated_at DATETIME(6) NOT NULL,
    deleted_at DATETIME(6) NULL
) ENGINE = InnoDB;
CREATE TABLE rel_activities (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    deal_id BINARY(16) NULL,
    kind VARCHAR(32) NOT NULL,
    minutes BIGINT NOT NULL,
    due_at DATETIME(6) NULL,
    created_at DATETIME(6) NOT NULL,
    updated_at DATETIME(6) NOT NULL
) ENGINE = InnoDB;
CREATE TABLE rel_tags (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(64) NOT NULL UNIQUE,
    updated_at DATETIME(6) NOT NULL
) ENGINE = InnoDB;
CREATE TABLE rel_deal_tag (
    deal_id BINARY(16) NOT NULL,
    tag_id BIGINT NOT NULL,
    role VARCHAR(32) NULL,
    created_at DATETIME(6) NULL,
    updated_at DATETIME(6) NULL,
    PRIMARY KEY (deal_id, tag_id)
) ENGINE = InnoDB;
CREATE TABLE rel_notes (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    notable_type VARCHAR(32) NULL,
    notable_id BIGINT NULL,
    body TEXT NOT NULL
) ENGINE = InnoDB
"#;

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "rel_teams")]
pub struct Team {
    pub id: i64,
    pub name: String,
}

#[relations]
impl Team {
    pub fn users(&self) -> HasMany<Team, User> {
        self.has_many(User::TEAM_ID)
    }

    pub fn deals(&self) -> HasManyThrough<Team, Deal> {
        self.has_many_through::<Deal, User, _, _>(User::TEAM_ID, Deal::OWNER_ID)
    }

    pub fn notes(&self) -> MorphMany<Team, Note> {
        self.morph_many("notable")
    }
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "rel_users", soft_deletes)]
pub struct User {
    pub id: i64,
    pub team_id: Option<i64>,
    pub name: String,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[relations]
impl User {
    pub fn deals(&self) -> HasMany<User, Deal> {
        self.has_many(Deal::OWNER_ID)
    }
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "rel_companies", timestamps)]
pub struct Company {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[relations]
impl Company {
    pub fn subsidiaries(&self) -> HasMany<Company, Company> {
        self.has_many(Company::PARENT_ID)
    }

    pub fn deals(&self) -> HasMany<Company, Deal> {
        self.has_many(Deal::COMPANY_ID)
    }

    pub fn notes(&self) -> MorphMany<Company, Note> {
        self.morph_many("notable")
    }
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "rel_deals", timestamps, soft_deletes)]
pub struct Deal {
    pub id: Uuid,
    pub company_id: Option<i64>,
    pub owner_id: Option<i64>,
    pub title: String,
    pub amount_cents: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[relations]
impl Deal {
    pub fn company(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
    }

    pub fn activities(&self) -> HasMany<Deal, Activity> {
        self.has_many(Activity::DEAL_ID)
    }

    pub fn latest_activity(&self) -> HasOne<Deal, Activity> {
        self.has_many(Activity::DEAL_ID).latest_of_many()
    }

    pub fn tags(&self) -> BelongsToMany<Deal, Tag> {
        self.belongs_to_many("rel_deal_tag")
            .with_pivot(["role"])
            .with_timestamps()
    }
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "rel_activities", timestamps, touches = ["deal"])]
pub struct Activity {
    pub id: i64,
    pub deal_id: Option<Uuid>,
    pub kind: String,
    pub minutes: i64,
    pub due_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[relations]
impl Activity {
    pub fn deal(&self) -> BelongsTo<Activity, Deal> {
        self.belongs_to(Activity::DEAL_ID)
    }
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "rel_tags", timestamps(created_at = false))]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub updated_at: DateTime<Utc>,
}

#[relations]
impl Tag {
    pub fn deals(&self) -> BelongsToMany<Tag, Deal> {
        self.belongs_to_many("rel_deal_tag").with_pivot(["role"])
    }
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "rel_notes")]
pub struct Note {
    pub id: i64,
    pub notable_type: Option<String>,
    pub notable_id: Option<i64>,
    pub body: String,
}

#[relations]
impl Note {
    pub fn notable(&self) -> MorphTo<Note> {
        self.morph_to("notable").targets::<(Company, Team)>()
    }
}

fn titles(deals: &Collection<Deal>) -> Vec<String> {
    deals.iter().map(|deal| deal.title.clone()).collect()
}

/// Creates the relation tables.
async fn create_tables(db: &Db, server: Server) {
    let script = match server {
        Server::Postgres => POSTGRES,
        Server::MySql => MYSQL,
    };
    for statement in script.split(";\n").map(str::trim).filter(|s| !s.is_empty()) {
        db.pool()
            .execute_script(statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// Relationships end to end on `server`.
pub async fn relations(db: Db, server: Server) {
    create_tables(&db, server).await;
    let db = &db;
    let team = Team::create(
        db,
        Team {
            name: "Enterprise".into(),
            ..Team::default()
        },
    )
    .await
    .unwrap();
    let ada = User::create(
        db,
        User {
            team_id: Some(team.id),
            name: "Ada".into(),
            ..User::default()
        },
    )
    .await
    .unwrap();
    let bob = User::create(
        db,
        User {
            team_id: Some(team.id),
            name: "Bob".into(),
            ..User::default()
        },
    )
    .await
    .unwrap();
    let acme = Company::create(
        db,
        Company {
            name: "Acme".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    Company::create(
        db,
        Company {
            parent_id: Some(acme.id),
            name: "Acme Labs".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    let deal = |title: &str, cents: i64, company: Option<i64>, owner: i64| Deal {
        company_id: company,
        owner_id: Some(owner),
        title: title.to_owned(),
        amount_cents: cents,
        ..Deal::default()
    };
    let renewal = Deal::create(db, deal("Renewal", 5_000, Some(acme.id), ada.id))
        .await
        .unwrap();
    let upgrade = Deal::create(db, deal("Upgrade", 25_000, Some(acme.id), bob.id))
        .await
        .unwrap();
    let platform = Deal::create(db, deal("Platform", 90_000, None, ada.id))
        .await
        .unwrap();
    let due = |hour: u32| {
        use chrono::TimeZone;
        Some(Utc.with_ymd_and_hms(2026, 3, 2, hour, 0, 0).unwrap())
    };
    for (deal_id, kind, minutes, due_at) in [
        (renewal.id, "call", 30, due(9)),
        (renewal.id, "email", 5, None),
        (renewal.id, "call", 45, due(11)),
        (upgrade.id, "meeting", 60, due(10)),
    ] {
        Activity::create(
            db,
            Activity {
                deal_id: Some(deal_id),
                kind: kind.into(),
                minutes,
                due_at,
                ..Activity::default()
            },
        )
        .await
        .unwrap();
    }

    // Eager loads: belongs-to, has-many, nested.
    let deals = Deal::query()
        .with("company.subsidiaries")
        .with("activities")
        .order_by(Deal::AMOUNT_CENTS)
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&deals), ["Renewal", "Upgrade", "Platform"]);
    let company = deals[0].loaded(Deal::company).unwrap().as_ref().unwrap();
    assert_eq!(company.name, "Acme");
    assert_eq!(company.loaded(Company::subsidiaries).unwrap().len(), 1);
    assert!(deals[2].loaded(Deal::company).unwrap().is_none());
    assert_eq!(deals[0].loaded(Deal::activities).unwrap().len(), 3);

    // A limit per parent (a window), NULLs last on every server.
    let deals = Deal::query()
        .with_constrained(Deal::activities, |q| {
            q.order(Activity::DUE_AT.desc().nulls_last()).limit(2)
        })
        .order_by(Deal::AMOUNT_CENTS)
        .get(db)
        .await
        .unwrap();
    let minutes: Vec<i64> = deals[0]
        .loaded(Deal::activities)
        .unwrap()
        .iter()
        .map(|activity| activity.minutes)
        .collect();
    assert_eq!(minutes, [45, 30], "{server:?}");
    assert_eq!(deals[1].loaded(Deal::activities).unwrap().len(), 1);

    // Existence, counts and aggregates.
    let busy = Deal::query()
        .has_count("activities", ">=", 2)
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&busy), ["Renewal"]);
    let long_calls = Deal::query()
        .where_has(Deal::activities, |q| {
            q.where_(Activity::KIND.eq("call"))
                .where_(Activity::MINUTES.gt(40))
        })
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&long_calls), ["Renewal"]);
    let parents = Company::query().has("subsidiaries").get(db).await.unwrap();
    assert_eq!(parents.len(), 1);
    let counted = Deal::query()
        .with_count("activities")
        .with_sum("activities", Activity::MINUTES)
        .with_max("activities", Activity::MINUTES)
        .with_exists("company")
        .order_by(Deal::AMOUNT_CENTS)
        .get(db)
        .await
        .unwrap();
    assert_eq!(
        counted[0].extra::<i64>("activities_count").unwrap(),
        Some(3)
    );
    assert_eq!(
        counted[0].extra::<i64>("activities_sum_minutes").unwrap(),
        Some(80),
        "{server:?}"
    );
    assert_eq!(
        counted[0].extra::<i64>("activities_max_minutes").unwrap(),
        Some(45)
    );
    assert_eq!(
        counted[0].extra::<bool>("company_exists").unwrap(),
        Some(true)
    );
    assert_eq!(
        counted[2].extra::<bool>("company_exists").unwrap(),
        Some(false)
    );

    // One of many, and through relations.
    let latest = renewal.latest_activity().first(db).await.unwrap().unwrap();
    assert_eq!(latest.minutes, 45);
    let loaded = Deal::query()
        .with("latest_activity")
        .order_by(Deal::AMOUNT_CENTS)
        .get(db)
        .await
        .unwrap();
    assert_eq!(
        loaded[1]
            .loaded(Deal::latest_activity)
            .unwrap()
            .as_ref()
            .unwrap()
            .kind,
        "meeting"
    );
    assert_eq!(team.deals().count(db).await.unwrap(), 3);
    let teams = Team::query()
        .with("deals")
        .with_count("deals")
        .get(db)
        .await
        .unwrap();
    assert_eq!(teams[0].loaded(Team::deals).unwrap().len(), 3);
    assert_eq!(teams[0].extra::<i64>("deals_count").unwrap(), Some(3));

    // Pivots: attach, sync, toggle, pivot columns, existence.
    let vip = Tag::create(
        db,
        Tag {
            name: "vip".into(),
            ..Tag::default()
        },
    )
    .await
    .unwrap();
    let hot = Tag::create(
        db,
        Tag {
            name: "hot".into(),
            ..Tag::default()
        },
    )
    .await
    .unwrap();
    renewal
        .tags()
        .attach_with(db, [(vip.id, [col("role").set("owner")])])
        .await
        .unwrap();
    let changes = renewal.tags().sync(db, [vip.id, hot.id]).await.unwrap();
    assert_eq!(changes.attached_as::<i64>().unwrap(), [hot.id]);
    let tags = renewal.tags().order_by(Tag::ID).get(db).await.unwrap();
    assert_eq!(
        tags[0].pivot_value::<String>("role").unwrap().as_deref(),
        Some("owner")
    );
    assert!(
        tags[1]
            .pivot_value::<DateTime<Utc>>("created_at")
            .unwrap()
            .is_some()
    );
    let tagged = Deal::query().where_attached_to(&vip).get(db).await.unwrap();
    assert_eq!(titles(&tagged), ["Renewal"]);
    let toggled = renewal.tags().toggle(db, [vip.id]).await.unwrap();
    assert_eq!(toggled.detached_as::<i64>().unwrap(), [vip.id]);
    let with_tags = Tag::query()
        .with("deals")
        .order_by(Tag::ID)
        .get(db)
        .await
        .unwrap();
    assert!(with_tags[0].loaded(Tag::deals).unwrap().is_empty());
    assert_eq!(with_tags[1].loaded(Tag::deals).unwrap().len(), 1);
    // UUID parent keys travel bound, never inlined.
    upgrade.tags().attach(db, [hot.id]).await.unwrap();
    assert_eq!(hot.deals().count(db).await.unwrap(), 2);
    let deals = Tag::query()
        .with("deals")
        .where_key(hot.id)
        .get(db)
        .await
        .unwrap();
    assert_eq!(deals[0].loaded(Tag::deals).unwrap().len(), 2);

    // Polymorphic relations.
    acme.notes()
        .create(
            db,
            Note {
                body: "lunch".into(),
                ..Note::default()
            },
        )
        .await
        .unwrap();
    team.notes()
        .create(
            db,
            Note {
                body: "offsite".into(),
                ..Note::default()
            },
        )
        .await
        .unwrap();
    let notes = Note::query()
        .with("notable")
        .order_by(Note::ID)
        .get(db)
        .await
        .unwrap();
    let notable = |index: usize| notes[index].loaded(Note::notable).unwrap().clone().unwrap();
    assert_eq!(notable(0).downcast_ref::<Company>().unwrap().name, "Acme");
    assert_eq!(
        notable(1).downcast_ref::<Team>().unwrap().name,
        "Enterprise"
    );
    let about_companies = Note::query()
        .where_has_morph("notable", MorphTypes::of::<Company>(), |q, _| {
            q.where_(col("name").eq("Acme"))
        })
        .get(db)
        .await
        .unwrap();
    assert_eq!(about_companies.len(), 1);

    // Touching the parent when a child changes.
    let before = Deal::find_or_fail(db, upgrade.id).await.unwrap().updated_at;
    let mut meeting = upgrade.activities().first(db).await.unwrap().unwrap();
    meeting.minutes = 61;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    meeting.save(db).await.unwrap();
    let after = Deal::find_or_fail(db, upgrade.id).await.unwrap().updated_at;
    assert!(after > before, "{server:?}: {before} then {after}");

    // Soft-deleted intermediate records are left out of through relations.
    let mut bob = User::find_or_fail(db, bob.id).await.unwrap();
    bob.delete(db).await.unwrap();
    assert_eq!(team.deals().count(db).await.unwrap(), 2);
    // A NULL foreign key relates nothing.
    assert!(platform.company().first(db).await.unwrap().is_none());
}
