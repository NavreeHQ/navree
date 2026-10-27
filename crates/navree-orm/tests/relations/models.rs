//! The models of the relation tests (a small CRM), and their SQLite tables.

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use navree_orm::ir::AggregateFunction;
use navree_orm::prelude::*;
use navree_orm::relations::{
    BelongsTo, BelongsToMany, HasMany, HasManyThrough, HasOne, HasOneThrough, MorphMany, MorphOne,
    MorphTo, MorphToMany,
};
use serde::Serialize;
use uuid::Uuid;

/// The tables, as one script.
pub const SCHEMA: &str = r#"
CREATE TABLE teams (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL
);
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id INTEGER NULL,
    name TEXT NOT NULL,
    deleted_at TEXT NULL
);
CREATE TABLE companies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id INTEGER NULL,
    owner_id INTEGER NULL,
    name TEXT NOT NULL,
    code TEXT NULL,
    active BOOLEAN NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE contacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    company_id INTEGER NULL,
    company_code TEXT NULL,
    name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE
);
CREATE TABLE deals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    company_id INTEGER NULL,
    owner_id INTEGER NULL,
    title TEXT NOT NULL,
    amount_cents INTEGER NOT NULL DEFAULT 0,
    stage TEXT NOT NULL DEFAULT 'lead',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT NULL
);
CREATE TABLE activities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    deal_id INTEGER NULL,
    kind TEXT NOT NULL,
    minutes INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    updated_at TEXT NOT NULL
);
CREATE TABLE deal_tag (
    deal_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    role TEXT NULL,
    created_at TEXT NULL,
    updated_at TEXT NULL,
    PRIMARY KEY (deal_id, tag_id)
);
CREATE TABLE deal_watchers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    deal_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    level TEXT NOT NULL DEFAULT 'all',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    notable_type TEXT NULL,
    notable_id INTEGER NULL,
    body TEXT NOT NULL
);
CREATE TABLE images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    imageable_type TEXT NULL,
    imageable_id INTEGER NULL,
    url TEXT NOT NULL
);
CREATE TABLE taggables (
    tag_id INTEGER NOT NULL,
    taggable_type TEXT NOT NULL,
    taggable_id INTEGER NOT NULL,
    PRIMARY KEY (tag_id, taggable_type, taggable_id)
);
CREATE TABLE tickets (
    id BLOB PRIMARY KEY,
    title TEXT NOT NULL
);
CREATE TABLE attachments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    attachable_type TEXT NULL,
    attachable_id BLOB NULL,
    file TEXT NOT NULL
);
CREATE TABLE events (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL
);
CREATE TABLE marks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    markable_type TEXT NULL,
    markable_id TEXT NULL,
    label TEXT NOT NULL
);
"#;

/// A sales team.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Team {
    pub id: i64,
    pub name: String,
}

#[relations]
impl Team {
    /// The team's reps.
    pub fn users(&self) -> HasMany<Team, User> {
        self.has_many(User::TEAM_ID)
    }

    /// The deals the team's reps own (reps soft-deleted are left out).
    pub fn deals(&self) -> HasManyThrough<Team, Deal> {
        self.has_many_through::<Deal, User, _, _>(User::TEAM_ID, Deal::OWNER_ID)
    }

    /// The deals of every rep, soft-deleted ones included.
    pub fn all_deals(&self) -> HasManyThrough<Team, Deal> {
        self.has_many_through::<Deal, User, _, _>(User::TEAM_ID, Deal::OWNER_ID)
            .with_trashed_parents()
    }

    /// The team's largest deal.
    pub fn biggest_deal(&self) -> HasOneThrough<Team, Deal> {
        self.has_one_through::<Deal, User, _, _>(User::TEAM_ID, Deal::OWNER_ID)
            .latest_of_many_by(Deal::AMOUNT_CENTS)
    }

    /// A deal of the team, any (`has_one_through`).
    pub fn a_deal(&self) -> HasOneThrough<Team, Deal> {
        self.has_one_through::<Deal, User, _, _>(User::TEAM_ID, Deal::OWNER_ID)
    }

    /// The deals of the team's reps, composed from two relations.
    pub fn rep_deals(&self) -> HasManyThrough<Team, Deal> {
        self.through(Team::users).has(User::deals)
    }
}

/// A sales rep; soft-deleting.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(soft_deletes)]
pub struct User {
    pub id: i64,
    pub team_id: Option<i64>,
    pub name: String,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[relations]
impl User {
    /// The rep's team.
    pub fn team(&self) -> BelongsTo<User, Team> {
        self.belongs_to(User::TEAM_ID)
    }

    /// The deals the rep owns.
    pub fn deals(&self) -> HasMany<User, Deal> {
        self.has_many(Deal::OWNER_ID)
    }

    /// The accounts the rep owns (a model with default loads and counts).
    pub fn accounts(&self) -> HasMany<User, Account> {
        self.has_many(Account::OWNER_ID)
    }

    /// The deals the rep watches, through the typed pivot model.
    pub fn watched(&self) -> BelongsToMany<User, Deal, DealWatcher> {
        self.belongs_to_many("deal_watchers")
            .using::<DealWatcher>()
            .with_pivot(["level"])
    }
}

/// A customer; has subsidiaries (a self-relation) and polymorphic notes.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps, morph_name = "company")]
pub struct Company {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub owner_id: Option<i64>,
    pub name: String,
    /// The company's code in the accounting system.
    pub code: Option<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Company {
    /// An active company named `name`.
    pub fn named(name: &str) -> Company {
        Company {
            name: name.to_owned(),
            active: true,
            ..Company::default()
        }
    }
}

#[relations]
impl Company {
    /// The parent company.
    pub fn parent(&self) -> BelongsTo<Company, Company> {
        self.belongs_to(Company::PARENT_ID)
    }

    /// The companies this one owns.
    pub fn subsidiaries(&self) -> HasMany<Company, Company> {
        self.has_many(Company::PARENT_ID)
    }

    /// The rep owning the account.
    pub fn owner(&self) -> BelongsTo<Company, User> {
        self.belongs_to(Company::OWNER_ID)
    }

    /// The people at the company.
    pub fn contacts(&self) -> HasMany<Company, Contact> {
        self.has_many(Contact::COMPANY_ID)
    }

    /// One person at the company (`has_one`).
    pub fn contact(&self) -> HasOne<Company, Contact> {
        self.has_one(Contact::COMPANY_ID)
    }

    /// The people filed under the company's accounting code: the parent's
    /// column is not its key.
    pub fn coded_contacts(&self) -> HasMany<Company, Contact> {
        self.has_many(Contact::COMPANY_CODE)
            .local_key(Company::CODE)
    }

    /// The company's deals; each loads with its company already set.
    pub fn deals(&self) -> HasMany<Company, Deal> {
        self.has_many(Deal::COMPANY_ID).inverse("company")
    }

    /// Notes about the company.
    pub fn notes(&self) -> MorphMany<Company, Note> {
        self.morph_many("notable")
    }

    /// The company's logo.
    pub fn logo(&self) -> MorphOne<Company, Image> {
        self.morph_one("imageable")
    }

    /// Labels, through the polymorphic pivot `taggables`.
    pub fn labels(&self) -> MorphToMany<Company, Tag> {
        self.morph_to_many("taggable", "taggables")
    }
}

/// Hides the contacts at `hidden.test`: a global scope.
#[derive(Clone, Copy, Debug, Default)]
pub struct Visible;

impl GlobalScope<Contact> for Visible {
    fn apply(&self, query: Builder<Contact>) -> Builder<Contact> {
        query.where_(Contact::EMAIL.not_like("%@hidden.test"))
    }
}

/// A person at a company (eager loads may leave out the email).
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(global_scopes(Visible))]
pub struct Contact {
    pub id: i64,
    pub company_id: Option<i64>,
    /// The accounting code of the company the contact is filed under.
    #[model(default)]
    pub company_code: Option<String>,
    pub name: String,
    #[model(default)]
    pub email: String,
}

#[relations]
impl Contact {
    /// The contact's company.
    pub fn company(&self) -> BelongsTo<Contact, Company> {
        self.belongs_to(Contact::COMPANY_ID)
    }

    /// The company the contact is filed under, by its accounting code.
    pub fn coded_company(&self) -> BelongsTo<Contact, Company> {
        self.belongs_to(Contact::COMPANY_CODE)
            .owner_key(Company::CODE)
    }
}

/// A sales opportunity; soft-deleting.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps, soft_deletes)]
pub struct Deal {
    pub id: i64,
    pub company_id: Option<i64>,
    pub owner_id: Option<i64>,
    pub title: String,
    pub amount_cents: i64,
    pub stage: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Deal {
    /// A lead titled `title`, worth `amount_cents`.
    pub fn new(title: &str, amount_cents: i64) -> Deal {
        Deal {
            title: title.to_owned(),
            amount_cents,
            stage: "lead".to_owned(),
            ..Deal::default()
        }
    }
}

#[relations]
impl Deal {
    /// The customer.
    pub fn company(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
    }

    /// The customer, or a placeholder naming the deal.
    pub fn company_or_placeholder(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
            .with_default_fn(|deal: &Deal| {
                Company::named(&format!("No company for {}", deal.title))
            })
    }

    /// The rep owning the deal.
    pub fn owner(&self) -> BelongsTo<Deal, User> {
        self.belongs_to(Deal::OWNER_ID)
    }

    /// Everything that happened on the deal.
    pub fn activities(&self) -> HasMany<Deal, Activity> {
        self.has_many(Activity::DEAL_ID)
    }

    /// The deal's calls.
    pub fn calls(&self) -> HasMany<Deal, Activity> {
        self.has_many(Activity::DEAL_ID)
            .where_(Activity::KIND.eq("call"))
    }

    /// The newest activity (largest key).
    pub fn latest_activity(&self) -> HasOne<Deal, Activity> {
        self.has_many(Activity::DEAL_ID).latest_of_many()
    }

    /// The first activity (smallest key).
    pub fn first_activity(&self) -> HasOne<Deal, Activity> {
        self.has_many(Activity::DEAL_ID).oldest_of_many()
    }

    /// The longest call.
    pub fn longest_call(&self) -> HasOne<Deal, Activity> {
        self.has_many(Activity::DEAL_ID)
            .of_many([(Activity::MINUTES, AggregateFunction::Max)], |q| {
                q.where_(Activity::KIND.eq("call"))
            })
    }

    /// The first activity, or a placeholder.
    pub fn first_activity_or_default(&self) -> HasOne<Deal, Activity> {
        self.has_many(Activity::DEAL_ID)
            .oldest_of_many()
            .with_default_value(Activity {
                kind: "none yet".to_owned(),
                ..Activity::default()
            })
    }

    /// Tags with a role, through `deal_tag` (timestamped).
    pub fn tags(&self) -> BelongsToMany<Deal, Tag> {
        self.belongs_to_many("deal_tag")
            .with_pivot(["role"])
            .with_timestamps()
    }

    /// Tags whose pivot holds the role `owner`; attaching through it sets
    /// the role.
    pub fn owner_tags(&self) -> BelongsToMany<Deal, Tag> {
        self.belongs_to_many("deal_tag")
            .with_pivot(["role"])
            .with_pivot_value("role", "owner")
    }

    /// Tags carrying their pivot under `assignment`.
    pub fn assigned_tags(&self) -> BelongsToMany<Deal, Tag> {
        self.belongs_to_many("deal_tag")
            .with_pivot(["role"])
            .as_("assignment")
            .chaperone_pivot()
    }

    /// Tags through the typed pivot model without an id.
    pub fn typed_tags(&self) -> BelongsToMany<Deal, Tag, DealTag> {
        self.belongs_to_many("deal_tag").using::<DealTag>()
    }

    /// The reps watching the deal, through the typed pivot model.
    pub fn watchers(&self) -> BelongsToMany<Deal, User, DealWatcher> {
        self.belongs_to_many("deal_watchers")
            .using::<DealWatcher>()
            .with_pivot(["level"])
            .with_timestamps()
    }

    /// Notes about the deal.
    pub fn notes(&self) -> MorphMany<Deal, Note> {
        self.morph_many("notable")
    }

    /// The deal's image.
    pub fn image(&self) -> MorphOne<Deal, Image> {
        self.morph_one("imageable")
    }

    /// Labels, through the polymorphic pivot `taggables`.
    pub fn labels(&self) -> MorphToMany<Deal, Tag> {
        self.morph_to_many("taggable", "taggables")
    }

    /// The people at the deal's company.
    pub fn contacts(&self) -> HasManyThrough<Deal, Contact> {
        self.through(Deal::company).has(Company::contacts)
    }

    /// The deal's rows in the reporting database (another connection).
    pub fn report_rows(&self) -> HasMany<Deal, ReportRow> {
        self.has_many(ReportRow::DEAL_ID)
    }
}

/// A row of the reporting database: a model on another connection.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "report_rows", connection = "analytics")]
pub struct ReportRow {
    pub id: i64,
    pub deal_id: Option<i64>,
    pub score: i64,
}

/// Something that happened on a deal; touches the deal when it changes.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps, touches = ["deal"])]
pub struct Activity {
    pub id: i64,
    pub deal_id: Option<i64>,
    pub kind: String,
    pub minutes: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Activity {
    /// An activity of `kind` lasting `minutes`.
    pub fn new(kind: &str, minutes: i64) -> Activity {
        Activity {
            kind: kind.to_owned(),
            minutes,
            ..Activity::default()
        }
    }
}

#[relations]
impl Activity {
    /// The deal.
    pub fn deal(&self) -> BelongsTo<Activity, Deal> {
        self.belongs_to(Activity::DEAL_ID)
    }
}

/// A label on deals; touches the deals it is attached to.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps(created_at = false), touches = ["deals"])]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub updated_at: DateTime<Utc>,
}

impl Tag {
    /// A tag named `name`.
    pub fn named(name: &str) -> Tag {
        Tag {
            name: name.to_owned(),
            ..Tag::default()
        }
    }
}

#[relations]
impl Tag {
    /// The deals with the tag.
    pub fn deals(&self) -> BelongsToMany<Tag, Deal> {
        self.belongs_to_many("deal_tag").with_pivot(["role"])
    }

    /// The deals labelled with the tag (polymorphic pivot).
    pub fn labeled_deals(&self) -> MorphToMany<Tag, Deal> {
        self.morphed_by_many("taggable", "taggables")
    }

    /// The companies labelled with the tag (polymorphic pivot).
    pub fn labeled_companies(&self) -> MorphToMany<Tag, Company> {
        self.morphed_by_many("taggable", "taggables")
    }
}

/// A row of `deal_tag` as a typed pivot model: no id of its own, so it is
/// identified by both pivot columns.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "deal_tag",
    pivot(foreign_key = "deal_id", related_key = "tag_id")
)]
pub struct DealTag {
    pub deal_id: i64,
    pub tag_id: i64,
    pub role: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// A rep watching a deal: a typed pivot model with an id and timestamps.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "deal_watchers",
    timestamps,
    pivot(foreign_key = "deal_id", related_key = "user_id")
)]
pub struct DealWatcher {
    pub id: i64,
    pub deal_id: i64,
    pub user_id: i64,
    pub level: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A note about a deal or a company: its `notable` is resolved through the
/// morph map.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Note {
    pub id: i64,
    pub notable_type: Option<String>,
    pub notable_id: Option<i64>,
    pub body: String,
}

impl Note {
    /// A note saying `body`.
    pub fn new(body: &str) -> Note {
        Note {
            body: body.to_owned(),
            ..Note::default()
        }
    }
}

#[relations]
impl Note {
    /// What the note is about.
    pub fn notable(&self) -> MorphTo<Note> {
        self.morph_to("notable")
    }
}

/// An image of a deal or a company: its `imageable` lists its models.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Image {
    pub id: i64,
    pub imageable_type: Option<String>,
    pub imageable_id: Option<i64>,
    pub url: String,
}

#[relations]
impl Image {
    /// What the image shows.
    pub fn imageable(&self) -> MorphTo<Image> {
        self.morph_to("imageable").targets::<(Deal, Company)>()
    }

    /// What the image shows, soft-deleted deals included.
    pub fn imageable_with_trashed(&self) -> MorphTo<Image> {
        self.morph_to("imageable")
            .targets::<(Deal, Company)>()
            .with_trashed()
    }

    /// What the image shows, only soft-deleted deals.
    pub fn imageable_only_trashed(&self) -> MorphTo<Image> {
        self.morph_to("imageable")
            .targets::<(Deal, Company)>()
            .only_trashed()
    }
}

/// A support ticket, keyed by a UUID.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Ticket {
    pub id: Uuid,
    pub title: String,
}

#[relations]
impl Ticket {
    /// The ticket's attachments.
    pub fn attachments(&self) -> MorphMany<Ticket, Attachment> {
        self.morph_many("attachable")
    }
}

/// A file attached to a ticket: a morph whose ids are UUIDs.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Attachment {
    pub id: i64,
    pub attachable_type: Option<String>,
    pub attachable_id: Option<Uuid>,
    pub file: String,
}

#[relations]
impl Attachment {
    /// What the file is attached to.
    pub fn attachable(&self) -> MorphTo<Attachment> {
        self.morph_to("attachable").target::<Ticket>()
    }
}

/// Something that happened, keyed by a ULID.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Event {
    pub id: Ulid,
    pub name: String,
}

#[relations]
impl Event {
    /// The event's marks.
    pub fn marks(&self) -> MorphMany<Event, Mark> {
        self.morph_many("markable")
    }
}

/// A mark on an event: a morph whose ids are ULIDs.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Mark {
    pub id: i64,
    pub markable_type: Option<String>,
    pub markable_id: Option<Ulid>,
    pub label: String,
}

#[relations]
impl Mark {
    /// What the mark is on.
    pub fn markable(&self) -> MorphTo<Mark> {
        self.morph_to("markable").target::<Event>()
    }
}

/// A company that eager loads its owner and counts its contacts on every
/// query (model defaults).
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "companies", timestamps, with = ["owner.team"], with_count = ["contacts"])]
pub struct Account {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub owner_id: Option<i64>,
    pub name: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[relations]
impl Account {
    /// The rep owning the account.
    pub fn owner(&self) -> BelongsTo<Account, User> {
        self.belongs_to(Account::OWNER_ID)
    }

    /// The people at the account (by `company_id`).
    pub fn contacts(&self) -> HasMany<Account, Contact> {
        self.has_many(Contact::COMPANY_ID)
    }
}
