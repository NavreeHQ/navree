//! The attribute layer's casts through SQLite: fixed-scale decimals,
//! formatted and integer date-times, binary ids, URLs, enum lists, inbound
//! casts, value types with their own cast, cast arguments, fields stored
//! across columns, increments and serialization through casts, custom
//! equivalence, vectors, and query-time casts.

use std::collections::BTreeSet;

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use navree_db::sea_orm::ConnectionTrait;
use navree_orm::attributes::ExtraCast;
use navree_orm::cast::{Cast, CastError, Castable, InboundCast, MultiColumnCast};
use navree_orm::{Decimal, Model, OrmError, Record, Ulid, attrs};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::json;
use url::Url;
use uuid::Uuid;

use super::support::{sqlite, stored_text};

const SCHEMA: &str = r#"
CREATE TABLE ledgers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    balance NUMERIC NOT NULL,
    opened_at TEXT NOT NULL,
    local_time TEXT NOT NULL,
    day TEXT NULL,
    synced_at INTEGER NOT NULL,
    pinged_at INTEGER NULL,
    reference BLOB NOT NULL,
    batch BLOB NOT NULL,
    website TEXT NULL,
    statuses TEXT NOT NULL,
    priorities TEXT NOT NULL,
    slug TEXT NOT NULL,
    price TEXT NOT NULL,
    ratio REAL NOT NULL,
    total_cents INTEGER NOT NULL,
    total_currency TEXT NOT NULL,
    tip_cents INTEGER NULL,
    tip_currency TEXT NULL,
    visits INTEGER NOT NULL,
    code TEXT NOT NULL,
    embedding BLOB NULL
)
"#;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Draft,
    Open,
    Closed,
}

/// A priority serialized as its number (lists of it are stored as JSON
/// numbers).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    #[default]
    Low = 1,
    High = 3,
}

impl Serialize for Priority {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(*self as i64)
    }
}

impl<'de> Deserialize<'de> for Priority {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match i64::deserialize(deserializer)? {
            1 => Ok(Priority::Low),
            3 => Ok(Priority::High),
            other => Err(serde::de::Error::custom(format!("no priority {other}"))),
        }
    }
}

/// Slugs: lower-case words joined by dashes (idempotent).
pub struct Slugify;

impl InboundCast<String> for Slugify {
    fn set(value: &String) -> Result<String, CastError> {
        Ok(value
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
            .join("-"))
    }
}

/// A price in cents that picks its own cast.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Price(pub i64);

/// Prices stored as decimal text (`"12.50"`).
pub struct PriceText;

impl Cast<Price> for PriceText {
    type Stored = String;

    fn get(stored: String) -> Result<Price, CastError> {
        let decimal: Decimal = stored
            .parse()
            .map_err(|_| CastError::new(format!("`{stored}` is not a price")))?;
        let cents = (decimal * Decimal::from(100)).trunc();
        cents
            .to_string()
            .parse()
            .map(Price)
            .map_err(|_| CastError::new(format!("`{stored}` is out of range")))
    }

    fn set(price: &Price) -> Result<String, CastError> {
        Ok(format!("{}.{:02}", price.0 / 100, price.0 % 100))
    }
}

impl Castable for Price {
    type Cast = PriceText;
}

/// An integer stored divided by `SCALE` (a cast argument).
pub struct Scaled<const SCALE: i64>;

impl<const SCALE: i64> Cast<i64> for Scaled<SCALE> {
    type Stored = f64;

    fn get(stored: f64) -> Result<i64, CastError> {
        // In range for the test's ratios.
        #[allow(clippy::cast_possible_truncation)]
        Ok((stored * SCALE as f64).round() as i64)
    }

    fn set(value: &i64) -> Result<f64, CastError> {
        #[allow(clippy::cast_precision_loss)]
        Ok(*value as f64 / SCALE as f64)
    }
}

/// Money: cents and a currency, in two columns.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Money {
    pub cents: i64,
    pub currency: String,
}

pub struct MoneyColumns;

impl MultiColumnCast<Money> for MoneyColumns {
    type Stored = (i64, String);

    fn get((cents, currency): (i64, String)) -> Result<Money, CastError> {
        if currency.len() == 3 {
            Ok(Money { cents, currency })
        } else {
            Err(CastError::new(format!("`{currency}` is not a currency code")))
        }
    }

    fn set(money: &Money) -> Result<(i64, String), CastError> {
        Ok((money.cents, money.currency.clone()))
    }
}

/// A visit counter that adds up through its cast and serializes as an
/// object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Counter(pub i64);

pub struct Visits;

impl Cast<Counter> for Visits {
    type Stored = i64;

    fn get(stored: i64) -> Result<Counter, CastError> {
        Ok(Counter(stored))
    }

    fn set(counter: &Counter) -> Result<i64, CastError> {
        Ok(counter.0)
    }

    fn increment(value: &Counter, amount: &Counter) -> Result<Counter, CastError> {
        Ok(Counter(value.0 + amount.0))
    }

    fn decrement(value: &Counter, amount: &Counter) -> Result<Counter, CastError> {
        if amount.0 > value.0 {
            return Err(CastError::new("a counter cannot go below zero"));
        }
        Ok(Counter(value.0 - amount.0))
    }

    fn serialize(counter: &Counter) -> Option<Result<serde_json::Value, CastError>> {
        Some(Ok(json!({"count": counter.0})))
    }
}

/// Codes compared without regard to case (custom equivalence).
pub struct CaseInsensitive;

impl Cast<String> for CaseInsensitive {
    type Stored = String;

    fn get(stored: String) -> Result<String, CastError> {
        Ok(stored)
    }

    fn set(value: &String) -> Result<String, CastError> {
        Ok(value.clone())
    }

    fn eq_stored(a: &String, b: &String) -> bool {
        a.eq_ignore_ascii_case(b)
    }
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "ledgers", unguarded)]
pub struct Ledger {
    pub id: i64,
    #[model(cast = "decimal(2)")]
    pub balance: Decimal,
    #[model(cast = "datetime_format(\"%d/%m/%Y %H:%M\")")]
    pub opened_at: DateTime<Utc>,
    #[model(cast = "datetime_format(\"%Y-%m-%dT%H:%M:%S%:z\")")]
    pub local_time: DateTime<FixedOffset>,
    #[model(cast = "datetime_format(\"%d.%m.%Y\")")]
    pub day: Option<NaiveDate>,
    #[model(cast = "unix_timestamp")]
    pub synced_at: DateTime<Utc>,
    #[model(cast = "unix_timestamp_ms")]
    pub pinged_at: Option<DateTime<Utc>>,
    #[model(cast = "binary_uuid")]
    pub reference: Uuid,
    #[model(cast = "binary_ulid")]
    pub batch: Ulid,
    pub website: Option<Url>,
    #[model(cast = "json")]
    pub statuses: Vec<Status>,
    #[model(cast = "json")]
    pub priorities: BTreeSet<Priority>,
    #[model(inbound = Slugify)]
    pub slug: String,
    #[model(cast)]
    pub price: Price,
    #[model(cast(Scaled, 100))]
    pub ratio: i64,
    #[model(cast = MoneyColumns, columns(total_cents: i64, total_currency: String))]
    pub total: Money,
    #[model(cast = MoneyColumns, columns(tip_cents: Option<i64>, tip_currency: Option<String>))]
    pub tip: Option<Money>,
    #[model(cast = Visits)]
    pub visits: Counter,
    #[model(cast = CaseInsensitive)]
    pub code: String,
    #[model(cast = "vector")]
    pub embedding: Option<Vec<f32>>,
}

fn at(day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, day, hour, minute, second).unwrap()
}

fn ledger() -> Ledger {
    Ledger {
        balance: "12.345".parse().unwrap(),
        opened_at: at(27, 8, 30, 45),
        local_time: FixedOffset::east_opt(19_800)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 27, 14, 0, 0)
            .unwrap(),
        day: NaiveDate::from_ymd_opt(2026, 2, 1),
        synced_at: at(27, 9, 0, 0) + chrono::Duration::milliseconds(750),
        pinged_at: Some(at(27, 9, 0, 1) + chrono::Duration::microseconds(1_234_567)),
        reference: Uuid::now_v7(),
        batch: Ulid::new(),
        website: Some(Url::parse("https://navree.com/docs").unwrap()),
        statuses: vec![Status::Open, Status::Closed],
        priorities: [Priority::High, Priority::Low].into(),
        slug: "Hello, World Again".into(),
        price: Price(1250),
        ratio: 25,
        total: Money {
            cents: 999,
            currency: "EUR".into(),
        },
        tip: None,
        visits: Counter(5),
        code: "ABC".into(),
        embedding: Some(vec![0.5, -1.0, 2.25]),
        ..Ledger::default()
    }
}

#[tokio::test]
async fn every_cast_stores_its_form_and_reads_back() {
    let t = sqlite(SCHEMA).await;
    let created = Ledger::create(&t.db, ledger()).await.unwrap();
    let stored = Ledger::find_or_fail(&t.db, created.id).await.unwrap();
    assert_eq!(*stored, *created, "the record holds what is stored");

    let row = navree_orm::table("ledgers").first(&t.db).await.unwrap().unwrap();
    assert_eq!(row.get::<Decimal>("balance").unwrap().to_string(), "12.35");
    assert_eq!(row.get::<String>("opened_at").unwrap(), "27/10/2026 08:30");
    assert_eq!(row.get::<String>("local_time").unwrap(), "2026-10-27T14:00:00+05:30");
    assert_eq!(row.get::<String>("day").unwrap(), "01.02.2026");
    assert_eq!(row.get::<i64>("synced_at").unwrap(), at(27, 9, 0, 0).timestamp());
    assert_eq!(
        row.get::<i64>("pinged_at").unwrap(),
        at(27, 9, 0, 2).timestamp_millis() + 234
    );
    assert_eq!(row.get::<Vec<u8>>("reference").unwrap(), created.reference.as_bytes());
    assert_eq!(row.get::<Vec<u8>>("batch").unwrap().len(), 16);
    assert_eq!(row.get::<String>("website").unwrap(), "https://navree.com/docs");
    assert_eq!(row.get::<serde_json::Value>("statuses").unwrap(), json!(["open", "closed"]));
    assert_eq!(row.get::<serde_json::Value>("priorities").unwrap(), json!([1, 3]));
    assert_eq!(row.get::<String>("slug").unwrap(), "hello-world-again");
    assert_eq!(row.get::<String>("price").unwrap(), "12.50");
    assert!((row.get::<f64>("ratio").unwrap() - 0.25).abs() < f64::EPSILON);
    assert_eq!(row.get::<i64>("total_cents").unwrap(), 999);
    assert_eq!(row.get::<String>("total_currency").unwrap(), "EUR");
    assert_eq!(row.get::<Option<i64>>("tip_cents").unwrap(), None);
    assert_eq!(row.get::<i64>("visits").unwrap(), 5);
    assert_eq!(
        row.get::<Vec<u8>>("embedding").unwrap(),
        [0.5_f32, -1.0, 2.25].iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>()
    );

    assert_eq!(created.balance.to_string(), "12.35", "rounded in the record when saved");
    assert_eq!(created.opened_at, at(27, 8, 30, 0), "the format's precision");
    assert_eq!(created.synced_at, at(27, 9, 0, 0), "whole seconds");
    assert_eq!(created.slug, "hello-world-again", "an inbound cast's value once saved");
    let mut renamed = stored;
    renamed.slug = "Another Title".into();
    assert!(renamed.is_dirty_on(["slug"]));
    renamed.slug = "HELLO world again".into();
    assert!(renamed.is_clean(), "stored the same either way");
}

#[tokio::test]
async fn conditions_and_column_constants_go_through_the_casts() {
    let t = sqlite(SCHEMA).await;
    let created = Ledger::create(&t.db, ledger()).await.unwrap();
    let count = |filter| {
        let db = &t.db;
        async move { Ledger::query().where_(filter).count(db).await.unwrap() }
    };
    assert_eq!(count(Ledger::REFERENCE.eq(created.reference)).await, 1);
    assert_eq!(count(Ledger::REFERENCE.eq(Uuid::now_v7())).await, 0);
    assert_eq!(count(Ledger::BATCH.eq(created.batch)).await, 1);
    assert_eq!(count(Ledger::BALANCE.eq("12.345".parse::<Decimal>().unwrap())).await, 1);
    assert_eq!(count(Ledger::TOTAL_CENTS.gte(999)).await, 1);
    assert_eq!(count(Ledger::TOTAL_CURRENCY.eq("USD")).await, 0);
    assert_eq!(count(Ledger::TIP_CENTS.is_null()).await, 1);
    assert_eq!(count(Ledger::SLUG.eq("Hello World Again")).await, 1, "the inbound cast applies");
    assert_eq!(count(Ledger::RATIO.eq(25)).await, 1);
    let meta = Ledger::meta();
    assert_eq!(meta.column("tip_cents").unwrap().field(), "tip");
    assert_eq!(meta.column("tip_cents").unwrap().cast_name(), Some("MoneyColumns"));
    assert!(meta.column("tip_currency").unwrap().is_nullable());
    assert_eq!(meta.column("ratio").unwrap().cast_name(), Some("Scaled<100>"));
    assert_eq!(meta.column("price").unwrap().cast_name(), Some("castable"));
    assert_eq!(meta.column("slug").unwrap().cast_name(), Some("inbound(Slugify)"));
    assert_eq!(meta.column("balance").unwrap().cast_name(), Some("decimal(2)"));
}

#[tokio::test]
async fn fields_across_columns_track_each_column() {
    let t = sqlite(SCHEMA).await;
    let mut record = Ledger::create(&t.db, ledger()).await.unwrap();
    record.total.currency = "USD".into();
    assert!(record.is_dirty_on(["total_currency"]));
    assert!(!record.is_dirty_on(["total_cents"]));
    let dirty = record.get_dirty().unwrap();
    assert_eq!(dirty.iter().map(|(name, _)| *name).collect::<Vec<_>>(), ["total_currency"]);
    record.save(&t.db).await.unwrap();
    assert_eq!(stored_text(&t.db, "ledgers", "total_currency").await.as_deref(), Some("USD"));

    record.tip = Some(Money {
        cents: 100,
        currency: "USD".into(),
    });
    record.save(&t.db).await.unwrap();
    let stored = Ledger::find_or_fail(&t.db, record.id).await.unwrap();
    assert_eq!(stored.tip, record.tip);

    let mut stored = stored;
    stored.set_value("total_cents", navree_orm::Value::BigInt(Some(1))).unwrap();
    assert_eq!(stored.total.cents, 1);
    stored.fill(attrs! { "total_currency" => "GBP", "tip_cents" => 250 }).unwrap();
    assert_eq!(stored.total.currency, "GBP");
    assert_eq!(stored.tip.as_ref().unwrap().cents, 250);
    let error = stored.fill(attrs! { "total_currency" => "POUNDS" }).unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot cast column `total` of Ledger: `POUNDS` is not a currency code"
    );
    assert_eq!(stored.value("total_currency").unwrap(), Some(navree_orm::Value::from("GBP")));

    t.db.conn()
        .execute_unprepared("UPDATE ledgers SET tip_cents = 5, tip_currency = NULL")
        .await
        .unwrap();
    let error = Ledger::find_or_fail(&t.db, record.id).await.unwrap_err();
    assert!(error.to_string().contains("tip_currency"), "{error}");
}

#[tokio::test]
async fn custom_equivalence_and_rounding_decide_dirtiness() {
    let t = sqlite(SCHEMA).await;
    let mut record = Ledger::create(&t.db, ledger()).await.unwrap();
    record.code = "abc".into();
    assert!(record.is_clean(), "CaseInsensitive::eq_stored");
    record.balance = "12.354".parse().unwrap();
    assert!(record.is_clean(), "stored as 12.35 either way");
    record.balance = "12.36".parse().unwrap();
    assert!(record.is_dirty_on(["balance"]));
    record.opened_at = at(27, 8, 30, 59);
    assert!(record.is_clean_on(["opened_at"]), "the format drops the seconds");
}

#[tokio::test]
async fn increments_go_through_the_cast_and_the_database() {
    let t = sqlite(SCHEMA).await;
    let mut record = Ledger::create(&t.db, ledger()).await.unwrap();
    assert!(record.increment_cast(&t.db, Ledger::VISITS, Counter(3)).await.unwrap());
    assert_eq!(record.visits, Counter(8));
    assert!(record.decrement_cast(&t.db, Ledger::VISITS, Counter(2)).await.unwrap());
    assert_eq!(record.visits, Counter(6));
    assert!(record.is_clean());
    let stored = Ledger::find_or_fail(&t.db, record.id).await.unwrap();
    assert_eq!(stored.visits, Counter(6));

    let error = record.decrement_cast(&t.db, Ledger::VISITS, Counter(100)).await.unwrap_err();
    assert_eq!(error.to_string(), "cannot cast column `visits` of Ledger: a counter cannot go below zero");
    let error = record.increment_cast(&t.db, Ledger::CODE, "x".to_owned()).await.unwrap_err();
    assert!(error.to_string().contains("which the database cannot add to"), "{error}");
    let error = record
        .increment_cast(&t.db, Ledger::RATIO, 1)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not support increments"), "{error}");
    let mut unsaved = Record::new(ledger());
    let error = unsaved.increment_cast(&t.db, Ledger::VISITS, Counter(1)).await.unwrap_err();
    assert!(matches!(error, OrmError::NotPersisted { .. }), "{error:?}");
}

#[tokio::test]
async fn serialized_records_write_cast_outputs() {
    let t = sqlite(SCHEMA).await;
    let record = Ledger::create(&t.db, ledger()).await.unwrap();
    let json = record.to_value().unwrap();
    assert_eq!(json["visits"], json!({"count": 5}), "Cast::serialize");
    assert_eq!(json["opened_at"], "27/10/2026 08:30", "the format's own output");
    assert_eq!(json["synced_at"], "2026-10-27T09:00:00.000000Z", "date-times in UTC with microseconds");
    assert_eq!(json["reference"], record.reference.to_string());
    assert_eq!(json["batch"], record.batch.to_string());
    assert_eq!(json["balance"], "12.35");
    assert_eq!(json["statuses"], json!(["open", "closed"]));
    assert_eq!(json["total"], json!({"cents": 999, "currency": "EUR"}));
    assert_eq!(json["tip"], serde_json::Value::Null);
    assert_eq!(json["embedding"], json!([0.5, -1.0, 2.25]));
    assert_eq!(json["website"], "https://navree.com/docs");
}

#[tokio::test]
async fn untyped_input_reaches_every_cast_in_its_plain_form() {
    let t = sqlite(SCHEMA).await;
    let reference = Uuid::now_v7();
    let batch = Ulid::new();
    let record = Ledger::create_from(
        &t.db,
        attrs! {
            "balance" => "7.005",
            "opened_at" => "01/11/2026 10:00",
            "local_time" => "2026-11-01T10:00:00-05:00",
            "synced_at" => 1_793_089_800,
            "pinged_at" => "2026-10-27T08:30:00.123Z",
            "reference" => reference.to_string(),
            "batch" => batch.to_string(),
            "website" => "https://example.com/x",
            "statuses" => ["draft"],
            "priorities" => [3],
            "slug" => "A B",
            "price" => "3.10",
            "ratio" => 0.5,
            "total_cents" => 1,
            "total_currency" => "JPY",
            "visits" => 2,
            "code" => "x",
            "embedding" => [1, 2.5],
        },
    )
    .await
    .unwrap();
    assert_eq!(record.balance.to_string(), "7.01");
    assert_eq!(record.opened_at, Utc.with_ymd_and_hms(2026, 11, 1, 10, 0, 0).unwrap());
    assert_eq!(record.local_time.offset().local_minus_utc(), -18_000);
    assert_eq!(record.synced_at.timestamp(), 1_793_089_800);
    assert_eq!(record.pinged_at.unwrap().timestamp_subsec_millis(), 123);
    assert_eq!((record.reference, record.batch), (reference, batch));
    assert_eq!(record.statuses, [Status::Draft]);
    assert_eq!(record.priorities, [Priority::High].into());
    assert_eq!(record.price, Price(310));
    assert_eq!(record.ratio, 50, "the input is the stored form for a custom cast");
    assert_eq!(record.total.currency, "JPY");
    assert_eq!(record.visits, Counter(2));
    assert_eq!(record.embedding.as_deref(), Some(&[1.0, 2.5][..]));
    let error = Record::new(Ledger::default())
        .fill(attrs! { "reference" => "not-a-uuid" })
        .unwrap_err();
    assert!(matches!(error, OrmError::Cast { ref column, .. } if column == "reference"), "{error:?}");
}

#[tokio::test]
async fn stored_values_that_do_not_fit_are_cast_errors_naming_the_column() {
    let t = sqlite(SCHEMA).await;
    Ledger::create(&t.db, ledger()).await.unwrap();
    t.db.conn()
        .execute_unprepared("UPDATE ledgers SET reference = x'0102'")
        .await
        .unwrap();
    let error = Ledger::query().first(&t.db).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot cast column `reference` of Ledger: 2 bytes are not a binary UUID (16 bytes)"
    );
    t.db.conn()
        .execute_unprepared("UPDATE ledgers SET reference = x'00000000000000000000000000000000', opened_at = 'soon'")
        .await
        .unwrap();
    let error = Ledger::query().first(&t.db).await.unwrap_err();
    assert!(error.to_string().starts_with("cannot cast column `opened_at` of Ledger: `soon` does not match the format"), "{error}");
}

#[tokio::test]
async fn query_time_casts_type_extra_columns() {
    let t = sqlite(SCHEMA).await;
    Ledger::create(&t.db, ledger()).await.unwrap();
    let records = Ledger::query()
        .select_raw("balance > 10 AS rich", [] as [i64; 0])
        .select_raw("'{\"a\": 1}' AS document", [] as [i64; 0])
        .select_raw("balance / 3 AS third", [] as [i64; 0])
        .with_casts([
            ("rich", ExtraCast::Bool),
            ("document", ExtraCast::Json),
            ("third", ExtraCast::Decimal(Some(3))),
        ])
        .get(&t.db)
        .await
        .unwrap();
    let record = &records[0];
    assert_eq!(record.extra::<bool>("rich").unwrap(), Some(true));
    assert_eq!(record.extra::<serde_json::Value>("document").unwrap(), Some(json!({"a": 1})));
    let json = record.to_value().unwrap();
    assert_eq!(json["rich"], true);
    assert_eq!(json["document"], json!({"a": 1}));
    assert_eq!(json["third"], "4.117");
    let error = Ledger::query()
        .select_raw("'x' AS rich", [] as [i64; 0])
        .with_casts([("rich", ExtraCast::Bool)])
        .get(&t.db)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "cannot cast column `rich` of Ledger: `x` is not a boolean");
}
