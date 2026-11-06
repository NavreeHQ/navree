//! The `crm-demo` seeder: the four demo accounts (password
//! `password123`), 20 companies, 50 contacts, 30 deals across every stage
//! and 60 activities, with tasks overdue, due today and upcoming. The data
//! is fixed (no randomness) and dated relative to the app's clock; the
//! names match crm-web's mock data.
//!
//! The models' factories make the records: each table's rows are one
//! factory [`Sequence`] whose state sets every column from the row's
//! position, so none of the factories' fake data is left.
//!
//! `db:seed` (or `migrate:fresh --seed`) runs it after `crm-roles`. Running
//! it again changes nothing: the accounts are found again and the records
//! are only created into an empty CRM.

use std::sync::Arc;

use chrono::{DateTime, Datelike, Days, Duration, NaiveTime, Utc};
use crm_core::factories::{ActivityFactory, CompanyFactory, ContactFactory, DealFactory};
use crm_core::models::{Activity, Company, Contact, Deal};
use crm_core::seed::{ensure_demo_accounts, in_uow};
use crm_core::{ActivityKind, CompanySize, ContactStatus, CrmCoreModule, Stage, SubjectType};
use navree::BoxFuture;
use navree::auth::User;
use navree::contracts::Clock;
use navree::db::{Db, Seeder};
use navree::orm::factory::{Factory, Sequence};
use navree::orm::{Executor, Model};
use navree::prelude::*;
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
            if Company::query().with_trashed().exists(db).await? {
                tracing::info!(target: "crm::seed", "the CRM has data; demo records skipped");
                return Ok(());
            }
            let demo = Demo::new(&users, self.clock.now());
            // All of it or none of it, in one unit of work.
            in_uow(db, |uow| async move {
                demo.create(&uow).await?;
                Ok(())
            })
            .await
        })
    }
}

/// `(name, domain, industry, size, city, country)`.
const COMPANIES: [(&str, &str, &str, CompanySize, &str, &str); 20] = [
    (
        "Doaba Logistics",
        "doabalogistics.in",
        "Logistics",
        CompanySize::Large,
        "Jalandhar",
        "India",
    ),
    (
        "Shivalik Health",
        "shivalikhealth.in",
        "Healthcare",
        CompanySize::Huge,
        "Chandigarh",
        "India",
    ),
    (
        "Beas Analytics",
        "beasanalytics.in",
        "Software",
        CompanySize::Medium,
        "Mohali",
        "India",
    ),
    (
        "Chenab Manufacturing",
        "chenabmfg.in",
        "Manufacturing",
        CompanySize::Large,
        "Ludhiana",
        "India",
    ),
    (
        "Phulkari Outfitters",
        "phulkarioutfitters.in",
        "Retail",
        CompanySize::Medium,
        "Amritsar",
        "India",
    ),
    (
        "Jhelum Freight Lines",
        "jhelumfreight.in",
        "Logistics",
        CompanySize::Huge,
        "Delhi",
        "India",
    ),
    (
        "Lakshya Learning Co.",
        "lakshyalearning.in",
        "Education",
        CompanySize::Small,
        "Jaipur",
        "India",
    ),
    (
        "Majha Capital Partners",
        "majhacapital.in",
        "Financial services",
        CompanySize::Small,
        "Mumbai",
        "India",
    ),
    (
        "Sarson Foods",
        "sarsonfoods.in",
        "Food & beverage",
        CompanySize::Large,
        "Patiala",
        "India",
    ),
    (
        "Sirhind Legal Associates",
        "sirhindlegal.in",
        "Legal",
        CompanySize::Medium,
        "Gurugram",
        "India",
    ),
    (
        "Vasant Media",
        "vasantmedia.in",
        "Media",
        CompanySize::Small,
        "Noida",
        "India",
    ),
    (
        "Indrapuri Energy",
        "indrapurienergy.in",
        "Energy",
        CompanySize::Huge,
        "Delhi",
        "India",
    ),
    (
        "Chandni Cloudworks",
        "chandnicloud.in",
        "Software",
        CompanySize::Large,
        "Pune",
        "India",
    ),
    (
        "Teej Hospitality",
        "teejhospitality.in",
        "Hospitality",
        CompanySize::Huge,
        "Jaipur",
        "India",
    ),
    (
        "Gehun Agritech",
        "gehunagritech.in",
        "Agriculture",
        CompanySize::Medium,
        "Bathinda",
        "India",
    ),
    (
        "Hariyali Realty",
        "hariyalirealty.in",
        "Real estate",
        CompanySize::Small,
        "Dehradun",
        "India",
    ),
    (
        "Kikar Aerospace",
        "kikaraero.in",
        "Aerospace",
        CompanySize::Huge,
        "Bengaluru",
        "India",
    ),
    (
        "Malwa Health Partners",
        "malwahealth.in",
        "Healthcare",
        CompanySize::Large,
        "Lucknow",
        "India",
    ),
    (
        "Pipal Software",
        "pipalsoft.in",
        "Software",
        CompanySize::Tiny,
        "Chandigarh",
        "India",
    ),
    (
        "Sutlej Biolabs",
        "sutlejbio.in",
        "Biotech",
        CompanySize::Medium,
        "Mohali",
        "India",
    ),
];

const FIRST_NAMES: [&str; 25] = [
    "Harleen",
    "Arjun",
    "Jaspreet",
    "Rohit",
    "Simran",
    "Amandeep",
    "Neha",
    "Vikram",
    "Ananya",
    "Navdeep",
    "Kavya",
    "Aditya",
    "Ravneet",
    "Ishaan",
    "Pooja",
    "Siddharth",
    "Meera",
    "Kulwinder",
    "Rajveer",
    "Sandeep",
    "Gurleen",
    "Manish",
    "Tanvi",
    "Harpreet",
    "Nisha",
];

const LAST_NAMES: [&str; 25] = [
    "Dhillon",
    "Sharma",
    "Gill",
    "Verma",
    "Sandhu",
    "Malhotra",
    "Grewal",
    "Kapoor",
    "Sidhu",
    "Arora",
    "Brar",
    "Khanna",
    "Bains",
    "Mehra",
    "Chopra",
    "Bhatia",
    "Gupta",
    "Agarwal",
    "Joshi",
    "Randhawa",
    "Walia",
    "Saini",
    "Kohli",
    "Ahluwalia",
    "Tiwana",
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
    ("North India rollout", Stage::Won),
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

/// The demo records of the demo accounts, dated relative to `now`.
#[derive(Clone, Copy, Debug)]
struct Demo {
    now: DateTime<Utc>,
    /// Who owns records, in turn: the manager, then each rep twice.
    sellers: [Uuid; 5],
}

impl Demo {
    /// The demo records of `users`, the demo accounts in
    /// [`DEMO_ACCOUNTS`](crm_core::seed::DEMO_ACCOUNTS) order.
    fn new(users: &[User], now: DateTime<Utc>) -> Self {
        let (manager, rep1, rep2) = (users[1].id, users[2].id, users[3].id);
        Demo {
            now,
            sellers: [manager, rep1, rep1, rep2, rep2],
        }
    }

    /// Creates every demo record through `exec`: the companies, then their
    /// contacts, the deals and the activities on all three.
    async fn create<E: Executor + ?Sized>(self, exec: &E) -> navree::orm::Result<()> {
        let companies = self.companies().create(exec).await?.into_models();
        let contacts = self
            .contacts(companies.clone())
            .create(exec)
            .await?
            .into_models();
        let deals = self
            .deals(companies.clone(), contacts.clone())
            .create(exec)
            .await?
            .into_models();
        self.activities(companies, contacts, deals)
            .create(exec)
            .await?;
        Ok(())
    }

    /// The 20 [`COMPANIES`], owned by the sellers in turn.
    fn companies(self) -> Factory<CompanyFactory> {
        Company::factory()
            .count(COMPANIES.len())
            .sequence(Sequence::from_fn(move |company: &mut Company, at| {
                let i = at.index();
                let (name, domain, industry, size, city, country) = COMPANIES[i];
                let created = self.days_ago(300 - i as u64 * 12);
                company.name = name.to_owned();
                company.domain = Some(domain.to_owned());
                company.industry = Some(industry.to_owned());
                company.size = Some(size);
                company.phone = Some(format!("+91 98765 {:05}", 43200 + i));
                company.website = Some(format!("https://{domain}"));
                company.city = Some(city.to_owned());
                company.country = Some(country.to_owned());
                company.owner_id = self.seller(i);
                company.created_at = created;
                company.updated_at = created + Duration::days((i % 7) as i64);
            }))
    }

    /// 50 contacts: the first 42 at the `companies` in turn (most owned by
    /// the company's owner), the last 8 at none we know of.
    fn contacts(self, companies: Vec<Company>) -> Factory<ContactFactory> {
        Contact::factory().count(50).sequence(Sequence::from_fn(
            move |contact: &mut Contact, at| {
                let i = at.index();
                let company = (i < 42).then(|| &companies[i % companies.len()]);
                let first = FIRST_NAMES[i % FIRST_NAMES.len()];
                let last = LAST_NAMES[(i * 7 + i / FIRST_NAMES.len() * 3) % LAST_NAMES.len()];
                let domain = company
                    .and_then(|company| company.domain.as_deref())
                    .unwrap_or("example.com");
                let created = self.days_ago(260 - i as u64 * 4);
                contact.first_name = first.to_owned();
                contact.last_name = last.to_owned();
                contact.email = Some(format!(
                    "{}.{}@{domain}",
                    first.to_lowercase(),
                    last.to_lowercase()
                ));
                contact.phone = (i % 5 != 0).then(|| format!("+91 99887 {:05}", 76500 + i));
                contact.title = (i % 9 != 0).then(|| TITLES[i % TITLES.len()].to_owned());
                contact.status = STATUSES[i % STATUSES.len()];
                contact.company_id = company.map(|company| company.id);
                contact.owner_id = match company {
                    Some(company) if i % 4 != 0 => company.owner_id,
                    _ => self.seller(i),
                };
                contact.created_at = created;
                contact.updated_at = created + Duration::days((i % 11) as i64);
            },
        ))
    }

    /// The 30 [`DEALS`], each with every third company (and the company's
    /// first contact), stacked in its stage's column in order. Won deals
    /// closed this month (the dashboard's `won_this_month`), lost ones a
    /// few weeks ago.
    fn deals(self, companies: Vec<Company>, contacts: Vec<Contact>) -> Factory<DealFactory> {
        Deal::factory()
            .count(DEALS.len())
            .sequence(Sequence::from_fn(move |deal: &mut Deal, at| {
                let i = at.index();
                let (title, stage) = DEALS[i];
                let company = &companies[(i * 3) % companies.len()];
                let contact = contacts
                    .iter()
                    .find(|contact| contact.company_id == Some(company.id));
                let created = self.days_ago(120 - i as u64 * 3);
                let closed_at = match stage {
                    Stage::Won => Some(self.days_ago((i % 3) as u64).max(self.start_of_month())),
                    Stage::Lost => Some(self.days_ago(20 + i as u64)),
                    _ => None,
                };
                deal.title = title.to_owned();
                // ₹3,00,000 to ₹1,02,50,000, in paise.
                deal.amount_cents = ((i as i64 * 37) % 200 + 6) * 5_000_000;
                deal.currency = "INR".to_owned();
                deal.stage = stage;
                // Below the deals of its stage listed before it.
                deal.position = DEALS[..i]
                    .iter()
                    .filter(|(_, earlier)| *earlier == stage)
                    .count() as i32;
                deal.expected_close_date = match closed_at {
                    Some(closed) => Some(closed.date_naive()),
                    None => self
                        .now
                        .date_naive()
                        .checked_add_days(Days::new(7 + (i as u64 * 5) % 90)),
                };
                deal.closed_at = closed_at;
                deal.lost_reason =
                    (stage == Stage::Lost).then(|| LOST_REASONS[i % LOST_REASONS.len()].to_owned());
                deal.company_id = company.id;
                deal.contact_id = contact.map(|contact| contact.id);
                deal.owner_id = if i % 4 == 0 {
                    self.seller(i)
                } else {
                    company.owner_id
                };
                deal.created_at = created;
                deal.updated_at = closed_at.unwrap_or(created + Duration::days(2));
            }))
    }

    /// 60 activities of the [`KINDS`] in turn, on a deal, a contact and a
    /// company in turn, owned by the record's owner. The tasks are overdue,
    /// due today, upcoming or done, in turn; every fifth activity's task is
    /// assigned to the next seller.
    fn activities(
        self,
        companies: Vec<Company>,
        contacts: Vec<Contact>,
        deals: Vec<Deal>,
    ) -> Factory<ActivityFactory> {
        Activity::factory().count(60).sequence(Sequence::from_fn(
            move |activity: &mut Activity, at| {
                let i = at.index();
                let kind = KINDS[i % KINDS.len()];
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
                let today = self.start_of_today();
                let (due_at, completed_at) = match kind {
                    ActivityKind::Task => match (i / 3) % 4 {
                        0 => (Some(today - Duration::days(2 + (i % 3) as i64)), None),
                        1 => (Some(today + Duration::hours(9 + (i % 8) as i64)), None),
                        2 => (Some(today + Duration::days(1 + (i % 6) as i64)), None),
                        _ => (
                            Some(today - Duration::days(4)),
                            Some(today - Duration::days(3)),
                        ),
                    },
                    _ => (None, None),
                };
                let (subject, body) = write_up(kind, &label);
                let created = self.days_ago(60 - i as u64);
                activity.kind = kind;
                activity.subject = subject;
                activity.body = body.map(str::to_owned);
                activity.due_at = due_at;
                activity.completed_at = completed_at;
                activity.subject_type = subject_type;
                activity.subject_id = subject_id;
                activity.owner_id = owner;
                activity.assignee_id = (kind == ActivityKind::Task).then(|| {
                    if i % 5 == 0 {
                        self.seller(i + 1)
                    } else {
                        owner
                    }
                });
                activity.created_at = created;
                activity.updated_at = completed_at.unwrap_or(created);
            },
        ))
    }

    fn days_ago(&self, days: u64) -> DateTime<Utc> {
        self.now - Duration::days(days as i64)
    }

    /// Midnight UTC at the start of `now`'s day.
    fn start_of_today(&self) -> DateTime<Utc> {
        self.now.date_naive().and_time(NaiveTime::MIN).and_utc()
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

    /// The `n`th seller, starting over after the last.
    fn seller(&self, n: usize) -> Uuid {
        self.sellers[n % self.sellers.len()]
    }
}

/// The subject line and body of an activity of `kind` about `label`.
fn write_up(kind: ActivityKind, label: &str) -> (String, Option<&'static str>) {
    match kind {
        ActivityKind::Task => (format!("Follow up with {label}"), None),
        ActivityKind::Note => (
            format!("Notes on {label}"),
            Some("Budget confirmed; decision expected after the steering meeting."),
        ),
        ActivityKind::Call => (
            format!("Call about {label}"),
            Some("Walked through pricing and the rollout plan."),
        ),
        ActivityKind::Email => (
            format!("Sent proposal to {label}"),
            Some("Attached the updated proposal and SLA terms."),
        ),
        ActivityKind::Meeting => (
            format!("Meeting with {label}"),
            Some("Demo for the operations team."),
        ),
    }
}
