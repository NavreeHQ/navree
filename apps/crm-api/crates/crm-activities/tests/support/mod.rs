//! Fixtures for the activities and dashboard tests: CRM records written
//! straight to the database (companies, contacts and deals belong to other
//! crates; these tests only need rows to attach activities to and count).

#![allow(dead_code)]

use chrono::{DateTime, TimeZone, Utc};
use crm_activities::{ActivitiesModule, DashboardModule};
use crm_core::entities::{activity, company, contact, deal};
use crm_core::{ActivityKind, ContactStatus, Stage, SubjectType};
use crm_testkit::TestCrm;
use keel::contracts::Clock;
use keel::db::Db;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue::Set;
use uuid::Uuid;

/// A CRM with the activities and dashboard modules, its clock at `now`.
pub async fn crm_at(now: DateTime<Utc>) -> TestCrm {
    TestCrm::builder()
        .module(ActivitiesModule)
        .module(DashboardModule)
        .now(now)
        .build()
        .await
}

/// A CRM with the activities and dashboard modules, its clock at
/// [`TestCrm::start`] (Monday 2026-10-26, 10:00 UTC).
pub async fn crm() -> TestCrm {
    crm_at(TestCrm::start()).await
}

/// `2026-10-26T10:00:00Z`-style UTC time.
pub fn at(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(time)
        .expect("an RFC 3339 time")
        .with_timezone(&Utc)
}

/// Midnight UTC of a date.
pub fn day(year: i32, month: u32, day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 0, 0, 0)
        .single()
        .expect("a valid date")
}

async fn db(crm: &TestCrm) -> std::sync::Arc<Db> {
    crm.get::<Db>().await
}

/// A company named `name`, owned by `owner`.
pub async fn company(crm: &TestCrm, owner: Uuid, name: &str) -> company::Model {
    let now = crm.clock().now();
    company::ActiveModel {
        id: Set(Uuid::now_v7()),
        name: Set(name.to_owned()),
        domain: Set(None),
        industry: Set(None),
        size: Set(None),
        phone: Set(None),
        website: Set(None),
        city: Set(None),
        country: Set(None),
        owner_id: Set(owner),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(None),
    }
    .insert(db(crm).await.conn())
    .await
    .expect("company inserted")
}

/// A soft-deleted company.
pub async fn deleted_company(crm: &TestCrm, owner: Uuid, name: &str) -> company::Model {
    let now = crm.clock().now();
    company::ActiveModel {
        id: Set(Uuid::now_v7()),
        name: Set(name.to_owned()),
        domain: Set(None),
        industry: Set(None),
        size: Set(None),
        phone: Set(None),
        website: Set(None),
        city: Set(None),
        country: Set(None),
        owner_id: Set(owner),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(Some(now)),
    }
    .insert(db(crm).await.conn())
    .await
    .expect("company inserted")
}

/// A contact, owned by `owner`.
pub async fn contact(crm: &TestCrm, owner: Uuid, first: &str, last: &str) -> contact::Model {
    let now = crm.clock().now();
    contact::ActiveModel {
        id: Set(Uuid::now_v7()),
        first_name: Set(first.to_owned()),
        last_name: Set(last.to_owned()),
        email: Set(None),
        phone: Set(None),
        title: Set(None),
        status: Set(ContactStatus::Lead),
        company_id: Set(None),
        owner_id: Set(owner),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(None),
    }
    .insert(db(crm).await.conn())
    .await
    .expect("contact inserted")
}

/// What a fixture deal looks like.
#[derive(Clone, Debug)]
pub struct NewDeal {
    pub title: &'static str,
    pub stage: Stage,
    pub amount_cents: i64,
    pub currency: &'static str,
    pub closed_at: Option<DateTime<Utc>>,
}

impl NewDeal {
    /// An open USD deal.
    pub fn open(title: &'static str, stage: Stage, amount_cents: i64) -> Self {
        NewDeal {
            title,
            stage,
            amount_cents,
            currency: "USD",
            closed_at: None,
        }
    }

    /// A deal won or lost at `closed_at`.
    pub fn closed(
        title: &'static str,
        stage: Stage,
        amount_cents: i64,
        closed_at: DateTime<Utc>,
    ) -> Self {
        NewDeal {
            closed_at: Some(closed_at),
            ..NewDeal::open(title, stage, amount_cents)
        }
    }

    /// The same deal in `currency`.
    pub fn currency(mut self, currency: &'static str) -> Self {
        self.currency = currency;
        self
    }
}

/// A deal of `company`, owned by `owner`.
pub async fn deal(crm: &TestCrm, owner: Uuid, company: Uuid, new: NewDeal) -> deal::Model {
    let now = crm.clock().now();
    deal::ActiveModel {
        id: Set(Uuid::now_v7()),
        title: Set(new.title.to_owned()),
        amount_cents: Set(new.amount_cents),
        currency: Set(new.currency.to_owned()),
        stage: Set(new.stage),
        position: Set(0),
        expected_close_date: Set(None),
        closed_at: Set(new.closed_at),
        lost_reason: Set((new.stage == Stage::Lost).then(|| "Budget".to_owned())),
        company_id: Set(company),
        contact_id: Set(None),
        owner_id: Set(owner),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db(crm).await.conn())
    .await
    .expect("deal inserted")
}

/// What a fixture activity looks like.
#[derive(Clone, Debug)]
pub struct NewActivity {
    pub kind: ActivityKind,
    pub subject: String,
    pub due_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub subject_type: SubjectType,
    pub subject_id: Uuid,
    pub owner_id: Uuid,
    pub assignee_id: Option<Uuid>,
    pub created_at: Option<DateTime<Utc>>,
}

impl NewActivity {
    /// A task on `deal`, owned by `owner`, unassigned and without a due date.
    pub fn task(subject: &str, deal: Uuid, owner: Uuid) -> Self {
        NewActivity {
            kind: ActivityKind::Task,
            subject: subject.to_owned(),
            due_at: None,
            completed_at: None,
            subject_type: SubjectType::Deal,
            subject_id: deal,
            owner_id: owner,
            assignee_id: None,
            created_at: None,
        }
    }

    /// An activity of `kind` instead of a task.
    pub fn kind(mut self, kind: ActivityKind) -> Self {
        self.kind = kind;
        self
    }

    /// Due at `due_at`.
    pub fn due(mut self, due_at: DateTime<Utc>) -> Self {
        self.due_at = Some(due_at);
        self
    }

    /// Assigned to `assignee`.
    pub fn assigned(mut self, assignee: Uuid) -> Self {
        self.assignee_id = Some(assignee);
        self
    }

    /// Completed at `completed_at`.
    pub fn completed(mut self, completed_at: DateTime<Utc>) -> Self {
        self.completed_at = Some(completed_at);
        self
    }

    /// Created at `created_at` (default: the clock's now).
    pub fn created(mut self, created_at: DateTime<Utc>) -> Self {
        self.created_at = Some(created_at);
        self
    }

    /// Attached to another record.
    pub fn on(mut self, subject_type: SubjectType, subject_id: Uuid) -> Self {
        self.subject_type = subject_type;
        self.subject_id = subject_id;
        self
    }
}

/// An activity written straight to the database.
pub async fn activity(crm: &TestCrm, new: NewActivity) -> activity::Model {
    let created_at = new.created_at.unwrap_or_else(|| crm.clock().now());
    activity::ActiveModel {
        id: Set(Uuid::now_v7()),
        kind: Set(new.kind),
        subject: Set(new.subject),
        body: Set(None),
        due_at: Set(new.due_at),
        completed_at: Set(new.completed_at),
        subject_type: Set(new.subject_type),
        subject_id: Set(new.subject_id),
        owner_id: Set(new.owner_id),
        assignee_id: Set(new.assignee_id),
        created_at: Set(created_at),
        updated_at: Set(created_at),
    }
    .insert(db(crm).await.conn())
    .await
    .expect("activity inserted")
}
