//! Mass assignment: fillable and guarded columns, unguarded scopes, the
//! discard policy, conversion of untyped input through casts and
//! mutators, JSON paths, and creating and updating from attributes.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};
use navree_orm::attributes::{Attributes, DiscardPolicy, unguarded, with_discard_policy};
use navree_orm::{Model, OrmError, Record, attrs};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::support::sqlite;

const SCHEMA: &str = r#"
CREATE TABLE members (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    role TEXT NOT NULL,
    is_admin BOOLEAN NOT NULL DEFAULT 0,
    age INTEGER NULL,
    settings TEXT NOT NULL,
    profile TEXT NULL,
    joined_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE posts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    author_id INTEGER NOT NULL,
    published BOOLEAN NOT NULL
);
CREATE TABLE labels (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL
);
CREATE TABLE notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    body TEXT NOT NULL,
    pinned BOOLEAN NOT NULL
);
CREATE TABLE people (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    full_name TEXT NOT NULL,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL
)
"#;

/// A full name that also sets the first and last names.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "people", fillable(full_name))]
pub struct Person {
    pub id: i64,
    #[model(set_with = split_name)]
    pub full_name: String,
    pub first_name: String,
    pub last_name: String,
}

/// Sets the full name (spaces collapsed) and the names it splits into.
fn split_name(person: &mut Person, full_name: String) {
    let full_name = full_name.split_whitespace().collect::<Vec<_>>().join(" ");
    let (first, last) = full_name.split_once(' ').unwrap_or((&full_name, ""));
    person.first_name = first.to_owned();
    person.last_name = last.to_owned();
    person.full_name = full_name;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Member,
    Admin,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub bio: String,
    #[serde(default)]
    pub links: Vec<String>,
}

/// Fillable columns, a mutator, casts and JSON columns.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "members",
    timestamps,
    fillable(name, email, role, age, settings, joined_at, "profile->bio")
)]
pub struct Member {
    pub id: i64,
    pub name: String,
    #[model(set_with = normalize_email)]
    pub email: String,
    #[model(cast = "string_enum")]
    pub role: Role,
    pub is_admin: bool,
    pub age: Option<i32>,
    pub settings: serde_json::Value,
    #[model(cast = "json")]
    pub profile: Option<Profile>,
    pub joined_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Emails are stored trimmed and lower-cased.
fn normalize_email(member: &mut Member, email: String) {
    member.email = email.trim().to_lowercase();
}

/// Every column but the guarded ones is fillable.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "posts", guarded(id, author_id))]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub author_id: i64,
    pub published: bool,
}

/// Neither fillable nor guarded: totally guarded.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "labels")]
pub struct Label {
    pub id: i64,
    pub name: String,
}

/// Never guarded.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "notes", unguarded)]
pub struct Note {
    pub id: i64,
    pub body: String,
    pub pinned: bool,
}

/// Fillable and guarded declared on the fields.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "posts")]
pub struct Draft {
    #[model(guarded)]
    pub id: i64,
    #[model(fillable)]
    pub title: String,
    #[model(fillable)]
    pub body: String,
    pub author_id: i64,
    pub published: bool,
}

#[test]
fn fill_sets_the_fillable_columns_and_leaves_the_rest() {
    let mut member = Record::new(Member::default());
    member
        .fill(attrs! {
            "name" => "Anu",
            "email" => "  Anu@Example.COM ",
            "is_admin" => true,
            "nonsense" => 1,
        })
        .unwrap();
    assert_eq!(member.name, "Anu");
    assert_eq!(member.email, "anu@example.com", "through the mutator");
    assert!(!member.is_admin, "not fillable: left out");
}

#[test]
fn untyped_input_is_converted_through_types_and_casts() {
    let mut member = Record::new(Member::default());
    member
        .fill(attrs! {
            "role" => "admin",
            "age" => "42",
            "settings" => json!({"theme": "dark"}),
            "joined_at" => "2026-10-27T10:30:00+02:00",
            "profile->bio" => "Mathematician",
        })
        .unwrap();
    assert_eq!(member.role, Role::Admin);
    assert_eq!(member.age, Some(42));
    assert_eq!(member.settings, json!({"theme": "dark"}));
    assert_eq!(
        member.joined_at,
        Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 0).unwrap()
    );
    assert_eq!(member.profile.as_ref().unwrap().bio, "Mathematician");

    member.fill(attrs! { "age" => null }).unwrap();
    assert_eq!(member.age, None);
}

#[test]
fn input_that_does_not_fit_changes_nothing() {
    let mut member = Record::new(Member {
        name: "Before".into(),
        ..Member::default()
    });
    let error = member
        .fill(attrs! { "name" => "After", "role" => "emperor" })
        .unwrap_err();
    assert!(
        matches!(error, OrmError::Cast { ref column, .. } if column == "role"),
        "{error:?}"
    );
    assert_eq!(member.name, "Before");
    let error = member.fill(attrs! { "age" => "old" }).unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot read column `age` of Member as Option<i32>: `old` is not an integer"
    );
    let error = member.fill(attrs! { "name" => null }).unwrap_err();
    assert!(error.to_string().contains("the input is null"), "{error}");
}

#[test]
fn totally_guarded_models_refuse_fill_and_accept_force_fill() {
    let mut label = Record::new(Label::default());
    let error = label.fill(attrs! { "name" => "vip" }).unwrap_err();
    assert!(
        matches!(&error, OrmError::MassAssignment { totally_guarded: true, columns, .. } if columns == &["name"]),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "cannot mass-assign `name` on Label: Label declares no fillable columns; list them with #[model(fillable(..))] (or guard some with #[model(guarded(..))]), or use force_fill"
    );
    label.fill(Attributes::new()).unwrap();
    label.force_fill(attrs! { "name" => "vip" }).unwrap();
    assert_eq!(label.name, "vip");
}

#[test]
fn guarded_models_fill_every_other_real_column() {
    let mut post = Record::new(Post::default());
    post.fill(attrs! {
        "title" => "Hello",
        "published" => "true",
        "author_id" => 9,
        "id" => 3,
        "title->x" => "path into text",
    })
    .unwrap();
    assert_eq!(post.title, "Hello");
    assert!(post.published);
    assert_eq!((post.author_id, post.id), (0, 0), "guarded");
}

#[test]
fn unknown_columns_and_paths_into_plain_columns_never_reach_the_model() {
    let mut post = Record::new(Post::default());
    let error = post.force_fill(attrs! { "nope" => 1 }).unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: Post has no column `nope`"
    );
    let error = post.force_fill(attrs! { "title->x" => 1 }).unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: cannot fill `title->x`: `title` of Post is not a JSON column"
    );
}

#[tokio::test]
async fn a_failing_discard_policy_refuses_before_changing_anything() {
    let mut member = Record::new(Member::default());
    let error = with_discard_policy(DiscardPolicy::Fail, async {
        member.fill(attrs! { "name" => "Anu", "is_admin" => true, "nope" => 1 })
    })
    .await
    .unwrap_err();
    assert!(
        matches!(&error, OrmError::MassAssignment { totally_guarded: false, columns, .. } if columns == &["is_admin", "nope"]),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "cannot mass-assign `is_admin`, `nope` on Member: not fillable; add them to #[model(fillable(..))], or use force_fill"
    );
    assert_eq!(member.name, "", "nothing was set");
}

/// What a reporting discard policy saw: the model and the discarded keys.
type Discarded = Vec<(String, Vec<String>)>;

#[tokio::test]
async fn a_reporting_discard_policy_calls_its_handler() {
    let seen: Arc<Mutex<Discarded>> = Arc::default();
    let handler = {
        let seen = Arc::clone(&seen);
        DiscardPolicy::Report(Arc::new(move |model, columns| {
            seen.lock()
                .unwrap()
                .push((model.to_owned(), columns.to_vec()));
        }))
    };
    let mut member = Record::new(Member::default());
    with_discard_policy(handler, async {
        member
            .fill(attrs! { "name" => "Anu", "is_admin" => true })
            .unwrap();
    })
    .await;
    assert_eq!(member.name, "Anu");
    assert_eq!(
        *seen.lock().unwrap(),
        [("Member".to_owned(), vec!["is_admin".to_owned()])]
    );
}

#[tokio::test]
async fn the_strict_mode_for_discarded_attributes_decides_without_a_policy() {
    use navree_orm::strict::{self, Strictness, Violation};

    // Strict: refused before anything changes.
    let mut member = Record::new(Member::default());
    let error = strict::scope(Strictness::strict(), async {
        member.fill(attrs! { "name" => "Anu", "is_admin" => true })
    })
    .await
    .unwrap_err();
    assert!(
        matches!(&error, OrmError::MassAssignment { totally_guarded: false, columns, .. } if columns == &["is_admin"]),
        "{error:?}"
    );
    assert_eq!(member.name, "", "nothing was set");

    // Lenient: left out.
    strict::scope(Strictness::lenient(), async {
        member
            .fill(attrs! { "name" => "Anu", "is_admin" => true })
            .unwrap();
    })
    .await;
    assert_eq!(member.name, "Anu");
    assert!(!member.is_admin);

    // A handler tolerates them, and sees the violation.
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let logging = {
        let seen = Arc::clone(&seen);
        Strictness::strict().on_discarded_attributes(move |violation| {
            seen.lock().unwrap().push(violation.to_string());
            Ok(())
        })
    };
    strict::scope(logging, async {
        member
            .fill(attrs! { "role" => "admin", "nope" => 1 })
            .unwrap();
    })
    .await;
    assert_eq!(member.role, Role::Admin);
    assert_eq!(
        *seen.lock().unwrap(),
        ["columns nope of Member are not fillable and were not filled"]
    );
    assert!(matches!(
        Violation::DiscardedAttributes {
            model: "Member",
            columns: vec!["nope".into()],
        }
        .into_error(),
        OrmError::MassAssignment {
            totally_guarded: false,
            ..
        }
    ));

    // The task's discard policy outranks the task's strict modes.
    let mut other = Record::new(Member::default());
    strict::scope(Strictness::strict(), async {
        with_discard_policy(DiscardPolicy::Ignore, async {
            other
                .fill(attrs! { "name" => "Gauri", "is_admin" => true })
                .unwrap();
        })
        .await;
    })
    .await;
    assert_eq!(other.name, "Gauri");
}

#[tokio::test]
async fn unguarded_scopes_and_models_fill_every_real_column() {
    let mut member = Record::new(Member::default());
    unguarded(async {
        assert!(navree_orm::attributes::is_unguarded());
        member.fill(attrs! { "is_admin" => true }).unwrap();
    })
    .await;
    assert!(member.is_admin);
    let mut label = Record::new(Label::default());
    unguarded(async { label.fill(attrs! { "name" => "x" }) })
        .await
        .unwrap();
    assert_eq!(label.name, "x");

    let mut note = Record::new(Note::default());
    note.fill(attrs! { "body" => "hello", "pinned" => 1 })
        .unwrap();
    assert_eq!((note.body.as_str(), note.pinned), ("hello", true));
}

#[test]
fn fill_only_is_an_allow_list_for_one_call() {
    let mut post = Record::new(Post::default());
    post.fill_only(
        attrs! { "title" => "T", "body" => "B", "author_id" => 4 },
        &["title", "author_id"],
    )
    .unwrap();
    assert_eq!(
        (post.title.as_str(), post.body.as_str(), post.author_id),
        ("T", "", 4)
    );
}

#[test]
fn guards_are_declared_and_queried_statically() {
    assert_eq!(
        Member::fillable(),
        [
            "name",
            "email",
            "role",
            "age",
            "settings",
            "joined_at",
            "profile->bio"
        ]
    );
    assert_eq!(Member::guarded(), None);
    assert!(Member::is_fillable("name"));
    assert!(Member::is_fillable("profile->bio"));
    assert!(!Member::is_fillable("profile->links"));
    assert!(
        Member::is_fillable("settings->theme"),
        "a path into a fillable JSON column"
    );
    assert!(!Member::is_fillable("is_admin"));
    assert!(!Member::is_fillable("nope"));
    assert!(Member::is_guarded("is_admin"));
    assert!(!Member::totally_guarded());

    assert_eq!(Post::guarded(), Some(&["id", "author_id"][..]));
    assert!(Post::is_fillable("title"));
    assert!(!Post::is_fillable("author_id"));
    assert!(Post::is_guarded("nope"), "unknown columns are guarded");
    assert!(Label::totally_guarded());
    assert!(!Note::totally_guarded());
    assert!(Note::is_fillable("body"));
    assert!(!Note::is_guarded("pinned"));

    assert_eq!(Draft::fillable(), ["title", "body"]);
    assert_eq!(Draft::guarded(), Some(&["id"][..]));
    assert!(Draft::is_fillable("body"));
    assert!(
        !Draft::is_fillable("author_id"),
        "fillable lists win over guarded lists"
    );
}

#[test]
fn json_paths_edit_json_columns_by_value() {
    let mut member = Record::new(Member::default());
    member
        .set_json_path("settings", "theme->accent", "green")
        .unwrap();
    member
        .set_json_path(Member::SETTINGS, "langs", ["en", "pa"])
        .unwrap();
    assert_eq!(
        member.settings,
        json!({"theme": {"accent": "green"}, "langs": ["en", "pa"]})
    );
    // A typed JSON field must still fit its type: an absent profile gains
    // `links` but no `bio`.
    let error = member
        .set_json_path(Member::PROFILE, "links", ["https://a.example"])
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot cast column `profile` of Member: invalid JSON: missing field `bio`"
    );
    assert_eq!(member.profile, None, "nothing changed");
    member.profile = Some(Profile::default());
    member
        .set_json_path(Member::PROFILE, "links", ["https://a.example"])
        .unwrap();
    member
        .set_json_path(Member::PROFILE, "links->1", "https://b.example")
        .unwrap();
    assert_eq!(
        member.profile,
        Some(Profile {
            bio: String::new(),
            links: vec!["https://a.example".into(), "https://b.example".into()],
        })
    );
    let error = member
        .set_json_path(Member::PROFILE, "links->5", "x")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: cannot set `profile->links->5` of Member: index 5 is past the end of an array of 2"
    );
    let error = member.set_json_path(Member::PROFILE, "bio", 7).unwrap_err();
    assert!(matches!(error, OrmError::Cast { .. }), "{error:?}");
    let error = member.set_json_path(Member::NAME, "x", 1).unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: cannot fill `name->x`: `name` of Member is not a JSON column"
    );
}

#[test]
fn mutators_also_run_through_generated_setters_and_set_attribute() {
    let mut member = Member::default();
    member.set_email(" Gauri@Example.com");
    assert_eq!(member.email, "gauri@example.com");
    let mut record = Record::new(Member::default());
    record.set_attribute("email", "LINUS@example.com").unwrap();
    assert_eq!(record.email, "laksh@example.com");
    record.set_attribute("is_admin", true).unwrap();
    assert!(record.is_admin, "set_attribute is not guarded");
    assert!(Member::meta().column("email").unwrap().has_mutator());
}

#[tokio::test]
async fn mutators_can_write_several_columns() {
    let t = sqlite(SCHEMA).await;
    let person = Person::create_from(&t.db, attrs! { "full_name" => "  Anu   King Lakhotia " })
        .await
        .unwrap();
    assert_eq!(person.full_name, "Anu Kaur Lakhotia");
    let mut stored = Person::find_or_fail(&t.db, person.id).await.unwrap();
    assert_eq!(
        (stored.first_name.as_str(), stored.last_name.as_str()),
        ("Anu", "King Lakhotia"),
        "set by the mutator, though not fillable"
    );
    stored.set_full_name("Gauri Hundal");
    assert!(stored.is_dirty_on(["first_name"]) && stored.is_dirty_on(["last_name"]));
    stored.fill(attrs! { "first_name" => "x" }).unwrap();
    assert_eq!(stored.first_name, "Gauri", "not fillable: left out");
}

#[test]
fn request_bodies_deserialize_into_attributes() {
    let body: Attributes =
        serde_json::from_str(r#"{"title": "From JSON", "published": false}"#).unwrap();
    let mut post = Record::new(Post::default());
    post.fill(body).unwrap();
    assert_eq!(post.title, "From JSON");
    #[derive(Serialize)]
    struct NewPost<'a> {
        title: &'a str,
        body: &'a str,
    }
    let typed = Attributes::from_serialize(&NewPost {
        title: "Typed",
        body: "Body",
    })
    .unwrap();
    post.fill(typed).unwrap();
    assert_eq!((post.title.as_str(), post.body.as_str()), ("Typed", "Body"));
}

#[tokio::test]
async fn records_are_created_and_updated_from_attributes() {
    let t = sqlite(SCHEMA).await;
    let member = Member::create_from(
        &t.db,
        attrs! { "name" => "Anu", "email" => "ANU@X.IO", "joined_at" => "2026-01-02T03:04:05Z", "is_admin" => true },
    )
    .await
    .unwrap();
    assert!(member.exists());
    let stored = Member::find_or_fail(&t.db, member.id).await.unwrap();
    assert_eq!(
        (stored.name.as_str(), stored.email.as_str(), stored.is_admin),
        ("Anu", "anu@x.io", false)
    );

    let forced = Member::force_create(&t.db, attrs! { "name" => "Root", "is_admin" => true })
        .await
        .unwrap();
    assert!(
        Member::find_or_fail(&t.db, forced.id)
            .await
            .unwrap()
            .is_admin
    );

    let label = Label::query()
        .force_create(&t.db, attrs! { "name" => "vip" })
        .await
        .unwrap();
    assert_eq!(label.name, "vip");
    let error = Label::create_from(&t.db, attrs! { "name" => "no" })
        .await
        .unwrap_err();
    assert!(
        matches!(error, OrmError::MassAssignment { .. }),
        "{error:?}"
    );

    let mut stored = stored;
    assert!(
        stored
            .update_attributes(&t.db, attrs! { "name" => "Anu L." })
            .await
            .unwrap()
    );
    assert_eq!(
        Member::find_or_fail(&t.db, member.id).await.unwrap().name,
        "Anu L."
    );
    let mut unsaved = Record::new(Member::default());
    assert!(
        !unsaved
            .update_attributes(&t.db, attrs! { "name" => "x" })
            .await
            .unwrap()
    );
    assert_eq!(
        unsaved.name, "",
        "a record that is not stored is left alone"
    );
}
