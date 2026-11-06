//! Fixtures for the activities and dashboard tests: CRM records made with
//! their model factories (companies, contacts and deals belong to other
//! crates; these tests only need records to attach activities to and
//! count).

#![allow(dead_code)]

use chrono::{DateTime, TimeZone, Utc};
use crm_activities::{ActivitiesModule, DashboardModule};
use crm_core::models::{Activity, Company, Contact, Deal};
use crm_core::{ActivityKind, Stage, SubjectType};
use crm_testkit::TestCrm;
use navree::contracts::Clock;
use navree::db::Db;
use navree::orm::Record;
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
pub async fn company(crm: &TestCrm, owner: Uuid, name: &str) -> Record<Company> {
    Company::factory()
        .set(Company::NAME, name)
        .set(Company::OWNER_ID, owner)
        .create_one(&*db(crm).await)
        .await
        .expect("company created")
}

/// A soft-deleted company.
pub async fn deleted_company(crm: &TestCrm, owner: Uuid, name: &str) -> Record<Company> {
    Company::factory()
        .set(Company::NAME, name)
        .set(Company::OWNER_ID, owner)
        .trashed()
        .create_one(&*db(crm).await)
        .await
        .expect("company created")
}

/// A contact, owned by `owner`.
pub async fn contact(crm: &TestCrm, owner: Uuid, first: &str, last: &str) -> Record<Contact> {
    Contact::factory()
        .set(Contact::FIRST_NAME, first)
        .set(Contact::LAST_NAME, last)
        .set(Contact::OWNER_ID, owner)
        .create_one(&*db(crm).await)
        .await
        .expect("contact created")
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
    /// An open deal in rupees.
    pub fn open(title: &'static str, stage: Stage, amount_cents: i64) -> Self {
        NewDeal {
            title,
            stage,
            amount_cents,
            currency: "INR",
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
pub async fn deal(crm: &TestCrm, owner: Uuid, company: Uuid, new: NewDeal) -> Record<Deal> {
    let lost_reason = (new.stage == Stage::Lost).then_some("Budget");
    Deal::factory()
        .set(Deal::TITLE, new.title)
        .set(Deal::STAGE, new.stage)
        .set(Deal::AMOUNT_CENTS, new.amount_cents)
        .set(Deal::CURRENCY, new.currency)
        .set(Deal::CLOSED_AT, new.closed_at)
        .set(Deal::LOST_REASON, lost_reason)
        .set(Deal::COMPANY_ID, company)
        .set(Deal::OWNER_ID, owner)
        .create_one(&*db(crm).await)
        .await
        .expect("deal created")
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

/// An activity made with its factory, created (and last changed) at
/// `new.created_at` or the clock's now.
pub async fn activity(crm: &TestCrm, new: NewActivity) -> Record<Activity> {
    let created_at = new.created_at.unwrap_or_else(|| crm.clock().now());
    Activity::factory()
        .set(Activity::KIND, new.kind)
        .set(Activity::SUBJECT, new.subject)
        .set(Activity::DUE_AT, new.due_at)
        .set(Activity::COMPLETED_AT, new.completed_at)
        .set(Activity::SUBJECT_TYPE, new.subject_type)
        .set(Activity::SUBJECT_ID, new.subject_id)
        .set(Activity::OWNER_ID, new.owner_id)
        .set(Activity::ASSIGNEE_ID, new.assignee_id)
        .set(Activity::CREATED_AT, created_at)
        .set(Activity::UPDATED_AT, created_at)
        .create_one(&*db(crm).await)
        .await
        .expect("activity created")
}
