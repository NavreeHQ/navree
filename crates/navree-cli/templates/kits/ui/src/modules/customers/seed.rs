//! Forty demo customers, so the screens have something to show.
//!
//! They are written when the app boots, in development and in tests, and never
//! when `env` is `production`. They are written once: a database that has
//! customers is left as it is. `navree db:seed customers` writes them on
//! demand. Edit the table below to change them; each line is one customer.

use chrono::NaiveDate;
use navree::BoxFuture;
use navree::config::Config;
use navree::db::{Db, Seeder, Uow};
use navree::kernel::Lifecycle;
use navree::prelude::*;

use super::entity::{Customer, Plan, Status};
use crate::ui::auth::ensure_demo_account;

/// One customer per line: name, city, plan, status, monthly fee in rupees, the
/// day they joined and whether they pay by auto-pay. The email follows from the
/// name and the phone number from the line's place in the table.
const CUSTOMERS: &str = "\
Rajesh Sharma | Ludhiana | growth | active | 29999 | 2024-03-12 | yes
Sunita Verma | Ludhiana | starter | active | 4999 | 2025-01-20 | yes
Amit Khanna | Ludhiana | scale | active | 100000 | 2023-08-05 | yes
Pooja Malhotra | Ludhiana | growth | trial | 29999 | 2026-10-02 | no
Deepak Chopra | Ludhiana | starter | paused | 4999 | 2024-11-18 | no
Vikram Arora | Amritsar | growth | active | 34999 | 2024-06-30 | yes
Neha Bhatia | Amritsar | starter | active | 4999 | 2025-05-14 | yes
Manoj Sethi | Amritsar | scale | active | 150000 | 2023-02-27 | yes
Kavita Joshi | Amritsar | growth | churned | 29999 | 2024-09-09 | no
Sanjay Tandon | Amritsar | starter | trial | 4999 | 2026-10-21 | no
Anil Mehra | Jalandhar | growth | active | 44999 | 2024-01-15 | yes
Ritu Kapoor | Jalandhar | starter | active | 7999 | 2025-07-01 | no
Naveen Saxena | Jalandhar | scale | paused | 125000 | 2023-11-23 | yes
Shivani Goyal | Jalandhar | growth | active | 29999 | 2025-03-09 | yes
Tarun Bansal | Jalandhar | starter | churned | 4999 | 2024-04-02 | no
Ashok Luthra | Chandigarh | scale | active | 250000 | 2022-12-10 | yes
Meena Walia | Chandigarh | growth | active | 39999 | 2024-08-19 | yes
Gaurav Sood | Chandigarh | starter | active | 9999 | 2025-09-25 | no
Isha Kakkar | Chandigarh | growth | trial | 29999 | 2026-09-30 | no
Rohit Anand | Chandigarh | starter | active | 4999 | 2025-02-11 | yes
Priya Agarwal | Delhi | scale | active | 175000 | 2023-05-16 | yes
Mohit Mittal | Delhi | growth | active | 59999 | 2024-10-07 | yes
Anjali Mishra | Delhi | starter | active | 4999 | 2025-11-03 | no
Kunal Pandey | Delhi | growth | paused | 29999 | 2024-02-28 | no
Seema Dubey | Delhi | starter | trial | 4999 | 2026-10-15 | no
Sahil Rastogi | Jaipur | growth | active | 29999 | 2025-04-22 | yes
Komal Mathur | Jaipur | starter | active | 5999 | 2025-06-18 | yes
Aman Trivedi | Jaipur | scale | active | 110000 | 2023-09-12 | yes
Rekha Gupta | Jaipur | growth | churned | 29999 | 2024-05-06 | no
Nitin Purohit | Jaipur | starter | active | 4999 | 2025-08-30 | no
Varun Shukla | Lucknow | growth | active | 32999 | 2024-07-14 | yes
Poonam Yadav | Lucknow | starter | active | 4999 | 2025-10-09 | no
Ankit Srivastava | Lucknow | scale | active | 120000 | 2023-04-03 | yes
Renu Tiwari | Lucknow | growth | trial | 29999 | 2026-10-27 | no
Sandeep Awasthi | Lucknow | starter | paused | 4999 | 2024-12-01 | no
Divya Bhardwaj | Gurugram | growth | active | 49999 | 2024-03-29 | yes
Rahul Chauhan | Gurugram | scale | active | 200000 | 2022-10-18 | yes
Simran Dhawan | Gurugram | starter | active | 6999 | 2025-12-05 | yes
Ajay Rawat | Gurugram | growth | active | 29999 | 2025-01-08 | no
Tanvi Khurana | Gurugram | starter | churned | 4999 | 2024-06-11 | no
";

/// One line of [`CUSTOMERS`], read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seed {
    /// The name.
    pub name: String,
    /// The email: the name in lower case, joined by a dot, at `example.com`.
    pub email: String,
    /// A made-up Indian mobile number in E.164, one for each line.
    pub phone: String,
    /// The city.
    pub city: String,
    /// The plan.
    pub plan: Plan,
    /// The status.
    pub status: Status,
    /// The monthly fee in rupees.
    pub rupees: i64,
    /// The day the customer joined.
    pub since: NaiveDate,
    /// Whether the customer pays by auto-pay.
    pub autopay: bool,
}

impl Seed {
    /// The customer to store.
    pub fn into_customer(self) -> Customer {
        Customer {
            name: self.name,
            email: self.email,
            phone: self.phone,
            city: self.city,
            plan: self.plan.key().to_owned(),
            status: self.status.key().to_owned(),
            monthly_paise: self.rupees * 100,
            since: self.since,
            autopay: self.autopay,
            ..Customer::default()
        }
    }
}

/// The lines of [`CUSTOMERS`], read. A line that is not seven cells long, or
/// holds a number or a day that does not read, stops the app with a message
/// that names it: it is a mistake in this file.
pub fn seeds() -> Vec<Seed> {
    CUSTOMERS
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| read(index, line))
        .collect()
}

fn read(index: usize, line: &str) -> Seed {
    let cells: Vec<&str> = line.split('|').map(str::trim).collect();
    let Ok([name, city, plan, status, rupees, since, autopay]) = <[&str; 7]>::try_from(cells)
    else {
        panic!("a customer line needs seven cells, split by `|`: {line}");
    };
    // 98765 followed by five digits that count up: not a number anyone owns.
    let tail = 1_000 + index;
    Seed {
        name: name.to_owned(),
        email: format!("{}@example.com", name.to_lowercase().replace(' ', ".")),
        phone: format!("+9198765{tail:05}"),
        city: city.to_owned(),
        plan: Plan::parse(plan),
        status: Status::parse(status),
        rupees: rupees
            .parse()
            .unwrap_or_else(|_| panic!("a whole number of rupees, not `{rupees}`: {line}")),
        since: NaiveDate::parse_from_str(since, "%Y-%m-%d")
            .unwrap_or_else(|_| panic!("a day like 2026-11-05, not `{since}`: {line}")),
        autopay: autopay == "yes",
    }
}

/// Whether the app runs in production, from the `env` setting (`APP_ENV`).
async fn in_production(ctx: &AppContext) -> bool {
    let Some(config) = ctx.try_get::<Config>().await else {
        return false;
    };
    config
        .get_string("env")
        .or_else(|| config.get_string("app.env"))
        .is_some_and(|env| env.trim().eq_ignore_ascii_case("production"))
}

/// Writes the demo customers if there are none, and the demo account.
pub async fn seed(ctx: &AppContext) -> Result<(), BoxError> {
    ensure_demo_account(ctx).await?;
    let db = ctx.get::<Db>().await?;
    let uow = db.uow(None, None).await;
    match seed_customers(&uow).await {
        Ok(()) => {
            uow.commit().await?;
            Ok(())
        }
        Err(error) => {
            uow.rollback().await?;
            Err(error)
        }
    }
}

async fn seed_customers(uow: &Uow) -> Result<(), BoxError> {
    if Customer::query().exists(uow).await? {
        return Ok(());
    }
    for seed in seeds() {
        Customer::create(uow, seed.into_customer()).await?;
    }
    Ok(())
}

/// Seeds at boot, in development and in tests.
#[injectable]
pub struct DemoData;

impl Lifecycle for DemoData {
    fn on_init(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            if in_production(&ctx).await {
                return Ok(());
            }
            seed(&ctx).await
        })
    }
}

/// `navree db:seed customers`.
#[injectable]
pub struct CustomerSeeder;

impl Seeder for CustomerSeeder {
    fn name(&self) -> &'static str {
        "customers"
    }

    fn run<'a>(&'a self, ctx: &'a AppContext, _db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move { seed(ctx).await })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn forty_customers_five_in_each_of_eight_cities() {
        let seeds = seeds();
        assert_eq!(seeds.len(), 40);
        let cities: BTreeSet<&str> = seeds.iter().map(|seed| seed.city.as_str()).collect();
        assert_eq!(cities.len(), 8);
        for city in cities {
            let count = seeds.iter().filter(|seed| seed.city == city).count();
            assert_eq!(count, 5, "{city}");
        }
    }

    #[test]
    fn emails_and_phones_are_unique_and_every_city_is_one_the_table_offers() {
        let seeds = seeds();
        let emails: BTreeSet<&str> = seeds.iter().map(|seed| seed.email.as_str()).collect();
        let phones: BTreeSet<&str> = seeds.iter().map(|seed| seed.phone.as_str()).collect();
        assert_eq!(emails.len(), seeds.len());
        assert_eq!(phones.len(), seeds.len());
        for seed in &seeds {
            assert!(
                crate::modules::customers::entity::CITIES
                    .iter()
                    .any(|(key, _)| *key == seed.city),
                "{}",
                seed.city
            );
        }
    }

    #[test]
    fn a_line_reads_into_a_customer() {
        let seed = read(
            0,
            "Test Person | Patiala | scale | trial | 1000 | 2026-01-02 | yes",
        );
        assert_eq!(seed.email, "test.person@example.com");
        assert_eq!(seed.phone, "+919876501000");
        let customer = seed.into_customer();
        assert_eq!(customer.monthly_paise, 100_000);
        assert_eq!(customer.plan, "scale");
        assert_eq!(customer.status, "trial");
        assert!(customer.autopay);
    }
}
