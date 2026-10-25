//! The `crm-demo` seeder (spec §6): the four demo accounts (password
//! `password123`), 20 companies, 50 contacts, 30 deals across every stage
//! and 60 activities, with tasks overdue, due today and upcoming. The data
//! is fixed (no randomness) and dated relative to the app's clock; the
//! names match crm-web's mock data.
//!
//! `db:seed` (or `migrate:fresh --seed`) runs it after `crm-roles`. Running
//! it again changes nothing: the accounts are found again and the records
//! are only created into an empty CRM.

use std::sync::Arc;

use chrono::{DateTime, Datelike, Days, Duration, NaiveTime, Utc};
use crm_core::entities::{activity, company, contact, deal};
use crm_core::seed::ensure_demo_accounts;
use crm_core::{ActivityKind, CompanySize, ContactStatus, CrmCoreModule, Stage, SubjectType};
use keel::BoxFuture;
use keel::auth::User;
use keel::contracts::Clock;
use keel::db::{Db, Seeder};
use keel::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, DbErr, EntityTrait, PaginatorTrait};
use uuid::Uuid;

/// The seeder's name: `db:seed crm-demo`.
pub const DEMO_SEEDER: &str = "crm-demo";

/// Registers the `crm-demo` seeder.
#[module(imports = [CrmCoreModule], register = Self::wire)]
pub struct DemoModule;

impl DemoModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.seeder::<DemoSeeder>();
    }
}

/// `db:seed crm-demo`; see the [module docs](self).
#[injectable]
pub struct DemoSeeder {
    clock: Arc<dyn Clock>,
}

impl Seeder for DemoSeeder {
    fn name(&self) -> &'static str {
        DEMO_SEEDER
    }

    fn run<'a>(&'a self, ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let users = ensure_demo_accounts(ctx).await?;
            if company::Entity::find().count(db.conn()).await? > 0 {
                tracing::info!(target: "crm::seed", "the CRM has data; demo records skipped");
                return Ok(());
            }
            let now = self.clock.now();
            db.transaction(|uow| async move {
                let tx = uow.tx().await?;
                Demo::new(&users, now).insert(&*tx).await
            })
            .await?;
            Ok(())
        })
    }
}

/// `(name, domain, industry, size, city, country)`.
const COMPANIES: [(&str, &str, &str, CompanySize, &str, &str); 20] = [
    (
        "Northwind Logistics",
        "northwind.io",
        "Logistics",
        CompanySize::Large,
        "Rotterdam",
        "Netherlands",
    ),
    (
        "Halcyon Health",
        "halcyonhealth.com",
        "Healthcare",
        CompanySize::Huge,
        "Boston",
        "United States",
    ),
    (
        "Brightwater Analytics",
        "brightwater.ai",
        "Software",
        CompanySize::Medium,
        "Austin",
        "United States",
    ),
    (
        "Copperline Manufacturing",
        "copperline.com",
        "Manufacturing",
        CompanySize::Large,
        "Pittsburgh",
        "United States",
    ),
    (
        "Fjord & Pine Outfitters",
        "fjordpine.no",
        "Retail",
        CompanySize::Medium,
        "Bergen",
        "Norway",
    ),
    (
        "Meridian Freight",
        "meridianfreight.com",
        "Logistics",
        CompanySize::Huge,
        "Singapore",
        "Singapore",
    ),
    (
        "Lumen Learning Co.",
        "lumenlearning.co",
        "Education",
        CompanySize::Small,
        "Toronto",
        "Canada",
    ),
    (
        "Atlas Ridge Capital",
        "atlasridge.com",
        "Financial services",
        CompanySize::Small,
        "New York",
        "United States",
    ),
    (
        "Saltmarsh Foods",
        "saltmarsh.co.uk",
        "Food & beverage",
        CompanySize::Large,
        "Bristol",
        "United Kingdom",
    ),
    (
        "Quillstone Legal",
        "quillstone.law",
        "Legal",
        CompanySize::Medium,
        "Chicago",
        "United States",
    ),
    (
        "Vantage Point Media",
        "vantagepoint.media",
        "Media",
        CompanySize::Small,
        "Los Angeles",
        "United States",
    ),
    (
        "Ironbark Energy",
        "ironbark.energy",
        "Energy",
        CompanySize::Huge,
        "Perth",
        "Australia",
    ),
    (
        "Cobalt Cloudworks",
        "cobaltcloud.dev",
        "Software",
        CompanySize::Large,
        "Berlin",
        "Germany",
    ),
    (
        "Tidewater Hospitality",
        "tidewaterhotels.com",
        "Hospitality",
        CompanySize::Huge,
        "Lisbon",
        "Portugal",
    ),
    (
        "Greenfield Agritech",
        "greenfieldag.com",
        "Agriculture",
        CompanySize::Medium,
        "Des Moines",
        "United States",
    ),
    (
        "Harbor & Main Realty",
        "harborandmain.com",
        "Real estate",
        CompanySize::Small,
        "Seattle",
        "United States",
    ),
    (
        "Kestrel Aerospace",
        "kestrelaero.com",
        "Aerospace",
        CompanySize::Huge,
        "Toulouse",
        "France",
    ),
    (
        "Mosaic Health Partners",
        "mosaichp.com",
        "Healthcare",
        CompanySize::Large,
        "Denver",
        "United States",
    ),
    (
        "Pinecrest Software",
        "pinecrest.io",
        "Software",
        CompanySize::Tiny,
        "Dublin",
        "Ireland",
    ),
    (
        "Sterling Biolabs",
        "sterlingbio.com",
        "Biotech",
        CompanySize::Medium,
        "San Diego",
        "United States",
    ),
];

const FIRST_NAMES: [&str; 25] = [
    "Elena",
    "James",
    "Aiko",
    "Mateo",
    "Grace",
    "Oluwaseun",
    "Hannah",
    "Ravi",
    "Sofia",
    "Liam",
    "Mei",
    "Daniel",
    "Freya",
    "Omar",
    "Isabel",
    "Noah",
    "Zara",
    "Lucas",
    "Amara",
    "Ethan",
    "Chloe",
    "Kenji",
    "Leah",
    "Samuel",
    "Nadia",
];

const LAST_NAMES: [&str; 25] = [
    "Lindqvist",
    "Okafor",
    "Tanaka",
    "Garcia",
    "Whitman",
    "Adeyemi",
    "Brooks",
    "Iyer",
    "Rossi",
    "Murphy",
    "Chen",
    "Kowalski",
    "Nielsen",
    "Haddad",
    "Moreau",
    "Fischer",
    "Khan",
    "Silva",
    "Mensah",
    "Clarke",
    "Dubois",
    "Sato",
    "Goldberg",
    "Novak",
    "Petrov",
];

const TITLES: [&str; 10] = [
    "VP of Operations",
    "Head of Procurement",
    "Chief Technology Officer",
    "Finance Director",
    "Operations Manager",
    "IT Director",
    "Chief Executive Officer",
    "Head of Partnerships",
    "Product Lead",
    "Supply Chain Manager",
];

const STATUSES: [ContactStatus; 7] = [
    ContactStatus::Lead,
    ContactStatus::Lead,
    ContactStatus::Prospect,
    ContactStatus::Prospect,
    ContactStatus::Customer,
    ContactStatus::Customer,
    ContactStatus::Churned,
];

/// `(title, stage)`: 6 lead, 6 qualified, 5 proposal, 5 negotiation, 5 won,
/// 3 lost.
const DEALS: [(&str, Stage); 30] = [
    ("Fleet telematics rollout", Stage::Lead),
    ("Annual platform license", Stage::Lead),
    ("Warehouse analytics pilot", Stage::Lead),
    ("Enterprise renewal", Stage::Lead),
    ("Data residency add-on", Stage::Lead),
    ("Regional expansion", Stage::Lead),
    ("Support plan upgrade", Stage::Qualified),
    ("Onboarding services", Stage::Qualified),
    ("Multi-site deployment", Stage::Qualified),
    ("Security review package", Stage::Qualified),
    ("API access tier", Stage::Qualified),
    ("Seasonal capacity", Stage::Qualified),
    ("Procurement integration", Stage::Proposal),
    ("Executive dashboard suite", Stage::Proposal),
    ("Field team seats", Stage::Proposal),
    ("Compliance module", Stage::Proposal),
    ("Partner portal build", Stage::Proposal),
    ("Migration from legacy CRM", Stage::Negotiation),
    ("Forecasting add-on", Stage::Negotiation),
    ("Training workshop series", Stage::Negotiation),
    ("Three-year enterprise agreement", Stage::Negotiation),
    ("Pilot extension", Stage::Negotiation),
    ("Customer success retainer", Stage::Won),
    ("Inventory sync connector", Stage::Won),
    ("Board reporting pack", Stage::Won),
    ("EMEA rollout", Stage::Won),
    ("Mobile field app", Stage::Won),
    ("Usage-based overage", Stage::Lost),
    ("Sandbox environments", Stage::Lost),
    ("Premium SLA", Stage::Lost),
];

const LOST_REASONS: [&str; 3] = [
    "Went with a competitor on price",
    "No budget this fiscal year",
    "Project paused after reorg; revisit next quarter",
];

/// The kinds of the activities, in turn (a third are tasks).
const KINDS: [ActivityKind; 6] = [
    ActivityKind::Task,
    ActivityKind::Note,
    ActivityKind::Call,
    ActivityKind::Task,
    ActivityKind::Email,
    ActivityKind::Meeting,
];

/// The demo records for `users` (the demo accounts, in order), dated
/// relative to `now`.
struct Demo<'a> {
    now: DateTime<Utc>,
    /// Who owns records, in turn: the manager and the two reps.
    sellers: [&'a User; 5],
}

impl<'a> Demo<'a> {
    fn new(users: &'a [User], now: DateTime<Utc>) -> Self {
        let (manager, rep1, rep2) = (&users[1], &users[2], &users[3]);
        Demo {
            now,
            sellers: [manager, rep1, rep1, rep2, rep2],
        }
    }

    fn days_ago(&self, days: u64) -> DateTime<Utc> {
        self.now - Duration::days(days as i64)
    }

    /// Midnight UTC on the first day of `now`'s month.
    fn start_of_month(&self) -> DateTime<Utc> {
        self.now
            .date_naive()
            .with_day(1)
            .unwrap_or(self.now.date_naive())
            .and_time(NaiveTime::MIN)
            .and_utc()
    }

    fn seller(&self, n: usize) -> Uuid {
        self.sellers[n % self.sellers.len()].id
    }

    async fn insert<C: sea_orm::ConnectionTrait>(&self, conn: &C) -> Result<(), DbErr> {
        let companies = self.companies(conn).await?;
        let contacts = self.contacts(conn, &companies).await?;
        let deals = self.deals(conn, &companies, &contacts).await?;
        self.activities(conn, &companies, &contacts, &deals).await
    }

    async fn companies<C: sea_orm::ConnectionTrait>(
        &self,
        conn: &C,
    ) -> Result<Vec<company::Model>, DbErr> {
        let mut companies = Vec::with_capacity(COMPANIES.len());
        for (i, (name, domain, industry, size, city, country)) in COMPANIES.into_iter().enumerate()
        {
            let created = self.days_ago(300 - i as u64 * 12);
            let model = company::ActiveModel {
                id: Set(Uuid::now_v7()),
                name: Set(name.to_owned()),
                domain: Set(Some(domain.to_owned())),
                industry: Set(Some(industry.to_owned())),
                size: Set(Some(size)),
                phone: Set(Some(format!("+1 555-01{:02}", i))),
                website: Set(Some(format!("https://{domain}"))),
                city: Set(Some(city.to_owned())),
                country: Set(Some(country.to_owned())),
                owner_id: Set(self.seller(i)),
                created_at: Set(created),
                updated_at: Set(created + Duration::days((i % 7) as i64)),
                deleted_at: Set(None),
            };
            companies.push(model.insert(conn).await?);
        }
        Ok(companies)
    }

    async fn contacts<C: sea_orm::ConnectionTrait>(
        &self,
        conn: &C,
        companies: &[company::Model],
    ) -> Result<Vec<contact::Model>, DbErr> {
        let mut contacts = Vec::with_capacity(50);
        for i in 0..50 {
            // The last 8 contacts work nowhere we know of.
            let company = (i < 42).then(|| &companies[i % companies.len()]);
            let first = FIRST_NAMES[i % FIRST_NAMES.len()];
            let last = LAST_NAMES[(i * 7 + i / FIRST_NAMES.len() * 3) % LAST_NAMES.len()];
            let domain = company
                .and_then(|c| c.domain.clone())
                .unwrap_or_else(|| "fastmail.com".to_owned());
            let created = self.days_ago(260 - i as u64 * 4);
            let model = contact::ActiveModel {
                id: Set(Uuid::now_v7()),
                first_name: Set(first.to_owned()),
                last_name: Set(last.to_owned()),
                email: Set(Some(format!(
                    "{}.{}@{domain}",
                    first.to_lowercase(),
                    last.to_lowercase()
                ))),
                phone: Set((i % 5 != 0).then(|| format!("+1 555-02{:02}", i))),
                title: Set((i % 9 != 0).then(|| TITLES[i % TITLES.len()].to_owned())),
                status: Set(STATUSES[i % STATUSES.len()]),
                company_id: Set(company.map(|c| c.id)),
                owner_id: Set(match company {
                    Some(company) if i % 4 != 0 => company.owner_id,
                    _ => self.seller(i),
                }),
                created_at: Set(created),
                updated_at: Set(created + Duration::days((i % 11) as i64)),
                deleted_at: Set(None),
            };
            contacts.push(model.insert(conn).await?);
        }
        Ok(contacts)
    }

    async fn deals<C: sea_orm::ConnectionTrait>(
        &self,
        conn: &C,
        companies: &[company::Model],
        contacts: &[contact::Model],
    ) -> Result<Vec<deal::Model>, DbErr> {
        let mut deals = Vec::with_capacity(DEALS.len());
        let mut positions = [0i32; 6];
        let today = self.now.date_naive();
        for (i, (title, stage)) in DEALS.into_iter().enumerate() {
            let company = &companies[(i * 3) % companies.len()];
            let contact = contacts.iter().find(|c| c.company_id == Some(company.id));
            let created = self.days_ago(120 - i as u64 * 3);
            let position = &mut positions[stage as usize];
            // Won deals close this month (the dashboard's `won_this_month`),
            // lost ones a few weeks ago.
            let closed_at = match stage {
                Stage::Won => Some(self.days_ago((i % 3) as u64).max(self.start_of_month())),
                Stage::Lost => Some(self.days_ago(20 + i as u64)),
                _ => None,
            };
            let expected_close_date = match closed_at {
                Some(closed) => Some(closed.date_naive()),
                None => today.checked_add_days(Days::new(7 + (i as u64 * 5) % 90)),
            };
            let model = deal::ActiveModel {
                id: Set(Uuid::now_v7()),
                title: Set(title.to_owned()),
                amount_cents: Set(((i as i64 * 37) % 200 + 6) * 100_000),
                currency: Set("USD".to_owned()),
                stage: Set(stage),
                position: Set(*position),
                expected_close_date: Set(expected_close_date),
                closed_at: Set(closed_at),
                lost_reason: Set(
                    (stage == Stage::Lost).then(|| LOST_REASONS[i % LOST_REASONS.len()].to_owned())
                ),
                company_id: Set(company.id),
                contact_id: Set(contact.map(|c| c.id)),
                owner_id: Set(if i % 4 == 0 {
                    self.seller(i)
                } else {
                    company.owner_id
                }),
                created_at: Set(created),
                updated_at: Set(closed_at.unwrap_or(created + Duration::days(2))),
            };
            *position += 1;
            deals.push(model.insert(conn).await?);
        }
        Ok(deals)
    }

    async fn activities<C: sea_orm::ConnectionTrait>(
        &self,
        conn: &C,
        companies: &[company::Model],
        contacts: &[contact::Model],
        deals: &[deal::Model],
    ) -> Result<(), DbErr> {
        let start_of_today = self.now.date_naive().and_time(NaiveTime::MIN).and_utc();
        for i in 0..60usize {
            let kind = KINDS[i % KINDS.len()];
            // Attached to a deal, a contact or a company, in turn.
            let (subject_type, subject_id, owner, label) = match i % 3 {
                0 => {
                    let deal = &deals[i % deals.len()];
                    (
                        SubjectType::Deal,
                        deal.id,
                        deal.owner_id,
                        deal.title.clone(),
                    )
                }
                1 => {
                    let contact = &contacts[i % contacts.len()];
                    (
                        SubjectType::Contact,
                        contact.id,
                        contact.owner_id,
                        contact.full_name(),
                    )
                }
                _ => {
                    let company = &companies[i % companies.len()];
                    (
                        SubjectType::Company,
                        company.id,
                        company.owner_id,
                        company.name.clone(),
                    )
                }
            };
            let created = self.days_ago(60 - i as u64);
            // Tasks: overdue, due today, upcoming or done, in turn.
            let (due_at, completed_at, assignee) = if kind == ActivityKind::Task {
                let assignee = if i % 5 == 0 {
                    self.seller(i + 1)
                } else {
                    owner
                };
                match (i / 3) % 4 {
                    0 => (
                        Some(start_of_today - Duration::days(2 + (i % 3) as i64)),
                        None,
                        assignee,
                    ),
                    1 => (
                        Some(start_of_today + Duration::hours(9 + (i % 8) as i64)),
                        None,
                        assignee,
                    ),
                    2 => (
                        Some(start_of_today + Duration::days(1 + (i % 6) as i64)),
                        None,
                        assignee,
                    ),
                    _ => (
                        Some(start_of_today - Duration::days(4)),
                        Some(start_of_today - Duration::days(3)),
                        assignee,
                    ),
                }
            } else {
                (None, None, owner)
            };
            let (subject, body) = match kind {
                ActivityKind::Task => (format!("Follow up with {label}"), None),
                ActivityKind::Note => (
                    format!("Notes on {label}"),
                    Some(
                        "Budget confirmed; decision expected after the steering meeting."
                            .to_owned(),
                    ),
                ),
                ActivityKind::Call => (
                    format!("Call about {label}"),
                    Some("Walked through pricing and the rollout plan.".to_owned()),
                ),
                ActivityKind::Email => (
                    format!("Sent proposal to {label}"),
                    Some("Attached the updated proposal and SLA terms.".to_owned()),
                ),
                ActivityKind::Meeting => (
                    format!("Meeting with {label}"),
                    Some("Demo for the operations team.".to_owned()),
                ),
            };
            activity::ActiveModel {
                id: Set(Uuid::now_v7()),
                kind: Set(kind),
                subject: Set(subject),
                body: Set(body),
                due_at: Set(due_at),
                completed_at: Set(completed_at),
                subject_type: Set(subject_type),
                subject_id: Set(subject_id),
                owner_id: Set(owner),
                assignee_id: Set((kind == ActivityKind::Task).then_some(assignee)),
                created_at: Set(created),
                updated_at: Set(completed_at.unwrap_or(created)),
            }
            .insert(conn)
            .await?;
        }
        Ok(())
    }
}
