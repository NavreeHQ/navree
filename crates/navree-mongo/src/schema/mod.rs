//! The schema builder on MongoDB: [`MongoSchema`] and [`MongoBlueprint`].
//!
//! The same blueprints as navree-schema's (every column method, indexes,
//! foreign keys, `hypertable`) describe collections:
//!
//! | Blueprint | MongoDB |
//! |---|---|
//! | `create` | `create` with a `$jsonSchema` validator: one property per column, typed (`string`, `int`/`long`, `decimal`, `date`, `binData`, ...), `null` allowed for nullable columns, `maxLength`, `enum`, `minimum: 0` for unsigned |
//! | the key column `id` | `_id` (models store their `id` key there) |
//! | `unique`, `index`, `primary` on other columns | unique and regular indexes; a unique index over nullable fields covers the documents that set them (NULLs do not collide, as in SQL) |
//! | `fulltext`, `spatial_index` | a text index (with its language), a `2dsphere` index |
//! | `foreign_id(..).constrained()` | an index on the field, and a notice (MongoDB enforces no references) |
//! | `hypertable(time)` | a time-series collection: `timeField`, `metaField` from the space partition, `granularity` from the chunk interval, `expireAfterSeconds` from the retention (MongoDB takes no validator on it: a notice says so) |
//! | `table` (alter) | the validator updated (`collMod`), dropped and renamed fields removed or renamed in every document (`$unset`, `$rename`) and in the indexes, indexes created, dropped and renamed |
//! | `rename`, `drop`, `drop_if_exists`, `has_table`, `has_column` | `renameCollection`, `drop`, `listCollections`, the validator or a document with the field |
//! | a described continuous aggregate | a view: `$group` by the time bucket and the groups |
//!
//! [`MongoBlueprint`] adds what only MongoDB has: ObjectId fields, TTL
//! indexes, the validation level and action, capped collections.

mod compile;

use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

use bson::{Bson, Document, doc};
use navree_schema::{
    Blueprint, ColumnBuilder, Command, ContinuousAggregate, Hypertable, IndexKind, Interval,
    SchemaOptions,
};

pub(crate) use compile::SchemaCommand;

use crate::connection::{MongoDb, MongoExecutor};
use crate::driver::MongoDriver;
use crate::error::{MongoError, MongoResult};
use crate::uow::MongoUow;

/// A blueprint for a collection: navree-schema's [`Blueprint`] (every
/// column, index and helper, through `Deref`) plus MongoDB's own settings.
///
/// ```
/// use navree_mongo::MongoBlueprint;
///
/// let mut t = MongoBlueprint::new("sessions");
/// t.object_id_primary();
/// t.object_id("user_id").index();
/// t.timestamp_tz("expires_at");
/// t.expire_after("expires_at", "0 seconds"); // removed once `expires_at` passes
/// assert_eq!(t.columns().len(), 3);
/// ```
pub struct MongoBlueprint {
    blueprint: Blueprint,
    extras: compile::Extras,
}

impl MongoBlueprint {
    /// A blueprint creating `collection`.
    pub fn new(collection: impl Into<String>) -> MongoBlueprint {
        MongoBlueprint::with(Blueprint::new(collection))
    }

    fn with(blueprint: Blueprint) -> MongoBlueprint {
        MongoBlueprint {
            blueprint,
            extras: compile::Extras::default(),
        }
    }

    /// The navree-schema blueprint.
    pub fn blueprint(&self) -> &Blueprint {
        &self.blueprint
    }

    /// An ObjectId field.
    pub fn object_id(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.blueprint.raw_column(name, "objectId")
    }

    /// An ObjectId key: the column `id`, stored as `_id` (models with an
    /// `id: ObjectId` key make it before they insert).
    pub fn object_id_primary(&mut self) -> ColumnBuilder<'_> {
        self.blueprint.raw_column("id", "objectId").primary()
    }

    /// A TTL index: MongoDB deletes a document once `field` (a date) is
    /// `interval` in the past (`"0 seconds"`: when the date passes,
    /// `"30 days"`: a month after). Months and years are not fixed lengths:
    /// write days.
    pub fn expire_after(&mut self, field: impl Into<String>, interval: &str) -> &mut Self {
        let field = field.into();
        match Interval::parse(interval) {
            Ok(parsed) => match parsed.as_seconds() {
                Some(seconds) => self.extras.expire.push((field, seconds)),
                None => self.extras.expire_errors.push(format!(
                    "the expiry of `{field}` is `{interval}`: MongoDB expires after a number of seconds; write it in days"
                )),
            },
            Err(error) => self.extras.expire_errors.push(error.to_string()),
        }
        self
    }

    /// How strictly the validator applies: `strict` (every insert and
    /// update, the default) or `moderate` (not to documents that already
    /// break it).
    pub fn validation_level(&mut self, level: impl Into<String>) -> &mut Self {
        self.extras.validation_level = Some(level.into());
        self
    }

    /// What a document breaking the validator gets: `error` (refused, the
    /// default) or `warn` (logged).
    pub fn validation_action(&mut self, action: impl Into<String>) -> &mut Self {
        self.extras.validation_action = Some(action.into());
        self
    }

    /// No validator: the columns document the collection but MongoDB checks
    /// nothing.
    pub fn without_validator(&mut self) -> &mut Self {
        self.extras.without_validator = true;
        self
    }

    /// A capped collection of at most `size` bytes (and `max` documents),
    /// keeping insertion order and dropping the oldest documents.
    pub fn capped(&mut self, size: u64, max: Option<u64>) -> &mut Self {
        self.extras.capped = Some((size, max));
        self
    }
}

impl Deref for MongoBlueprint {
    type Target = Blueprint;

    fn deref(&self) -> &Blueprint {
        &self.blueprint
    }
}

impl DerefMut for MongoBlueprint {
    fn deref_mut(&mut self) -> &mut Blueprint {
        &mut self.blueprint
    }
}

/// The schema builder on a MongoDB database: collections, validators,
/// indexes, time-series collections and views, from blueprints (see the
/// [module docs](self)).
///
/// It records every command it runs ([`statements`](Self::statements)) and
/// what MongoDB does differently from SQL ([`notices`](Self::notices));
/// [`pretending`](Self::pretending), it reads the database but records its
/// changes instead of running them.
///
/// ```no_run
/// # use navree_mongo::MongoDb;
/// # async fn demo(mongo: MongoDb) -> navree_mongo::MongoResult {
/// let schema = mongo.schema();
/// schema
///     .create("deals", |t| {
///         t.object_id_primary();
///         t.object_id("company_id").index();
///         t.string("title");
///         t.decimal("amount", 12, 2);
///         t.timestamps_tz();
///         t.soft_deletes_tz();
///     })
///     .await?;
/// schema
///     .create_hypertable("events", "occurred_at", |t| {
///         t.object_id_primary();
///         t.timestamp_tz("occurred_at");
///         t.hypertable("occurred_at").retain_for("90 days");
///     })
///     .await?;
/// # Ok(()) }
/// ```
pub struct MongoSchema {
    db: MongoDb,
    /// The driver commands run on: in a migration's transaction, its
    /// session.
    driver: Arc<MongoDriver>,
    pretend: bool,
    options: SchemaOptions,
    statements: Mutex<Vec<String>>,
    notices: Mutex<Vec<String>>,
    uow: Option<MongoUow>,
}

impl MongoSchema {
    pub(crate) fn new(db: MongoDb) -> MongoSchema {
        MongoSchema {
            driver: db.driver().clone(),
            db,
            pretend: false,
            options: SchemaOptions::default(),
            statements: Mutex::new(Vec::new()),
            notices: Mutex::new(Vec::new()),
            uow: None,
        }
    }

    /// The same schema recording its changes instead of running them (it
    /// still reads the database: `has_table`, the current validator).
    pub fn pretending(mut self) -> MongoSchema {
        self.pretend = true;
        self
    }

    /// The schema running in `uow`'s session (a migration in a
    /// transaction).
    pub(crate) fn in_uow(mut self, uow: MongoUow) -> MongoResult<MongoSchema> {
        self.driver = uow.mongo_driver()?;
        self.uow = Some(uow);
        Ok(self)
    }

    /// The options blueprints use for defaults (string lengths, time
    /// precision).
    pub fn with_options(mut self, options: SchemaOptions) -> MongoSchema {
        self.options = options;
        self
    }

    /// Whether the schema only records what it would change.
    pub fn is_pretend(&self) -> bool {
        self.pretend
    }

    /// The connection, for data migrations: models run on it
    /// (`Deal::query().update(schema.db(), ..)`). Pretending does not stop
    /// writes made through it: check [`is_pretend`](Self::is_pretend).
    pub fn db(&self) -> &MongoDb {
        &self.db
    }

    /// The unit of work of a migration that runs in a transaction: writes
    /// through it commit with the migration's record.
    pub fn uow(&self) -> Option<&MongoUow> {
        self.uow.as_ref()
    }

    /// The commands run (or, pretending, that would run), in order.
    pub fn statements(&self) -> Vec<String> {
        lock(&self.statements).clone()
    }

    /// What MongoDB did differently from an SQL database.
    pub fn notices(&self) -> Vec<String> {
        lock(&self.notices).clone()
    }

    fn notice(&self, notice: String) {
        tracing::info!(target: "navree_mongo::schema", "{notice}");
        lock(&self.notices).push(notice);
    }

    /// Runs (or records) `command`.
    async fn run(&self, command: SchemaCommand) -> MongoResult<Option<Document>> {
        lock(&self.statements).push(command.to_string());
        if self.pretend {
            return Ok(None);
        }
        if command.admin {
            let admin = self.db.client().database("admin");
            return Ok(Some(admin.run_command(command.document).await?));
        }
        Ok(Some(self.driver.run_command(command.document).await?))
    }

    /// Runs a read-only command on the database (not recorded).
    async fn read(&self, command: Document) -> MongoResult<Document> {
        Ok(self.db.database().run_command(command).await?)
    }

    /// Runs `command` on the database (recorded; pretending, only
    /// recorded), for what the builder does not cover.
    pub async fn command(&self, command: Document) -> MongoResult<Option<Document>> {
        self.run(SchemaCommand::database(command)).await
    }

    /// Creates `collection` as `f` describes it.
    pub async fn create(
        &self,
        collection: &str,
        f: impl FnOnce(&mut MongoBlueprint),
    ) -> MongoResult {
        let mut blueprint =
            MongoBlueprint::with(Blueprint::with_options(collection, self.options.clone()));
        f(&mut blueprint);
        self.create_from(blueprint).await
    }

    /// Creates a time-series collection keyed by `time_field` (MongoDB's
    /// engine for time-series data, as a hypertable is TimescaleDB's).
    pub async fn create_hypertable(
        &self,
        collection: &str,
        time_field: &str,
        f: impl FnOnce(&mut MongoBlueprint),
    ) -> MongoResult {
        let mut blueprint =
            MongoBlueprint::with(Blueprint::with_options(collection, self.options.clone()));
        blueprint.hypertable(time_field);
        f(&mut blueprint);
        self.create_from(blueprint).await
    }

    async fn create_from(&self, blueprint: MongoBlueprint) -> MongoResult {
        let collection = blueprint.table().to_owned();
        if let Some(error) = blueprint.extras.expire_errors.first() {
            return Err(MongoError::schema(&collection, error.clone()));
        }
        if self.has_table(&collection).await? {
            if blueprint.creates_if_missing() {
                return Ok(());
            }
            return Err(MongoError::schema(
                &collection,
                "the collection already exists",
            ));
        }
        let plan = compile::create(&blueprint.blueprint, &blueprint.extras)?;
        for notice in plan.notices {
            self.notice(notice);
        }
        for command in plan.commands {
            self.run(command).await?;
        }
        Ok(())
    }

    /// Changes `collection` as `f` describes: new and changed fields (the
    /// validator), dropped and renamed fields (in every document, the
    /// validator and the indexes), indexes, renames and drops.
    pub async fn table(
        &self,
        collection: &str,
        f: impl FnOnce(&mut MongoBlueprint),
    ) -> MongoResult {
        let mut blueprint =
            MongoBlueprint::with(Blueprint::alter(collection, self.options.clone()));
        f(&mut blueprint);
        self.alter(blueprint).await
    }

    async fn alter(&self, blueprint: MongoBlueprint) -> MongoResult {
        let collection = blueprint.table().to_owned();
        if let Some(error) = blueprint.extras.expire_errors.first() {
            return Err(MongoError::schema(&collection, error.clone()));
        }
        if blueprint.hypertable_definition().is_some() {
            return Err(convert_error(&collection));
        }
        let key = compile::key_column(&blueprint.blueprint);
        let field = |name: &str| -> String {
            if key.as_deref() == Some(name) && name == "id" {
                "_id".to_owned()
            } else {
                name.to_owned()
            }
        };
        let mut validator = self.validator(&collection).await?;
        let mut validator_changed = false;
        let mut notices = Vec::new();
        for command in blueprint.commands() {
            match command {
                Command::Column(index) => {
                    let column = &blueprint.columns()[*index];
                    let name = field(&column.name);
                    let (property, required) =
                        compile::property(&collection, column, &mut notices)?;
                    if !column.change && required && column.default.is_none() {
                        let existing = self
                            .db
                            .collection(&collection)
                            .estimated_document_count()
                            .await?;
                        if existing > 0 {
                            return Err(MongoError::schema(
                                &collection,
                                format!(
                                    "`{}` is required, but the collection's {existing} documents lack it: give it a default (their field is set to it) or make it nullable",
                                    column.name
                                ),
                            ));
                        }
                    }
                    set_property(&mut validator, &name, Bson::Document(property), required);
                    validator_changed = true;
                    if !column.change
                        && let Some(default) =
                            column.default.as_ref().and_then(compile::default_value)
                    {
                        self.update_all(
                            &collection,
                            doc! { name.clone(): { "$exists": false } },
                            doc! { "$set": { name.clone(): default } },
                        )
                        .await?;
                    }
                }
                Command::Index(index) => {
                    if index.kind == IndexKind::Primary
                        && index.columns.len() == 1
                        && field(&index.columns[0]) == "_id"
                    {
                        continue;
                    }
                    let spec = compile::index_spec(
                        &collection,
                        index,
                        &field,
                        &|_| false,
                        false,
                        &mut notices,
                    )?;
                    self.run(SchemaCommand::database(doc! {
                        "createIndexes": collection.clone(),
                        "indexes": [spec],
                    }))
                    .await?;
                }
                Command::Foreign(foreign) => {
                    notices.push(format!(
                        "MongoDB has no foreign keys: `{collection}.{}` is not enforced; an index on it keeps lookups fast",
                        foreign.columns.join(", ")
                    ));
                    let mut key = Document::new();
                    for column in &foreign.columns {
                        key.insert(field(column), 1);
                    }
                    self.run(SchemaCommand::database(doc! {
                        "createIndexes": collection.clone(),
                        "indexes": [{ "key": key, "name": foreign.name.clone() }],
                    }))
                    .await?;
                }
                Command::DropColumn(columns) => {
                    let fields: Vec<String> = columns.iter().map(|c| field(c)).collect();
                    let mut unset = Document::new();
                    for name in &fields {
                        unset.insert(name.clone(), "");
                        remove_property(&mut validator, name);
                    }
                    validator_changed = true;
                    self.update_all(&collection, Document::new(), doc! { "$unset": unset })
                        .await?;
                    // Like SQL, the indexes over a dropped column go too.
                    for index in self.indexes(&collection).await? {
                        let uses = index
                            .get_document("key")
                            .is_ok_and(|key| key.keys().any(|k| fields.contains(k)));
                        if uses && let Ok(name) = index.get_str("name") {
                            self.run(SchemaCommand::database(doc! {
                                "dropIndexes": collection.clone(), "index": name,
                            }))
                            .await?;
                        }
                    }
                }
                Command::RenameColumn { from, to } => {
                    let (from, to) = (field(from), field(to));
                    self.update_all(
                        &collection,
                        doc! { from.clone(): { "$exists": true } },
                        doc! { "$rename": { from.clone(): to.clone() } },
                    )
                    .await?;
                    rename_property(&mut validator, &from, &to);
                    validator_changed = true;
                    // Indexes over the old name are rebuilt over the new one.
                    for index in self.indexes(&collection).await? {
                        let Ok(key) = index.get_document("key") else {
                            continue;
                        };
                        if !key.contains_key(&from) {
                            continue;
                        }
                        let Ok(name) = index.get_str("name") else {
                            continue;
                        };
                        let renamed: Document = key
                            .iter()
                            .map(|(k, v)| {
                                (if *k == from { to.clone() } else { k.clone() }, v.clone())
                            })
                            .collect();
                        let mut rebuilt = rebuildable(&index);
                        rebuilt.insert("key", renamed);
                        self.run(SchemaCommand::database(doc! {
                            "dropIndexes": collection.clone(), "index": name,
                        }))
                        .await?;
                        self.run(SchemaCommand::database(doc! {
                            "createIndexes": collection.clone(), "indexes": [rebuilt],
                        }))
                        .await?;
                    }
                }
                Command::DropIndex {
                    kind,
                    name,
                    columns,
                } => {
                    if *kind == IndexKind::Primary
                        && (name.is_none() || columns.iter().any(|c| field(c) == "_id"))
                    {
                        return Err(MongoError::schema(
                            &collection,
                            "the `_id` index cannot be dropped",
                        ));
                    }
                    if let Some(name) = name {
                        self.run(SchemaCommand::database(doc! {
                            "dropIndexes": collection.clone(), "index": name.clone(),
                        }))
                        .await?;
                    }
                }
                Command::DropForeign { name, .. } => {
                    let exists = self
                        .indexes(&collection)
                        .await?
                        .iter()
                        .any(|index| index.get_str("name").ok() == Some(name.as_str()));
                    if exists {
                        self.run(SchemaCommand::database(doc! {
                            "dropIndexes": collection.clone(), "index": name.clone(),
                        }))
                        .await?;
                    }
                }
                Command::RenameIndex { from, to } => {
                    let Some(index) = self
                        .indexes(&collection)
                        .await?
                        .into_iter()
                        .find(|index| index.get_str("name").ok() == Some(from.as_str()))
                    else {
                        return Err(MongoError::schema(
                            &collection,
                            format!("there is no index `{from}` to rename"),
                        ));
                    };
                    // MongoDB cannot rename an index: it is rebuilt.
                    let mut rebuilt = rebuildable(&index);
                    rebuilt.insert("name", to.clone());
                    self.run(SchemaCommand::database(doc! {
                        "dropIndexes": collection.clone(), "index": from.clone(),
                    }))
                    .await?;
                    self.run(SchemaCommand::database(doc! {
                        "createIndexes": collection.clone(), "indexes": [rebuilt],
                    }))
                    .await?;
                }
                Command::Rename(to) => self.rename(&collection, to).await?,
                Command::Drop => self.drop(&collection).await?,
                Command::DropIfExists => self.drop_if_exists(&collection).await?,
                Command::Comment(comment) => {
                    validator.insert("description", comment.clone());
                    validator_changed = true;
                }
                other => {
                    return Err(MongoError::unsupported(
                        format!("the change {other:?}"),
                        "MongoDB collections have no such setting",
                    ));
                }
            }
        }
        // Indexes declared on the columns (`.unique()`, `.index()`), TTL
        // indexes, and indexes a changed column drops.
        let (specs, drops) =
            compile::column_indexes(&blueprint.blueprint, &blueprint.extras, &mut notices)?;
        for name in drops {
            self.run(SchemaCommand::database(doc! {
                "dropIndexes": collection.clone(), "index": name,
            }))
            .await?;
        }
        if !specs.is_empty() {
            self.run(SchemaCommand::database(doc! {
                "createIndexes": collection.clone(),
                "indexes": specs,
            }))
            .await?;
        }
        if validator_changed
            && !blueprint.extras.without_validator
            && self.is_time_series(&collection).await?
        {
            notices.push(compile::time_series_unchecked(&collection));
        } else if validator_changed && !blueprint.extras.without_validator {
            let mut command =
                doc! { "collMod": collection.clone(), "validator": { "$jsonSchema": validator } };
            if let Some(level) = &blueprint.extras.validation_level {
                command.insert("validationLevel", level.clone());
            }
            if let Some(action) = &blueprint.extras.validation_action {
                command.insert("validationAction", action.clone());
            }
            self.run(SchemaCommand::database(command)).await?;
        }
        for notice in notices {
            self.notice(notice);
        }
        Ok(())
    }

    /// An `update` command over every matching document (it runs in a
    /// migration's session). The documents are moving to the new schema,
    /// so the validator they are leaving does not check them (the new one
    /// is set once the change is done). A document the server refuses
    /// fails the change.
    async fn update_all(
        &self,
        collection: &str,
        filter: Document,
        update: Document,
    ) -> MongoResult {
        let reply = self
            .run(SchemaCommand::database(doc! {
                "update": collection,
                "updates": [{ "q": filter, "u": update, "multi": true }],
                "bypassDocumentValidation": true,
            }))
            .await?;
        if let Some(problem) = reply.as_ref().and_then(write_problem) {
            return Err(MongoError::schema(
                collection,
                format!("updating its documents failed: {problem}"),
            ));
        }
        Ok(())
    }

    /// `listCollections`' entry for `collection` (or a view of that name).
    async fn collection_info(&self, collection: &str) -> MongoResult<Option<Document>> {
        let reply = self
            .read(doc! { "listCollections": 1, "filter": { "name": collection } })
            .await?;
        Ok(reply
            .get_document("cursor")
            .ok()
            .and_then(|cursor| cursor.get_array("firstBatch").ok())
            .and_then(|batch| batch.first())
            .and_then(Bson::as_document)
            .cloned())
    }

    /// The `$jsonSchema` of `collection`'s validator (an empty object
    /// schema when it has none, or does not exist).
    pub async fn validator(&self, collection: &str) -> MongoResult<Document> {
        Ok(self
            .collection_info(collection)
            .await?
            .and_then(|info| info.get_document("options").ok().cloned())
            .and_then(|options| options.get_document("validator").ok().cloned())
            .and_then(|validator| validator.get_document("$jsonSchema").ok().cloned())
            .unwrap_or_else(|| doc! { "bsonType": "object", "properties": {} }))
    }

    /// Whether `collection` is a time-series collection.
    async fn is_time_series(&self, collection: &str) -> MongoResult<bool> {
        Ok(self
            .collection_info(collection)
            .await?
            .is_some_and(|info| info.get_str("type").ok() == Some("timeseries")))
    }

    /// The options `collection` was created with (`timeseries`,
    /// `validator`, `expireAfterSeconds`, `viewOn`, ...).
    pub async fn collection_options(&self, collection: &str) -> MongoResult<Option<Document>> {
        Ok(self
            .collection_info(collection)
            .await?
            .and_then(|info| info.get_document("options").ok().cloned()))
    }

    /// Renames `from` to `to`.
    pub async fn rename(&self, from: &str, to: &str) -> MongoResult {
        let database = self.db.database().name().to_owned();
        self.run(SchemaCommand::admin(doc! {
            "renameCollection": format!("{database}.{from}"),
            "to": format!("{database}.{to}"),
        }))
        .await?;
        Ok(())
    }

    /// Drops `collection` (a missing one is an error).
    pub async fn drop(&self, collection: &str) -> MongoResult {
        if !self.pretend && !self.has_table(collection).await? {
            return Err(MongoError::schema(
                collection,
                "the collection does not exist",
            ));
        }
        self.run(SchemaCommand::database(doc! { "drop": collection }))
            .await?;
        Ok(())
    }

    /// Drops `collection` if it exists.
    pub async fn drop_if_exists(&self, collection: &str) -> MongoResult {
        if self.has_table(collection).await? {
            self.run(SchemaCommand::database(doc! { "drop": collection }))
                .await?;
        }
        Ok(())
    }

    /// Whether `collection` (or a view of that name) exists.
    pub async fn has_table(&self, collection: &str) -> MongoResult<bool> {
        Ok(self.collection_info(collection).await?.is_some())
    }

    /// Whether `collection`'s validator declares `field`, or a document has
    /// it.
    pub async fn has_column(&self, collection: &str, field: &str) -> MongoResult<bool> {
        let declared = self
            .validator(collection)
            .await?
            .get_document("properties")
            .is_ok_and(|properties| properties.contains_key(field));
        if declared {
            return Ok(true);
        }
        Ok(self
            .db
            .collection(collection)
            .find_one(doc! { field: { "$exists": true } })
            .await?
            .is_some())
    }

    /// The collections and views of the database (not MongoDB's own).
    pub async fn collections(&self) -> MongoResult<Vec<String>> {
        let mut names = self.db.database().list_collection_names().await?;
        names.retain(|name| !name.starts_with("system."));
        names.sort();
        Ok(names)
    }

    /// The indexes of `collection`, as `listIndexes` gives them (`key`,
    /// `name`, `unique`, `expireAfterSeconds`, ...); none when it does not
    /// exist.
    pub async fn indexes(&self, collection: &str) -> MongoResult<Vec<Document>> {
        if !self.has_table(collection).await? {
            return Ok(Vec::new());
        }
        let reply = self.read(doc! { "listIndexes": collection }).await?;
        Ok(reply
            .get_document("cursor")
            .ok()
            .and_then(|cursor| cursor.get_array("firstBatch").ok())
            .map(|batch| {
                batch
                    .iter()
                    .filter_map(Bson::as_document)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Creates the view `name` over `source`, computed by `pipeline` when
    /// read.
    pub async fn create_view(
        &self,
        name: &str,
        source: &str,
        pipeline: Vec<Document>,
    ) -> MongoResult {
        self.run(SchemaCommand::database(doc! {
            "create": name, "viewOn": source, "pipeline": pipeline,
        }))
        .await?;
        Ok(())
    }

    /// A continuous aggregate described with `ContinuousAggregate::over`,
    /// as a view grouping by its time bucket and groups (MongoDB computes
    /// views when they are read).
    pub async fn create_continuous_aggregate(
        &self,
        aggregate: &ContinuousAggregate,
    ) -> MongoResult {
        let mut notices = Vec::new();
        let command = compile::view(aggregate, &mut notices)?;
        for notice in notices {
            self.notice(notice);
        }
        self.run(command).await?;
        Ok(())
    }

    /// MongoDB cannot turn a collection into a time-series collection: an
    /// error explaining how to move the documents instead.
    pub async fn convert_to_hypertable(
        &self,
        collection: &str,
        _hypertable: impl Into<Hypertable>,
    ) -> MongoResult {
        Err(convert_error(collection))
    }

    /// Drops every collection and view of the database
    /// (`mongo:migrate:fresh`); returns their names.
    pub async fn drop_all(&self) -> MongoResult<Vec<String>> {
        let names = self.collections().await?;
        for name in &names {
            self.run(SchemaCommand::database(doc! { "drop": name.clone() }))
                .await?;
        }
        Ok(names)
    }
}

/// The error for converting an existing collection.
fn convert_error(collection: &str) -> MongoError {
    MongoError::schema(
        collection,
        "MongoDB cannot turn an existing collection into a time-series collection: create one with `create_hypertable`, copy the documents into it (an aggregation ending in `$out` to it), then drop the old collection",
    )
}

/// What went wrong in a write command's reply, which reports refused
/// documents (`writeErrors`) and an unmet write concern
/// (`writeConcernError`) beside `ok: 1`.
pub(crate) fn write_problem(reply: &Document) -> Option<String> {
    let message = |error: &Document| {
        let text = error.get_str("errmsg").unwrap_or("refused");
        match error.get("code").and_then(Bson::as_i32) {
            Some(code) => format!("{text} (code {code})"),
            None => text.to_owned(),
        }
    };
    if let Ok(errors) = reply.get_array("writeErrors")
        && let Some(first) = errors.iter().find_map(Bson::as_document)
    {
        let more = errors.len() - 1;
        return Some(if more > 0 {
            format!("{}, and {more} more", message(first))
        } else {
            message(first)
        });
    }
    reply.get_document("writeConcernError").ok().map(message)
}

/// An index as `listIndexes` gives it, ready for `createIndexes`: without
/// the version and namespace the server adds.
fn rebuildable(index: &Document) -> Document {
    index
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "v" | "ns"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Sets `field`'s property in a `$jsonSchema` (and whether it is
/// required).
fn set_property(schema: &mut Document, field: &str, property: Bson, required: bool) {
    let mut properties = schema
        .get_document("properties")
        .cloned()
        .unwrap_or_default();
    properties.insert(field, property);
    schema.insert("properties", properties);
    let mut list: Vec<Bson> = schema.get_array("required").cloned().unwrap_or_default();
    list.retain(|name| name.as_str() != Some(field));
    if required {
        list.push(Bson::String(field.to_owned()));
    }
    if list.is_empty() {
        schema.remove("required");
    } else {
        schema.insert("required", list);
    }
}

/// Removes `field` from a `$jsonSchema`.
fn remove_property(schema: &mut Document, field: &str) {
    if let Ok(properties) = schema.get_document_mut("properties") {
        properties.remove(field);
    }
    if let Ok(required) = schema.get_array_mut("required") {
        required.retain(|name| name.as_str() != Some(field));
    }
    if schema.get_array("required").is_ok_and(Vec::is_empty) {
        schema.remove("required");
    }
}

/// Renames `from` to `to` in a `$jsonSchema`.
fn rename_property(schema: &mut Document, from: &str, to: &str) {
    if let Ok(properties) = schema.get_document_mut("properties")
        && let Some(property) = properties.remove(from)
    {
        properties.insert(to, property);
    }
    if let Ok(required) = schema.get_array_mut("required") {
        for name in required.iter_mut() {
            if name.as_str() == Some(from) {
                *name = Bson::String(to.to_owned());
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_replies_name_what_was_refused() {
        assert_eq!(
            write_problem(&doc! { "ok": 1, "n": 2, "nModified": 2 }),
            None
        );
        let refused = doc! {
            "ok": 1,
            "writeErrors": [
                { "index": 0, "code": 121, "errmsg": "Document failed validation" },
                { "index": 1, "code": 121, "errmsg": "Document failed validation" },
            ],
        };
        assert_eq!(
            write_problem(&refused).as_deref(),
            Some("Document failed validation (code 121), and 1 more")
        );
        let concern = doc! { "ok": 1, "writeConcernError": { "code": 64, "errmsg": "waiting for replication timed out" } };
        assert_eq!(
            write_problem(&concern).as_deref(),
            Some("waiting for replication timed out (code 64)")
        );
    }

    #[test]
    fn validators_change_property_by_property() {
        let mut schema = doc! { "bsonType": "object", "properties": {} };
        set_property(
            &mut schema,
            "title",
            Bson::Document(doc! { "bsonType": "string" }),
            true,
        );
        set_property(&mut schema, "note", Bson::Document(doc! {}), false);
        assert_eq!(
            schema,
            doc! {
                "bsonType": "object",
                "properties": { "title": { "bsonType": "string" }, "note": {} },
                "required": ["title"],
            }
        );
        rename_property(&mut schema, "title", "name");
        assert_eq!(
            schema.get_array("required").unwrap(),
            &vec![Bson::String("name".into())]
        );
        assert!(
            schema
                .get_document("properties")
                .unwrap()
                .contains_key("name")
        );
        remove_property(&mut schema, "name");
        assert!(!schema.contains_key("required"));
        assert_eq!(
            schema.get_document("properties").unwrap(),
            &doc! { "note": {} }
        );
    }

    #[test]
    fn listed_indexes_rebuild_without_server_fields() {
        let index = doc! { "v": 2, "key": { "email": 1 }, "name": "users_email_unique", "unique": true, "ns": "app.users" };
        assert_eq!(
            rebuildable(&index),
            doc! { "key": { "email": 1 }, "name": "users_email_unique", "unique": true }
        );
    }
}
