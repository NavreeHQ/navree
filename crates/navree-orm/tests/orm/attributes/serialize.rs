//! Serialized records: serde's keys and customizations, one date-time
//! format, cast outputs, appended accessors, hidden and visible names per
//! model, record and collection, extras and relations, failures naming the
//! column, and subsets with `only` / `except`.

use std::collections::HashMap;

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use navree_orm::attributes::ExtraCast;
use navree_orm::{Collection, Kind, Model, OrmError, Record, accessors};
use serde::{Serialize, Serializer};
use serde_json::json;

use super::support::sqlite;

const SCHEMA: &str = r#"
CREATE TABLE people (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    email TEXT NOT NULL,
    secret_token TEXT NOT NULL,
    nickname TEXT NULL,
    born_on TEXT NULL,
    last_seen_at TEXT NULL,
    score INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE remarks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    person_id INTEGER NOT NULL,
    body TEXT NOT NULL
)
"#;

fn as_text<S: Serializer>(score: &i64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&score.to_string())
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[model(
    table = "people",
    timestamps,
    unguarded,
    hidden(secret_token, "remarks_count"),
    appends(full_name)
)]
pub struct Person {
    pub id: i64,
    pub first_name: String,
    pub last_name: String,
    #[serde(rename = "mail")]
    pub email: String,
    pub secret_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    pub born_on: Option<NaiveDate>,
    pub last_seen_at: Option<DateTime<FixedOffset>>,
    #[serde(serialize_with = "as_text")]
    pub score: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[accessors]
impl Person {
    /// "Ada Lovelace".
    #[accessor]
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
    }

    /// "A.L.".
    #[accessor(name = "initials")]
    pub fn initials_of(&self) -> String {
        format!(
            "{}.{}.",
            self.first_name.chars().next().unwrap_or('?'),
            self.last_name.chars().next().unwrap_or('?')
        )
    }

    /// A value JSON cannot hold (keys that are not strings).
    #[accessor]
    pub fn broken(&self) -> HashMap<(i32, i32), i32> {
        HashMap::from([((1, 2), 3)])
    }
}

/// The same table, keeping only some names and writing dates its own way.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "people",
    unguarded,
    visible(id, first_name, created_at, "rank"),
    serialize_dates_with = local_dates
)]
pub struct Badge {
    pub id: i64,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub secret_token: String,
    pub nickname: Option<String>,
    pub born_on: Option<NaiveDate>,
    pub last_seen_at: Option<DateTime<FixedOffset>>,
    pub score: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Dates as `YYYY-MM-DD HH:MM` in the date-time's own offset.
fn local_dates(at: &DateTime<FixedOffset>) -> serde_json::Value {
    json!(at.format("%Y-%m-%d %H:%M %:z").to_string())
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "remarks", unguarded)]
pub struct Remark {
    pub id: i64,
    #[model(hidden)]
    pub person_id: i64,
    pub body: String,
}

/// A value whose `Serialize` fails.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opaque;

impl Serialize for Opaque {
    fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("opaque values do not serialize"))
    }
}

impl navree_orm::Field for Opaque {
    fn to_value(&self) -> navree_orm::Value {
        navree_orm::Value::String(String::new())
    }

    fn null() -> navree_orm::Value {
        navree_orm::Value::Null(Kind::String)
    }

    fn from_sql(
        _row: &navree_db::Row,
        _column: &str,
    ) -> Result<Self, navree_orm::FieldError> {
        Ok(Opaque)
    }

    fn from_value(_value: navree_orm::Value) -> Result<Self, navree_orm::FieldError> {
        Ok(Opaque)
    }

    fn type_name() -> std::borrow::Cow<'static, str> {
        "Opaque".into()
    }
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "remarks")]
pub struct Odd {
    pub id: i64,
    pub body: Opaque,
}

fn ada() -> Person {
    Person {
        id: 1,
        first_name: "Ada".into(),
        last_name: "Lovelace".into(),
        email: "ada@example.com".into(),
        secret_token: "s3cret".into(),
        nickname: None,
        born_on: NaiveDate::from_ymd_opt(1815, 12, 10),
        last_seen_at: Some(
            FixedOffset::east_opt(3600)
                .unwrap()
                .with_ymd_and_hms(2026, 10, 27, 10, 30, 0)
                .unwrap(),
        ),
        score: 42,
        created_at: Utc.with_ymd_and_hms(2026, 10, 27, 8, 0, 0).unwrap()
            + chrono::Duration::nanoseconds(123_456_789),
        updated_at: Utc.with_ymd_and_hms(2026, 10, 27, 9, 0, 0).unwrap(),
    }
}

#[test]
fn records_serialize_with_serdes_keys_one_date_format_and_appends() {
    let person = Record::new(ada());
    assert_eq!(
        person.to_json().unwrap(),
        r#"{"id":1,"firstName":"Ada","lastName":"Lovelace","mail":"ada@example.com","bornOn":"1815-12-10","lastSeenAt":"2026-10-27T09:30:00.000000Z","score":"42","createdAt":"2026-10-27T08:00:00.123456Z","updatedAt":"2026-10-27T09:00:00.000000Z","full_name":"Ada Lovelace"}"#
    );
    assert_eq!(
        serde_json::to_value(&person).unwrap(),
        person.to_value().unwrap()
    );
    let pretty = person.to_json_pretty().unwrap();
    assert!(
        pretty.starts_with("{\n  \"id\": 1,\n  \"firstName\": \"Ada\""),
        "{pretty}"
    );
}

#[test]
fn visibility_changes_per_record() {
    let mut person = Record::new(ada());
    assert_eq!(person.hidden(), ["secret_token", "remarks_count"]);
    assert!(person.visible().is_empty());
    person.make_hidden(["first_name", "full_name"]);
    let json = person.to_value().unwrap();
    assert!(json.get("firstName").is_none() && json.get("full_name").is_none());
    person.make_visible(["secret_token"]);
    assert_eq!(person.to_value().unwrap()["secretToken"], "s3cret");
    person.set_visible(["id", "mail"]);
    assert_eq!(
        person.to_value().unwrap(),
        json!({"id": 1, "mail": "ada@example.com"})
    );
    person.merge_visible(["last_name"]);
    assert_eq!(person.visible(), ["id", "mail", "last_name"]);
    person.make_visible(["first_name"]);
    assert_eq!(
        person.to_value().unwrap(),
        json!({"id": 1, "firstName": "Ada", "lastName": "Lovelace", "mail": "ada@example.com"})
    );
    person
        .set_hidden(Vec::<String>::new())
        .set_visible(Vec::<String>::new());
    assert!(person.to_value().unwrap().get("secretToken").is_some());

    let mut person = Record::new(ada());
    person.make_hidden_if(|p: &Record<Person>| p.score > 40, ["score"]);
    person.make_hidden_if(false, ["id"]);
    let json = person.to_value().unwrap();
    assert!(json.get("score").is_none());
    assert_eq!(json["id"], 1);
    person
        .make_visible_if(true, ["score"])
        .merge_hidden(["mail"]);
    let json = person.to_value().unwrap();
    assert_eq!(json["score"], "42");
    assert!(json.get("mail").is_none(), "serde keys work too");
}

#[test]
fn appends_change_per_record() {
    let mut person = Record::new(ada());
    assert_eq!(
        person.full_name(),
        "Ada Lovelace",
        "an accessor is a plain method of the model"
    );
    assert_eq!(person.appends(), ["full_name"]);
    assert!(person.has_appended("full_name"));
    person.append(["initials"]).unwrap();
    assert_eq!(person.to_value().unwrap()["initials"], "A.L.");
    let error = person.append(["age"]).unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: Person has no accessor named `age` to append (its accessors: full_name, initials, broken)"
    );
    person.set_appends(["initials"]).unwrap();
    let json = person.to_value().unwrap();
    assert!(json.get("full_name").is_none());
    person.without_appends();
    assert!(person.appends().is_empty());
    assert!(person.to_value().unwrap().get("initials").is_none());
    person.merge_appends(["full_name"]).unwrap();
    assert_eq!(person.appends(), ["full_name"]);
    assert_eq!(
        Person::accessors()
            .iter()
            .map(|a| a.name())
            .collect::<Vec<_>>(),
        ["full_name", "initials", "broken"]
    );
    assert_eq!(Person::accessors()[0].rust_type(), "String");
    assert_eq!(Person::meta().appended_names(), ["full_name"]);
    let mut badge = Record::new(Badge::default());
    let error = badge.append(["full_name"]).unwrap_err();
    assert!(
        error.to_string().contains("it has no #[accessors] block"),
        "{error}"
    );
}

#[test]
fn visible_lists_and_custom_date_formats_per_model() {
    let mut badge = Record::new(Badge {
        id: 7,
        first_name: "Grace".into(),
        created_at: Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap(),
        ..Badge::default()
    });
    badge.set_extra("rank", 1_i64);
    badge.set_extra("other", 2_i64);
    assert_eq!(
        badge.to_value().unwrap(),
        json!({"id": 7, "first_name": "Grace", "created_at": "2026-01-02 03:04 +00:00", "rank": 1})
    );
}

#[tokio::test]
async fn extras_and_relations_serialize_with_the_records_visibility() {
    let t = sqlite(SCHEMA).await;
    let person = Person::create(&t.db, ada()).await.unwrap();
    for body in ["first", "second"] {
        Remark::create(
            &t.db,
            Remark {
                person_id: person.id,
                body: body.into(),
                ..Remark::default()
            },
        )
        .await
        .unwrap();
    }
    let mut people = Person::query()
        .select_raw(
            "(SELECT COUNT(*) FROM remarks) AS remarks_count",
            [] as [i64; 0],
        )
        .select_raw("'2026-05-06 07:08:09' AS reviewed_at", [] as [i64; 0])
        .with_casts([("reviewed_at", ExtraCast::DateTime)])
        .get(&t.db)
        .await
        .unwrap();
    let remarks = Remark::query().get(&t.db).await.unwrap();
    let person = &mut people[0];
    person.relations_mut().set("remarks", remarks);
    person.relations_mut().set("mentor", None::<Record<Person>>);
    person
        .relations_mut()
        .set("replies", Collection::<Remark>::new());
    let json = person.to_value().unwrap();
    assert!(json.get("remarks_count").is_none(), "hidden by the model");
    assert_eq!(
        json["reviewed_at"], "2026-05-06T07:08:09.000000Z",
        "extras use the date format"
    );
    assert_eq!(
        json["remarks"],
        json!([{"id": 1, "body": "first"}, {"id": 2, "body": "second"}])
    );
    assert_eq!(json["mentor"], serde_json::Value::Null, "an empty to-one");
    assert_eq!(json["replies"], json!([]), "an empty to-many");
    person
        .make_hidden(["remarks"])
        .make_visible(["remarks_count"]);
    let json = person.to_value().unwrap();
    assert!(json.get("remarks").is_none());
    assert_eq!(json["remarks_count"], 2);
    assert_eq!(person.extra::<i64>("remarks_count").unwrap(), Some(2));
}

#[tokio::test]
async fn collections_serialize_as_arrays_and_change_every_record() {
    let t = sqlite(SCHEMA).await;
    for name in ["Ada", "Grace"] {
        Person::create(
            &t.db,
            Person {
                id: 0,
                first_name: name.into(),
                ..ada()
            },
        )
        .await
        .unwrap();
    }
    let mut people: Collection<Person> = Person::all(&t.db).await.unwrap();
    people
        .make_hidden(["mail", "score"])
        .append(["initials"])
        .unwrap();
    let json = serde_json::to_value(&people).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 2);
    for (person, first) in json.as_array().unwrap().iter().zip(["Ada", "Grace"]) {
        assert_eq!(person["firstName"], first);
        assert!(person.get("mail").is_none() && person.get("score").is_none());
        assert!(person.get("initials").is_some());
    }
    people.set_visible(["id"]).without_appends();
    assert_eq!(
        serde_json::to_value(&people).unwrap(),
        json!([{"id": 1}, {"id": 2}])
    );
    people
        .merge_visible(["first_name"])
        .make_visible(["mail"])
        .set_hidden(["id"]);
    assert_eq!(
        serde_json::to_value(&people).unwrap(),
        json!([{"firstName": "Ada", "mail": "ada@example.com"}, {"firstName": "Grace", "mail": "ada@example.com"}])
    );
    people
        .merge_hidden(["mail"])
        .set_appends(["full_name"])
        .unwrap();
    assert_eq!(
        serde_json::to_value(&people).unwrap(),
        json!([{"firstName": "Ada"}, {"firstName": "Grace"}]),
        "visible lists filter appended values too"
    );
    assert!(people.append(["nope"]).is_err());
}

#[test]
fn serialization_failures_name_the_model_and_column() {
    let mut person = Record::new(ada());
    person.append(["broken"]).unwrap();
    let error = person.to_value().unwrap_err();
    assert!(
        matches!(&error, OrmError::Serialize { column: Some(column), .. } if column == "broken"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "cannot serialize `broken` of Person: key must be a string"
    );
    let odd = Record::new(Odd::default());
    let error = odd.to_json().unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot serialize `body` of Odd: opaque values do not serialize"
    );
    let error = serde_json::to_value(&odd).unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot serialize `body` of Odd: opaque values do not serialize"
    );
}

#[test]
fn only_and_except_pick_values_whatever_the_visibility() {
    let person = Record::new(ada());
    assert_eq!(
        serde_json::Value::Object(
            person
                .only(["first_name", "secret_token", "initials", "nope", "mail"])
                .unwrap()
        ),
        json!({
            "first_name": "Ada",
            "secret_token": "s3cret",
            "initials": null,
            "nope": null,
            "mail": "ada@example.com",
        })
    );
    let except = person
        .except([
            "secret_token",
            "score",
            "created_at",
            "updated_at",
            "born_on",
        ])
        .unwrap();
    assert_eq!(
        serde_json::Value::Object(except),
        json!({
            "id": 1,
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@example.com",
            "last_seen_at": "2026-10-27T09:30:00.000000Z",
        }),
        "keyed by column; columns serde leaves out are absent"
    );
}

/// A remark that counts its words once: an expensive computed value cached
/// in a field that is not a column.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "remarks", unguarded, appends(word_count))]
pub struct Essay {
    pub id: i64,
    pub person_id: i64,
    pub body: String,
    #[model(skip)]
    #[serde(skip)]
    pub words: std::sync::OnceLock<usize>,
}

#[accessors]
impl Essay {
    /// How many words the body has (counted once per record).
    #[accessor]
    pub fn word_count(&self) -> usize {
        *self
            .words
            .get_or_init(|| self.body.split_whitespace().count())
    }
}

#[tokio::test]
async fn expensive_accessors_cache_in_fields_that_are_not_columns() {
    let t = sqlite(SCHEMA).await;
    Remark::create(
        &t.db,
        Remark {
            person_id: 1,
            body: "one two three".into(),
            ..Remark::default()
        },
    )
    .await
    .unwrap();
    let essay = Essay::query().first(&t.db).await.unwrap().unwrap();
    assert_eq!(essay.words.get(), None, "not computed until asked");
    assert_eq!(essay.to_value().unwrap()["word_count"], 3);
    assert_eq!(essay.words.get(), Some(&3), "kept on the record");
    assert_eq!(essay.word_count(), 3);
    assert!(essay.is_clean(), "the cache is not a column");
}

/// Whether `T` implements `Display`, decided at compile time: the inherent
/// method exists only when it does, and wins over the trait's.
struct DisplayProbe<T>(std::marker::PhantomData<T>);

trait NoDisplay {
    fn displays(&self) -> bool {
        false
    }
}

impl<T> NoDisplay for DisplayProbe<T> {}

impl<T: std::fmt::Display> DisplayProbe<T> {
    fn displays(&self) -> bool {
        true
    }
}

#[test]
fn records_have_no_display_and_templates_get_their_values() {
    assert!(
        DisplayProbe::<String>(std::marker::PhantomData).displays(),
        "the probe tells"
    );
    assert!(
        !DisplayProbe::<Record<Person>>(std::marker::PhantomData).displays(),
        "records do not print themselves"
    );
    let mut person = Record::new(ada());
    person.first_name = "<b>Ada</b>".into();
    assert_eq!(
        person.to_value().unwrap()["firstName"],
        "<b>Ada</b>",
        "values stay as they are, for the template to escape"
    );
}
