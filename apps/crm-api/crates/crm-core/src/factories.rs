//! Model factories: CRM records with fake data, for tests and the demo
//! seeder.
//!
//! ```no_run
//! # use crm_core::factories::DealFactory;
//! # use crm_core::models::{Company, Deal};
//! # use crm_core::Stage;
//! # use chrono::Utc;
//! # use navree::db::Db;
//! # use navree::orm::Model;
//! # async fn demo(db: &Db, rep: uuid::Uuid) -> navree::orm::Result<()> {
//! let agra = Company::factory()
//!     .set(Company::OWNER_ID, rep)
//!     .set(Company::NAME, "Agra")
//!     .create_one(db)
//!     .await?;
//! let won = Deal::factory()
//!     .count(3)
//!     .set(Deal::OWNER_ID, rep)
//!     .set(Deal::COMPANY_ID, agra.id)
//!     .state(DealFactory::won_at(Utc::now()))
//!     .create(db)
//!     .await?;
//! # let _ = (won, Stage::Won);
//! # Ok(()) }
//! ```
//!
//! Owners and assignees are navree-auth accounts, which factories do not
//! create (`AuthService` hashes passwords and normalises emails): set
//! `owner_id` (and a deal's `company_id`, an activity's subject) with
//! `.set(..)` or a state. A record created without one fails its foreign
//! key.

use chrono::{DateTime, Utc};
use navree::orm::factory::{FactoryContext, ModelFactory};
use uuid::Uuid;

use crate::enums::{ActivityKind, CompanySize, ContactStatus, Stage, SubjectType};
use crate::models::{Activity, Company, Contact, Deal};

/// Industries fake companies are in.
const INDUSTRIES: [&str; 8] = [
    "Software",
    "Logistics",
    "Healthcare",
    "Manufacturing",
    "Retail",
    "Financial services",
    "Energy",
    "Education",
];

/// Companies with a fake name, domain, website, phone and address.
#[derive(Default)]
pub struct CompanyFactory;

impl ModelFactory for CompanyFactory {
    type Model = Company;

    fn definition(&self, ctx: &mut FactoryContext<'_, Company>) -> Company {
        let fake = ctx.fake();
        let domain = fake.domain();
        Company {
            name: fake.company(),
            website: Some(format!("https://{domain}")),
            domain: Some(domain),
            industry: Some(fake.pick(&INDUSTRIES).to_owned()),
            size: Some(fake.pick(CompanySize::ALL)),
            phone: Some(fake.phone()),
            city: Some(fake.city()),
            country: Some(fake.country()),
            ..Company::default()
        }
    }
}

/// Contacts with a fake name, email, phone and job title, status `lead`,
/// at no company.
#[derive(Default)]
pub struct ContactFactory;

impl ModelFactory for ContactFactory {
    type Model = Contact;

    fn definition(&self, ctx: &mut FactoryContext<'_, Contact>) -> Contact {
        let fake = ctx.fake();
        let first_name = fake.first_name();
        let last_name = fake.last_name();
        Contact {
            email: Some(format!(
                "{}.{}@{}",
                first_name.to_lowercase(),
                last_name.to_lowercase(),
                fake.domain()
            )),
            first_name,
            last_name,
            phone: Some(fake.phone()),
            title: Some(fake.job_title()),
            status: ContactStatus::Lead,
            ..Contact::default()
        }
    }
}

impl ContactFactory {
    /// A state: the contact works at `company`.
    pub fn at(company: Uuid) -> impl Fn(&mut Contact) + Send + Sync + 'static {
        move |contact| contact.company_id = Some(company)
    }
}

/// Open deals in rupees with a fake title and amount, at the top of the
/// `lead` column.
#[derive(Default)]
pub struct DealFactory;

impl ModelFactory for DealFactory {
    type Model = Deal;

    fn definition(&self, ctx: &mut FactoryContext<'_, Deal>) -> Deal {
        let fake = ctx.fake();
        Deal {
            title: fake.catch_phrase(),
            amount_cents: fake.number(10..=5_000) * 10_000,
            currency: "INR".to_owned(),
            stage: Stage::Lead,
            ..Deal::default()
        }
    }
}

impl DealFactory {
    /// A state: won at `at`.
    pub fn won_at(at: DateTime<Utc>) -> impl Fn(&mut Deal) + Send + Sync + 'static {
        move |deal| {
            deal.stage = Stage::Won;
            deal.closed_at = Some(at);
            deal.lost_reason = None;
        }
    }

    /// A state: lost at `at`, for `reason`.
    pub fn lost_at(
        at: DateTime<Utc>,
        reason: impl Into<String>,
    ) -> impl Fn(&mut Deal) + Send + Sync + 'static {
        let reason = reason.into();
        move |deal| {
            deal.stage = Stage::Lost;
            deal.closed_at = Some(at);
            deal.lost_reason = Some(reason.clone());
        }
    }
}

/// Notes with a fake one-line summary.
#[derive(Default)]
pub struct ActivityFactory;

impl ModelFactory for ActivityFactory {
    type Model = Activity;

    fn definition(&self, ctx: &mut FactoryContext<'_, Activity>) -> Activity {
        Activity {
            kind: ActivityKind::Note,
            subject: ctx.fake().sentence(),
            ..Activity::default()
        }
    }
}

impl ActivityFactory {
    /// A state: attached to the record `subject_type` / `id`.
    pub fn on(
        subject_type: SubjectType,
        id: Uuid,
    ) -> impl Fn(&mut Activity) + Send + Sync + 'static {
        move |activity| {
            activity.subject_type = subject_type;
            activity.subject_id = id;
        }
    }

    /// A state: a task due at `at`.
    pub fn task_due(at: DateTime<Utc>) -> impl Fn(&mut Activity) + Send + Sync + 'static {
        move |activity| {
            activity.kind = ActivityKind::Task;
            activity.due_at = Some(at);
        }
    }
}
