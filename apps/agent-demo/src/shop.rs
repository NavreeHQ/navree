//! What the shop stores: its orders, with how each was paid and where it stands; who
//! may do what; and a seed that writes the same shop on every start.
//!
//! Every name, order and number here is invented.

use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveTime, TimeZone, Utc};
use navree::access::{AbilityCtx, AccessService, Rules};
use navree::auth::AuthService;
use navree::db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree::db::{Db, MigrationSet};
use navree::orm::Model;
use navree::prelude::BoxError;

/// The shop the app runs.
pub const SHOP: &str = "Kesar Kitchen Supplies";

/// The order every walkthrough starts from.
pub const SAMPLE_ORDER: &str = "KK-1042";

/// The manager, who may refund and mark orders delivered.
pub const MANAGER: (&str, &str) = ("Simran Kaur", "simran.kaur@kesar.example");

/// A member of staff, who may read orders and nothing more.
pub const STAFF: (&str, &str) = ("Arjun Mehta", "arjun.mehta@kesar.example");

/// The password of both demo accounts.
pub const DEMO_PASSWORD: &str = "demo-password-2026";

/// India Standard Time.
pub fn ist() -> FixedOffset {
    FixedOffset::east_opt(19_800).expect("a real offset")
}

/// An order, as stored. Amounts are in paise.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "shop_orders", subject = "Order", permission_prefix = "orders")]
pub struct Order {
    pub id: i64,
    /// `KK-1042`.
    pub number: String,
    pub customer: String,
    pub city: String,
    /// `upi`, `card`, `net_banking` or `cod`.
    pub method: String,
    pub total: i64,
    pub refunded: i64,
    /// `paid`, `delivered` or `refunded`.
    pub status: String,
    /// Unix seconds.
    pub placed_at: i64,
}

impl Order {
    /// What is left to refund, in paise.
    pub fn left(&self) -> i64 {
        (self.total - self.refunded).max(0)
    }

    /// How it was paid, in words.
    pub fn method_label(&self) -> &'static str {
        method_label(&self.method)
    }

    /// Where it stands, in words.
    pub fn status_label(&self) -> &'static str {
        match self.status.as_str() {
            "delivered" => "Delivered",
            "refunded" => "Refunded",
            _ => "Paid",
        }
    }
}

/// A payment method's name.
pub fn method_label(method: &str) -> &'static str {
    match method {
        "upi" => "UPI",
        "card" => "Card",
        "net_banking" => "Net banking",
        _ => "Cash on delivery",
    }
}

/// Every payment method, in the order the shop lists them.
pub const METHODS: [&str; 4] = ["upi", "card", "net_banking", "cod"];

/// `paise` as rupees, grouped the Indian way: `₹1,00,000`, `₹1,250.50`.
pub fn rupees(paise: i64) -> String {
    let sign = if paise < 0 { "−" } else { "" };
    let paise = paise.abs();
    let digits = (paise / 100).to_string();
    let grouped = if digits.len() <= 3 {
        digits
    } else {
        let (head, last) = digits.split_at(digits.len() - 3);
        let mut groups = Vec::new();
        let mut rest = head;
        while rest.len() > 2 {
            let (front, pair) = rest.split_at(rest.len() - 2);
            groups.push(pair);
            rest = front;
        }
        if !rest.is_empty() {
            groups.push(rest);
        }
        groups.reverse();
        format!("{},{last}", groups.join(","))
    };
    match paise % 100 {
        0 => format!("{sign}₹{grouped}"),
        fraction => format!("{sign}₹{grouped}.{fraction:02}"),
    }
}

/// When a period of days starts, in IST: today, this week (from Monday) or this
/// month, as unix seconds.
pub fn period_start(period: &str, now: DateTime<Utc>) -> i64 {
    let local = now.with_timezone(&ist()).date_naive();
    let day = match period {
        "today" => local,
        "this_week" => local - Duration::days(i64::from(local.weekday().num_days_from_monday())),
        _ => local.with_day(1).unwrap_or(local),
    };
    ist()
        .from_local_datetime(&day.and_time(NaiveTime::MIN))
        .single()
        .map_or(0, |start| start.timestamp())
}

/// Everyone signed in may read orders; managers may also change them (refund, mark
/// delivered).
pub fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    if ctx.user.is_none() {
        return;
    }
    rules.can("read", "Order");
    if ctx.has_role("manager") {
        rules.can("update", "Order");
    }
    ctx.grant_permissions(rules);
}

struct CreateOrders;

impl Migration for CreateOrders {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("shop_orders", |t| {
                t.big_integer("id").primary();
                t.string("number").length(20);
                t.string("customer").length(80);
                t.string("city").length(40);
                t.string("method").length(20);
                t.big_integer("total");
                t.big_integer("refunded").default(0);
                t.string("status").length(20);
                t.big_integer("placed_at");
                t.index(["placed_at"]).name("shop_orders_placed_at_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("shop_orders").await
    }
}

/// The shop's migrations.
pub fn migrations() -> MigrationSet {
    MigrationSet::new("shop").migration(SchemaMigration::new(
        "m20261106_000001_create_shop_orders",
        CreateOrders,
    ))
}

const CUSTOMERS: [(&str, &str); 16] = [
    ("Harpreet Kaur", "Ludhiana"),
    ("Gurpreet Singh", "Amritsar"),
    ("Neha Arora", "Chandigarh"),
    ("Rohit Malhotra", "Delhi"),
    ("Jaspreet Sandhu", "Jalandhar"),
    ("Pooja Bansal", "Patiala"),
    ("Vikram Chopra", "Gurugram"),
    ("Kavita Mehra", "Jaipur"),
    ("Manpreet Gill", "Mohali"),
    ("Ritu Aggarwal", "Dehradun"),
    ("Sandeep Bhatia", "Bathinda"),
    ("Meenakshi Sethi", "Shimla"),
    ("Amandeep Kaur", "Ludhiana"),
    ("Rajinder Sharma", "Ambala"),
    ("Sunita Verma", "Panipat"),
    ("Deepak Khanna", "Noida"),
];

/// A small generator that gives the same shop on every start.
struct Seq(u64);

impl Seq {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below.max(1)
    }
}

/// The orders of the last 30 days before `now`: about three a day, most paid by UPI,
/// and order [`SAMPLE_ORDER`] by Harpreet Kaur this morning.
pub fn orders(now: DateTime<Utc>) -> Vec<Order> {
    let mut seq = Seq(1042);
    let mut orders = Vec::new();
    for n in 0..90_i64 {
        let id = 1000 + n;
        let (customer, city) = CUSTOMERS[seq.next(CUSTOMERS.len() as u64) as usize];
        let method = match seq.next(100) {
            0..=54 => "upi",
            55..=74 => "card",
            75..=84 => "net_banking",
            _ => "cod",
        };
        let total = (349 + seq.next(12_650) as i64) * 100;
        let placed_at = now.timestamp() - (90 - n) * 8 * 3600 - seq.next(3600) as i64;
        let status = match seq.next(10) {
            0 => "refunded",
            1..=5 => "delivered",
            _ => "paid",
        };
        orders.push(Order {
            id,
            number: format!("KK-{id}"),
            customer: customer.to_owned(),
            city: city.to_owned(),
            method: method.to_owned(),
            total,
            refunded: if status == "refunded" { total } else { 0 },
            status: status.to_owned(),
            placed_at,
        });
    }
    if let Some(sample) = orders.iter_mut().find(|order| order.number == SAMPLE_ORDER) {
        sample.customer = "Harpreet Kaur".to_owned();
        sample.city = "Ludhiana".to_owned();
        sample.method = "upi".to_owned();
        sample.total = 125_000;
        sample.refunded = 0;
        sample.status = "paid".to_owned();
        sample.placed_at = now.timestamp() - 2 * 3600;
    }
    orders
}

/// Writes the shop and its two people when the database has none yet.
pub async fn seed(
    db: &Db,
    auth: &AuthService,
    access: &AccessService,
    now: DateTime<Utc>,
) -> Result<(), BoxError> {
    if Order::query().get(db).await?.into_models().is_empty() {
        for order in orders(now) {
            Order::create(db, order).await?;
        }
    }
    for ((name, email), role) in [(MANAGER, "manager"), (STAFF, "staff")] {
        if auth.find_by_email(email).await?.is_some() {
            continue;
        }
        let user = auth.create_user(name, email, DEMO_PASSWORD).await?;
        let uow = db.uow(None, None).await;
        let _ = access.create_role(&uow, role, None).await;
        access.assign_role(&uow, user.id, role).await?;
        uow.commit().await?;
    }
    Ok(())
}

/// The shop's database, for the pages.
pub type ShopDb = Arc<Db>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rupees_are_grouped_the_indian_way() {
        assert_eq!(rupees(125_000), "₹1,250");
        assert_eq!(rupees(10_000_000), "₹1,00,000");
        assert_eq!(rupees(123_456_789), "₹12,34,567.89");
        assert_eq!(rupees(5_000), "₹50");
        assert_eq!(rupees(-125_050), "−₹1,250.50");
    }

    #[test]
    fn the_seed_is_the_same_shop_every_time() {
        let now = Utc.with_ymd_and_hms(2026, 11, 6, 6, 30, 0).unwrap();
        let first = orders(now);
        assert_eq!(first.len(), 90);
        assert_eq!(
            first.iter().map(|o| o.total).sum::<i64>(),
            orders(now).iter().map(|o| o.total).sum::<i64>()
        );
        let sample = first.iter().find(|o| o.number == SAMPLE_ORDER).unwrap();
        assert_eq!((sample.method.as_str(), sample.total), ("upi", 125_000));
        assert!(first.iter().filter(|o| o.method == "upi").count() > first.len() / 3);
    }

    #[test]
    fn periods_start_in_ist() {
        // Friday 6 November 2026, 12:00 IST.
        let now = Utc.with_ymd_and_hms(2026, 11, 6, 6, 30, 0).unwrap();
        let monday = ist().with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap();
        assert_eq!(period_start("this_week", now), monday.timestamp());
        let first = ist().with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap();
        assert_eq!(period_start("this_month", now), first.timestamp());
    }
}
