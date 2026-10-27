//! Blueprints as MongoDB commands: a created table is a collection with a
//! `$jsonSchema` validator and indexes, a hypertable a time-series
//! collection, a described continuous aggregate a view.

use bson::{Bson, Document, doc};
use navree_schema::{
    AggregateSpec, Blueprint, ColumnDefinition, ColumnType, Command, ContinuousAggregate,
    DefaultValue, Hypertable, IndexDefinition, IndexKind, Interval,
};

use crate::error::{MongoError, MongoResult};

/// A command for the database, or for the `admin` database
/// (`renameCollection`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SchemaCommand {
    /// Runs against `admin` rather than the app's database.
    pub(crate) admin: bool,
    /// The command document.
    pub(crate) document: Document,
}

impl SchemaCommand {
    pub(crate) fn database(document: Document) -> SchemaCommand {
        SchemaCommand {
            admin: false,
            document,
        }
    }

    pub(crate) fn admin(document: Document) -> SchemaCommand {
        SchemaCommand {
            admin: true,
            document,
        }
    }
}

impl std::fmt::Display for SchemaCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let body = crate::command::render_document(&self.document);
        if self.admin {
            write!(f, "db.adminCommand({body})")
        } else {
            write!(f, "db.runCommand({body})")
        }
    }
}

/// What creating a collection takes: commands, and notices of what MongoDB
/// does differently.
#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub(crate) commands: Vec<SchemaCommand>,
    pub(crate) notices: Vec<String>,
}

/// The Mongo-only settings of a [`MongoBlueprint`](crate::MongoBlueprint).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Extras {
    /// TTL indexes: `(field, seconds)`.
    pub(crate) expire: Vec<(String, u64)>,
    /// `validationLevel`.
    pub(crate) validation_level: Option<String>,
    /// `validationAction`.
    pub(crate) validation_action: Option<String>,
    /// No validator at all.
    pub(crate) without_validator: bool,
    /// A capped collection: `(size in bytes, most documents)`.
    pub(crate) capped: Option<(u64, Option<u64>)>,
    /// TTL settings that did not parse, reported when the blueprint runs.
    pub(crate) expire_errors: Vec<String>,
}

/// The field a column is stored in: the key column `id` is `_id`, as models
/// on MongoDB store their `id` key.
pub(crate) fn field_name(column: &ColumnDefinition, key: Option<&str>) -> String {
    if key == Some(column.name.as_str()) && column.name == "id" {
        "_id".to_owned()
    } else {
        column.name.clone()
    }
}

/// The primary key's column, when it is one column: declared with
/// `.primary()`, an auto-incrementing column, or a `primary([..])` index of
/// one column.
pub(crate) fn key_column(blueprint: &Blueprint) -> Option<String> {
    for column in blueprint.columns() {
        if column.auto_increment
            || matches!(column.primary, Some(navree_schema::FluentIndex::Add(_)))
        {
            return Some(column.name.clone());
        }
    }
    blueprint.commands().iter().find_map(|command| match command {
        Command::Index(index) if index.kind == IndexKind::Primary && index.columns.len() == 1 => {
            Some(index.columns[0].clone())
        }
        _ => None,
    })
}

/// The BSON type aliases MongoDB's `$jsonSchema` knows, accepted as the
/// type of a raw column (`raw_column("owner", "objectId")`).
const BSON_TYPES: [&str; 20] = [
    "double",
    "string",
    "object",
    "array",
    "binData",
    "objectId",
    "bool",
    "date",
    "null",
    "regex",
    "javascript",
    "int",
    "timestamp",
    "long",
    "decimal",
    "minKey",
    "maxKey",
    "number",
    "dbPointer",
    "symbol",
];

/// The `$jsonSchema` property of a column, and whether it is required.
pub(crate) fn property(
    collection: &str,
    column: &ColumnDefinition,
    notices: &mut Vec<String>,
) -> MongoResult<(Document, bool)> {
    let mut property = Document::new();
    let types: Option<Vec<&str>> = match &column.kind {
        ColumnType::Char { length } | ColumnType::String { length } => {
            property.insert("maxLength", i64::from(*length));
            Some(vec!["string"])
        }
        ColumnType::TinyText
        | ColumnType::Text
        | ColumnType::MediumText
        | ColumnType::LongText
        | ColumnType::Time { .. }
        | ColumnType::TimeTz { .. }
        | ColumnType::IpAddress
        | ColumnType::MacAddress => Some(vec!["string"]),
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::MediumInteger
        | ColumnType::Integer
        | ColumnType::BigInteger
        | ColumnType::Year => Some(vec!["int", "long"]),
        ColumnType::Float { .. } | ColumnType::Double => Some(vec!["number"]),
        ColumnType::Decimal { .. } => Some(vec!["decimal"]),
        ColumnType::Boolean => Some(vec!["bool"]),
        ColumnType::Enum { values } => {
            let mut allowed: Vec<Bson> = values.iter().map(|v| Bson::String(v.clone())).collect();
            if column.nullable == Some(true) {
                allowed.push(Bson::Null);
            }
            property.insert("enum", allowed);
            Some(vec!["string"])
        }
        ColumnType::Set { values } => {
            property.insert(
                "items",
                doc! { "enum": values.iter().map(|v| Bson::String(v.clone())).collect::<Vec<_>>() },
            );
            property.insert("uniqueItems", true);
            Some(vec!["array"])
        }
        ColumnType::Json | ColumnType::Jsonb => None,
        ColumnType::Date
        | ColumnType::DateTime { .. }
        | ColumnType::DateTimeTz { .. }
        | ColumnType::Timestamp { .. }
        | ColumnType::TimestampTz { .. } => Some(vec!["date"]),
        ColumnType::Binary { .. } | ColumnType::Uuid => Some(vec!["binData"]),
        ColumnType::Geometry { .. } | ColumnType::Geography { .. } => Some(vec!["object"]),
        ColumnType::Vector { dimensions } => {
            property.insert("items", doc! { "bsonType": "number" });
            if let Some(dimensions) = dimensions {
                property.insert("minItems", i64::from(*dimensions));
                property.insert("maxItems", i64::from(*dimensions));
            }
            Some(vec!["array"])
        }
        ColumnType::TsVector => {
            return Err(MongoError::schema(
                collection,
                format!(
                    "`{}` is a PostgreSQL full-text vector, which MongoDB has no type for: index the text fields with a full-text index (`t.fulltext([..])`)",
                    column.name
                ),
            ));
        }
        ColumnType::Raw { definition } => match BSON_TYPES.iter().find(|alias| **alias == definition.trim()) {
            Some(alias) => Some(vec![*alias]),
            None => {
                notices.push(format!(
                    "MongoDB does not check the type of `{collection}.{}`: `{definition}` is SQL (a raw column type of BSON's, such as `objectId`, is checked)",
                    column.name
                ));
                None
            }
        },
        other => {
            notices.push(format!(
                "MongoDB does not check the type of `{collection}.{}` ({})",
                column.name,
                other.name()
            ));
            None
        }
    };
    if column.unsigned && column.kind.is_integer() {
        property.insert("minimum", 0);
    }
    if let Some(types) = types {
        let mut types: Vec<Bson> = types.into_iter().map(|t| Bson::String(t.to_owned())).collect();
        if column.nullable == Some(true) {
            types.push(Bson::String("null".into()));
        }
        let bson_type = if types.len() == 1 {
            types.remove(0)
        } else {
            Bson::Array(types)
        };
        // `bsonType` first: it reads best.
        let mut ordered = doc! { "bsonType": bson_type };
        ordered.extend(property);
        property = ordered;
    }
    if let Some(comment) = &column.comment {
        property.insert("description", comment.clone());
    }
    let required = column.nullable != Some(true);
    Ok((property, required))
}

/// The `$jsonSchema` of `blueprint`'s columns (`None` without columns).
pub(crate) fn json_schema(
    blueprint: &Blueprint,
    comment: Option<&str>,
    notices: &mut Vec<String>,
) -> MongoResult<Option<Document>> {
    if blueprint.columns().is_empty() {
        return Ok(None);
    }
    let key = key_column(blueprint);
    let mut properties = Document::new();
    let mut required: Vec<Bson> = Vec::new();
    for column in blueprint.columns() {
        check_column(blueprint.table(), column)?;
        let field = field_name(column, key.as_deref());
        let (property, is_required) = property(blueprint.table(), column, notices)?;
        if is_required {
            required.push(Bson::String(field.clone()));
        }
        properties.insert(field, property);
    }
    let mut schema = doc! { "bsonType": "object" };
    if !required.is_empty() {
        schema.insert("required", required);
    }
    schema.insert("properties", properties);
    if let Some(comment) = comment {
        schema.insert("description", comment);
    }
    Ok(Some(schema))
}

/// Refuses column modifiers MongoDB cannot honour.
fn check_column(collection: &str, column: &ColumnDefinition) -> MongoResult {
    if column.virtual_as.is_some() || column.stored_as.is_some() {
        return Err(MongoError::schema(
            collection,
            format!(
                "`{}` is a generated column, which MongoDB has not: compute it in Rust before saving, or in a view",
                column.name
            ),
        ));
    }
    Ok(())
}

/// The value of a column default, for backfilling existing documents.
pub(crate) fn default_value(default: &DefaultValue) -> Option<Bson> {
    match default {
        DefaultValue::Null => Some(Bson::Null),
        DefaultValue::Bool(b) => Some(Bson::Boolean(*b)),
        DefaultValue::Int(i) => Some(Bson::Int64(*i)),
        DefaultValue::UInt(u) => i64::try_from(*u).ok().map(Bson::Int64),
        DefaultValue::Float(f) => Some(Bson::Double(*f)),
        DefaultValue::Text(text) => Some(Bson::String(text.clone())),
        _ => None,
    }
}

/// The indexes a created blueprint declares (commands and column
/// modifiers), foreign keys as indexes on their columns, and TTL indexes;
/// `None` when there are none.
pub(crate) fn indexes(
    blueprint: &Blueprint,
    extras: &Extras,
    time_series: bool,
    notices: &mut Vec<String>,
) -> MongoResult<Vec<Document>> {
    let collection = blueprint.table();
    let key = key_column(blueprint);
    let field = |column: &str| -> String {
        if key.as_deref() == Some(column) && column == "id" {
            "_id".to_owned()
        } else {
            column.to_owned()
        }
    };
    let nullable = |column: &str| -> bool {
        blueprint
            .columns()
            .iter()
            .any(|c| c.name == column && c.nullable == Some(true))
    };
    let mut specs: Vec<Document> = Vec::new();
    let mut declared: Vec<IndexDefinition> = Vec::new();
    // Foreign keys: indexes on their columns, unless another index starts
    // with them (checked once every other index is known).
    let mut foreign_keys: Vec<IndexDefinition> = Vec::new();
    for command in blueprint.commands() {
        match command {
            Command::Index(index) => declared.push(index.clone()),
            Command::Foreign(foreign) => {
                notices.push(format!(
                    "MongoDB has no foreign keys: `{collection}.{}` → `{}` is not enforced; an index on it keeps lookups fast",
                    foreign.columns.join(", "),
                    foreign.on.clone().unwrap_or_else(|| "?".into())
                ));
                foreign_keys.push(IndexDefinition::new(
                    IndexKind::Index,
                    foreign.name.clone(),
                    foreign.columns.clone(),
                ));
            }
            Command::Comment(_) => {}
            other if blueprint.is_creating() => {
                return Err(MongoError::schema(
                    collection,
                    format!("{other:?} does not belong in a created collection"),
                ));
            }
            _ => {}
        }
    }
    // Indexes declared on the columns themselves.
    for column in blueprint.columns() {
        for (kind, fluent) in [
            (IndexKind::Primary, &column.primary),
            (IndexKind::Unique, &column.unique),
            (IndexKind::Index, &column.index),
            (IndexKind::Fulltext, &column.fulltext),
            (IndexKind::Spatial, &column.spatial_index),
        ] {
            if let Some(navree_schema::FluentIndex::Add(name)) = fluent {
                let name = name.clone().unwrap_or_else(|| {
                    format!("{collection}_{}_{}", column.name, kind.suffix())
                });
                declared.push(IndexDefinition::new(kind, name, vec![column.name.clone()]));
            }
        }
    }
    for index in declared {
        let is_id = index.kind == IndexKind::Primary
            && index.columns.len() == 1
            && field(&index.columns[0]) == "_id";
        if is_id {
            continue; // MongoDB's own `_id` index.
        }
        specs.push(index_spec(collection, &index, &field, &nullable, time_series, notices)?);
    }
    for foreign in foreign_keys {
        let covered = specs.iter().any(|spec| {
            spec.get_document("key").is_ok_and(|key| {
                let fields: Vec<&String> = key.keys().collect();
                fields.len() >= foreign.columns.len()
                    && foreign
                        .columns
                        .iter()
                        .zip(&fields)
                        .all(|(column, f)| field(column) == **f)
            })
        }) || (foreign.columns.len() == 1 && field(&foreign.columns[0]) == "_id");
        if !covered {
            specs.push(index_spec(collection, &foreign, &field, &nullable, time_series, notices)?);
        }
    }
    for (column, seconds) in &extras.expire {
        let field = field(column);
        specs.push(doc! {
            "key": { field.clone(): 1 },
            "name": format!("{collection}_{column}_ttl"),
            "expireAfterSeconds": i64::try_from(*seconds).unwrap_or(i64::MAX),
        });
    }
    Ok(specs)
}

/// The indexes an alteration declares on its columns (`.unique()`,
/// `.index()`, ...) and its TTL indexes, as `createIndexes` specs; and the
/// conventional names of the indexes a changed column drops
/// (`.unique(false)`-style `without_*`).
pub(crate) fn column_indexes(
    blueprint: &Blueprint,
    extras: &Extras,
    notices: &mut Vec<String>,
) -> MongoResult<(Vec<Document>, Vec<String>)> {
    let collection = blueprint.table();
    let key = key_column(blueprint);
    let field = |column: &str| -> String {
        if key.as_deref() == Some(column) && column == "id" {
            "_id".to_owned()
        } else {
            column.to_owned()
        }
    };
    let nullable = |column: &str| -> bool {
        blueprint
            .columns()
            .iter()
            .any(|c| c.name == column && c.nullable == Some(true))
    };
    let mut specs = Vec::new();
    let mut drops = Vec::new();
    for column in blueprint.columns() {
        for (kind, fluent) in [
            (IndexKind::Primary, &column.primary),
            (IndexKind::Unique, &column.unique),
            (IndexKind::Index, &column.index),
            (IndexKind::Fulltext, &column.fulltext),
            (IndexKind::Spatial, &column.spatial_index),
        ] {
            let conventional = format!("{collection}_{}_{}", column.name, kind.suffix());
            match fluent {
                Some(navree_schema::FluentIndex::Add(name)) => {
                    if kind == IndexKind::Primary && field(&column.name) == "_id" {
                        continue;
                    }
                    let index = IndexDefinition::new(
                        kind,
                        name.clone().unwrap_or(conventional),
                        vec![column.name.clone()],
                    );
                    specs.push(index_spec(collection, &index, &field, &nullable, false, notices)?);
                }
                Some(navree_schema::FluentIndex::Drop) if column.change => drops.push(conventional),
                _ => {}
            }
        }
    }
    for (column, seconds) in &extras.expire {
        specs.push(doc! {
            "key": { field(column): 1 },
            "name": format!("{collection}_{column}_ttl"),
            "expireAfterSeconds": i64::try_from(*seconds).unwrap_or(i64::MAX),
        });
    }
    Ok((specs, drops))
}

/// One index of `createIndexes`.
pub(crate) fn index_spec(
    collection: &str,
    index: &IndexDefinition,
    field: &dyn Fn(&str) -> String,
    nullable: &dyn Fn(&str) -> bool,
    time_series: bool,
    notices: &mut Vec<String>,
) -> MongoResult<Document> {
    if index.expression.is_some() {
        return Err(MongoError::schema(
            collection,
            format!(
                "the index `{}` is on an SQL expression: MongoDB indexes fields (index the field, or store the computed value)",
                index.name
            ),
        ));
    }
    if index.columns.is_empty() {
        return Err(MongoError::schema(
            collection,
            format!("the index `{}` names no field", index.name),
        ));
    }
    let unique = matches!(index.kind, IndexKind::Primary | IndexKind::Unique);
    if unique && time_series {
        return Err(MongoError::schema(
            collection,
            format!(
                "MongoDB time-series collections have no unique indexes (`{}`): drop it, or keep `{collection}` a regular collection",
                index.name
            ),
        ));
    }
    let direction = |_: &str| -> Bson {
        match (index.kind, index.algorithm.as_deref()) {
            (IndexKind::Fulltext, _) => Bson::String("text".into()),
            (IndexKind::Spatial, _) => Bson::String("2dsphere".into()),
            (_, Some(algorithm)) if algorithm.eq_ignore_ascii_case("hash") || algorithm.eq_ignore_ascii_case("hashed") => {
                Bson::String("hashed".into())
            }
            _ => Bson::Int32(1),
        }
    };
    if let Some(algorithm) = &index.algorithm
        && !matches!(algorithm.to_ascii_lowercase().as_str(), "hash" | "hashed" | "btree")
    {
        notices.push(format!(
            "MongoDB has no `{algorithm}` indexes: `{}` is a regular index",
            index.name
        ));
    }
    let mut key = Document::new();
    for column in &index.columns {
        key.insert(field(column), direction(column));
    }
    let mut spec = doc! { "key": key, "name": index.name.clone() };
    if unique {
        spec.insert("unique", true);
        // Like SQL, documents with a NULL in the key do not collide: the
        // index covers the documents whose nullable fields are set.
        let nullable_fields: Vec<String> = index
            .columns
            .iter()
            .filter(|column| nullable(column))
            .map(|column| field(column))
            .collect();
        if !nullable_fields.is_empty() {
            let mut filter = Document::new();
            for field in nullable_fields {
                filter.insert(field, doc! { "$exists": true, "$type": NOT_NULL_TYPES.to_vec() });
            }
            spec.insert("partialFilterExpression", filter);
        }
    }
    if index.kind == IndexKind::Fulltext
        && let Some(language) = &index.language
    {
        spec.insert("default_language", language.to_ascii_lowercase());
    }
    if let Some(predicate) = &index.predicate {
        let filter: serde_json::Value = serde_json::from_str(predicate.as_str()).map_err(|_| {
            MongoError::schema(
                collection,
                format!(
                    "the partial index `{}` filters with SQL (`{}`): on MongoDB write the filter as a JSON document, e.g. `{{\"deleted_at\": {{\"$exists\": false}}}}`",
                    index.name,
                    predicate.as_str()
                ),
            )
        })?;
        let filter = crate::convert::json_to_bson(&filter)
            .map_err(|error| MongoError::schema(collection, error.to_string()))?;
        if let Bson::Document(filter) = filter {
            spec.insert("partialFilterExpression", filter);
        }
    }
    Ok(spec)
}

/// Every BSON type but null, for unique indexes over nullable fields.
const NOT_NULL_TYPES: [&str; 14] = [
    "double", "string", "object", "array", "binData", "objectId", "bool", "date", "int",
    "timestamp", "long", "decimal", "regex", "javascript",
];

/// The `timeseries` options and `expireAfterSeconds` of a hypertable.
pub(crate) fn time_series(
    blueprint: &Blueprint,
    hypertable: &Hypertable,
    notices: &mut Vec<String>,
) -> MongoResult<(Document, Option<i64>)> {
    let collection = blueprint.table();
    let time = hypertable.time_column();
    if let Some(column) = blueprint.columns().iter().find(|c| c.name == time)
        && !matches!(
            column.kind,
            ColumnType::Date
                | ColumnType::DateTime { .. }
                | ColumnType::DateTimeTz { .. }
                | ColumnType::Timestamp { .. }
                | ColumnType::TimestampTz { .. }
        )
    {
        return Err(MongoError::schema(
            collection,
            format!(
                "the time field `{time}` of a time-series collection must be a date column, not {}",
                column.kind.name()
            ),
        ));
    }
    let mut options = doc! { "timeField": time };
    if let Some((column, partitions)) = hypertable.partition() {
        options.insert("metaField", column);
        notices.push(format!(
            "MongoDB groups the measurements of `{collection}` by `{column}` (its metaField); it has no fixed number of partitions ({partitions} on TimescaleDB)"
        ));
    }
    let granularity = match hypertable.chunk_interval_value() {
        None => "hours",
        Some(text) => {
            let interval = Interval::parse(text)
                .map_err(|error| MongoError::schema(collection, error.to_string()))?;
            match interval.as_seconds() {
                Some(seconds) if seconds <= 3_600 => "seconds",
                Some(seconds) if seconds <= 86_400 => "minutes",
                _ => "hours",
            }
        }
    };
    options.insert("granularity", granularity);
    let expire = match hypertable.retention() {
        None => None,
        Some(text) => {
            let interval = Interval::parse(text)
                .map_err(|error| MongoError::schema(collection, error.to_string()))?;
            let seconds = interval.as_seconds().ok_or_else(|| {
                MongoError::schema(
                    collection,
                    format!(
                        "a retention of `{text}`: MongoDB expires time-series data after a number of seconds; write it in days"
                    ),
                )
            })?;
            Some(i64::try_from(seconds).unwrap_or(i64::MAX))
        }
    };
    if hypertable.compression().is_some() {
        notices.push(format!(
            "MongoDB compresses the buckets of the time-series collection `{collection}` on its own: its compression settings are not needed"
        ));
    }
    if hypertable.reorder_index().is_some() {
        notices.push(format!(
            "MongoDB keeps the measurements of `{collection}` in time order on its own: its reorder policy is not needed"
        ));
    }
    Ok((options, expire))
}

/// The commands creating the collection `blueprint` describes.
pub(crate) fn create(blueprint: &Blueprint, extras: &Extras) -> MongoResult<Plan> {
    let mut plan = Plan::default();
    let collection = blueprint.table();
    let comment = blueprint.commands().iter().rev().find_map(|command| match command {
        Command::Comment(comment) => Some(comment.as_str()),
        _ => None,
    });
    let mut create = doc! { "create": collection };
    let hypertable = blueprint.hypertable_definition();
    if let Some(hypertable) = hypertable {
        let (options, expire) = time_series(blueprint, hypertable, &mut plan.notices)?;
        create.insert("timeseries", options);
        if let Some(expire) = expire {
            create.insert("expireAfterSeconds", expire);
        }
        if let Some(key) = key_column(blueprint)
            && key != "id"
        {
            return Err(MongoError::schema(
                collection,
                format!(
                    "the key `{key}` of a time-series collection would need a unique index, which MongoDB time-series collections have not: key it by `id` (stored as `_id`)"
                ),
            ));
        }
    }
    if let Some((size, max)) = extras.capped {
        create.insert("capped", true);
        create.insert("size", i64::try_from(size).unwrap_or(i64::MAX));
        if let Some(max) = max {
            create.insert("max", i64::try_from(max).unwrap_or(i64::MAX));
        }
    }
    if !extras.without_validator
        && let Some(schema) = json_schema(blueprint, comment, &mut plan.notices)?
    {
        if hypertable.is_some() {
            plan.notices.push(time_series_unchecked(collection));
        } else {
            create.insert("validator", doc! { "$jsonSchema": schema });
            if let Some(level) = &extras.validation_level {
                create.insert("validationLevel", level.clone());
            }
            if let Some(action) = &extras.validation_action {
                create.insert("validationAction", action.clone());
            }
        }
    }
    let indexes = indexes(blueprint, extras, hypertable.is_some(), &mut plan.notices)?;
    plan.commands.push(SchemaCommand::database(create));
    if !indexes.is_empty() {
        plan.commands.push(SchemaCommand::database(doc! {
            "createIndexes": collection,
            "indexes": indexes,
        }));
    }
    Ok(plan)
}

/// The notice for a time-series collection's fields, which MongoDB does not
/// check: time-series collections take no validator.
pub(crate) fn time_series_unchecked(collection: &str) -> String {
    format!(
        "MongoDB time-series collections take no validator: the types of `{collection}`'s fields are not checked (the models' casts still are)"
    )
}

/// The `create` command of the view a continuous aggregate described with
/// [`ContinuousAggregate::over`] becomes: a `$group` by the time bucket and
/// the groups, computed when read.
pub(crate) fn view(aggregate: &ContinuousAggregate, notices: &mut Vec<String>) -> MongoResult<SchemaCommand> {
    let name = aggregate.name();
    let AggregateSpec::Described {
        from,
        bucket,
        groups,
        aggregates,
        filter,
    } = aggregate.query()
    else {
        return Err(MongoError::schema(
            name,
            "a continuous aggregate written in SQL cannot run on MongoDB: describe it with `ContinuousAggregate::over(..).time_bucket(..).aggregate(..)`",
        ));
    };
    let mut pipeline = Vec::new();
    if let Some(filter) = filter {
        let json: serde_json::Value = serde_json::from_str(filter).map_err(|_| {
            MongoError::schema(
                name,
                format!(
                    "the filter `{filter}` is SQL: on MongoDB write it as a JSON filter document"
                ),
            )
        })?;
        match crate::convert::json_to_bson(&json) {
            Ok(Bson::Document(filter)) => pipeline.push(doc! { "$match": filter }),
            _ => {
                return Err(MongoError::schema(name, "the filter is not a JSON document"));
            }
        }
    }
    let mut id = Document::new();
    let mut project = doc! { "_id": 0 };
    if let Some((width, column, alias)) = bucket {
        let bucket = crate::compile::time_bucket(width, Bson::String(format!("${column}")), None)
            .map_err(|error| MongoError::schema(name, error.to_string()))?;
        id.insert(alias, bucket);
        project.insert(alias, format!("$_id.{alias}"));
    }
    for group in groups {
        id.insert(group.clone(), format!("${group}"));
        project.insert(group.clone(), format!("$_id.{group}"));
    }
    let mut group = doc! { "_id": if id.is_empty() { Bson::Null } else { Bson::Document(id) } };
    for (expression, alias) in aggregates {
        group.insert(alias.clone(), accumulator(name, expression)?);
        project.insert(alias.clone(), 1);
    }
    pipeline.push(doc! { "$group": group });
    pipeline.push(doc! { "$project": project });
    if let Some((_, _, alias)) = bucket {
        pipeline.push(doc! { "$sort": { alias: 1 } });
    }
    if aggregate.refresh_policy().is_some() {
        notices.push(format!(
            "MongoDB computes the view `{name}` when it is read: its refresh policy is not needed"
        ));
    }
    Ok(SchemaCommand::database(doc! {
        "create": name,
        "viewOn": from,
        "pipeline": pipeline,
    }))
}

/// An aggregate written as SQL (`count(*)`, `sum(amount)`, `max(price)`)
/// as a `$group` accumulator.
fn accumulator(view: &str, expression: &str) -> MongoResult<Bson> {
    let unknown = || {
        MongoError::schema(
            view,
            format!(
                "`{expression}` is not an aggregate MongoDB views compute here (count(*), count(field), sum, avg, min or max of a field)"
            ),
        )
    };
    let text = expression.trim();
    let open = text.find('(').ok_or_else(unknown)?;
    if !text.ends_with(')') {
        return Err(unknown());
    }
    let function = text[..open].trim().to_ascii_lowercase();
    let argument = text[open + 1..text.len() - 1].trim();
    if argument != "*" && !argument.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(unknown());
    }
    let field = format!("${argument}");
    Ok(Bson::Document(match (function.as_str(), argument) {
        ("count", "*") => doc! { "$sum": 1 },
        ("count", _) => doc! { "$sum": { "$cond": [{ "$eq": [{ "$ifNull": [field, Bson::Null] }, Bson::Null] }, 0, 1] } },
        ("sum", a) if a != "*" => doc! { "$sum": field },
        ("avg", a) if a != "*" => doc! { "$avg": field },
        ("min", a) if a != "*" => doc! { "$min": field },
        ("max", a) if a != "*" => doc! { "$max": field },
        _ => return Err(unknown()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(f: impl FnOnce(&mut Blueprint)) -> Plan {
        let mut blueprint = Blueprint::new("deals");
        f(&mut blueprint);
        create(&blueprint, &Extras::default()).unwrap()
    }

    #[test]
    fn columns_become_a_json_schema_validator() {
        let plan = plan(|t| {
            t.id();
            t.string("title").length(120);
            t.decimal("amount", 12, 2);
            t.enumeration("stage", ["lead", "won"]);
            t.timestamp_tz("closed_at").nullable().comment("when it closed");
            t.json("meta").nullable();
            t.unsigned_integer("seats");
        });
        let command = &plan.commands[0].document;
        assert_eq!(command.get_str("create").unwrap(), "deals");
        let schema = command
            .get_document("validator")
            .unwrap()
            .get_document("$jsonSchema")
            .unwrap();
        assert_eq!(
            schema.get_array("required").unwrap(),
            &vec![
                Bson::String("_id".into()),
                Bson::String("title".into()),
                Bson::String("amount".into()),
                Bson::String("stage".into()),
                Bson::String("seats".into()),
            ]
        );
        let properties = schema.get_document("properties").unwrap();
        assert_eq!(
            properties.get_document("_id").unwrap(),
            &doc! { "bsonType": ["int", "long"], "minimum": 0 }
        );
        assert_eq!(
            properties.get_document("title").unwrap(),
            &doc! { "bsonType": "string", "maxLength": 120_i64 }
        );
        assert_eq!(properties.get_document("amount").unwrap(), &doc! { "bsonType": "decimal" });
        assert_eq!(
            properties.get_document("stage").unwrap(),
            &doc! { "bsonType": "string", "enum": ["lead", "won"] }
        );
        assert_eq!(
            properties.get_document("closed_at").unwrap(),
            &doc! { "bsonType": ["date", "null"], "description": "when it closed" }
        );
        assert_eq!(properties.get_document("meta").unwrap(), &doc! {});
        assert_eq!(plan.commands.len(), 1, "no index but _id");
    }

    #[test]
    fn indexes_unique_nullable_text_and_foreign_keys() {
        let plan = plan(|t| {
            t.uuid_primary();
            t.string("email").unique();
            t.string("slug").nullable().unique();
            t.foreign_uuid("company_id").constrained();
            t.text("body");
            t.fulltext(["body"]).language("german");
            t.index(["company_id", "slug"]);
        });
        let indexes = plan.commands[1].document.get_array("indexes").unwrap();
        let names: Vec<&str> = indexes
            .iter()
            .map(|index| index.as_document().unwrap().get_str("name").unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "deals_body_fulltext",
                "deals_company_id_slug_index",
                "deals_email_unique",
                "deals_slug_unique",
            ],
            "the foreign key is covered by the compound index on company_id"
        );
        let slug = indexes[3].as_document().unwrap();
        assert!(slug.get_document("partialFilterExpression").unwrap().contains_key("slug"));
        let text = indexes[0].as_document().unwrap();
        assert_eq!(text.get_document("key").unwrap(), &doc! { "body": "text" });
        assert_eq!(text.get_str("default_language").unwrap(), "german");
        assert!(plan.notices.iter().any(|notice| notice.contains("no foreign keys")), "{:?}", plan.notices);
    }

    #[test]
    fn hypertables_become_time_series_collections() {
        let mut t = Blueprint::new("events");
        t.uuid_primary();
        t.foreign_uuid("workspace_id");
        t.timestamp_tz("occurred_at");
        let events = t.hypertable("occurred_at");
        events.chunk_interval("1 day").partition_by("workspace_id", 4).retain_for("90 days");
        events.compress().segment_by(["workspace_id"]).after("7 days");
        let plan = create(&t, &Extras::default()).unwrap();
        let command = &plan.commands[0].document;
        assert_eq!(
            command.get_document("timeseries").unwrap(),
            &doc! { "timeField": "occurred_at", "metaField": "workspace_id", "granularity": "minutes" }
        );
        assert_eq!(command.get_i64("expireAfterSeconds").unwrap(), 90 * 86_400);
        assert!(plan.notices.iter().any(|notice| notice.contains("compresses")));

        let mut unique = Blueprint::new("events");
        unique.uuid_primary();
        unique.timestamp_tz("at");
        unique.string("code").unique();
        unique.hypertable("at");
        let error = create(&unique, &Extras::default()).unwrap_err();
        assert!(error.to_string().contains("no unique indexes"), "{error}");

        let mut wrong = Blueprint::new("events");
        wrong.string("at");
        wrong.hypertable("at");
        assert!(create(&wrong, &Extras::default()).unwrap_err().to_string().contains("must be a date column"));
    }

    #[test]
    fn mongo_extras_and_refusals() {
        let mut t = Blueprint::new("sessions");
        t.raw_column("owner_id", "objectId");
        t.timestamp_tz("expires_at");
        let extras = Extras {
            expire: vec![("expires_at".into(), 0)],
            validation_level: Some("moderate".into()),
            capped: Some((1_048_576, Some(1000))),
            ..Extras::default()
        };
        let plan = create(&t, &extras).unwrap();
        let command = &plan.commands[0].document;
        assert_eq!(command.get_str("validationLevel").unwrap(), "moderate");
        assert!(command.get_bool("capped").unwrap());
        let owner = command
            .get_document("validator").unwrap()
            .get_document("$jsonSchema").unwrap()
            .get_document("properties").unwrap()
            .get_document("owner_id").unwrap();
        assert_eq!(owner, &doc! { "bsonType": "objectId" });
        let ttl = plan.commands[1].document.get_array("indexes").unwrap()[0].as_document().unwrap().clone();
        assert_eq!(ttl, doc! { "key": { "expires_at": 1 }, "name": "sessions_expires_at_ttl", "expireAfterSeconds": 0_i64 });

        let mut generated = Blueprint::new("totals");
        generated.integer("a");
        generated.integer("b").stored_as("a + 1");
        assert!(create(&generated, &Extras::default()).unwrap_err().to_string().contains("generated column"));
        let mut tsv = Blueprint::new("docs");
        tsv.tsvector("search");
        assert!(create(&tsv, &Extras::default()).is_err());
    }

    #[test]
    fn described_aggregates_become_views() {
        let daily = ContinuousAggregate::over("events_daily", "events")
            .time_bucket("1 day", "occurred_at", "day")
            .group_by("workspace_id")
            .aggregate("count(*)", "total")
            .aggregate("max(amount)", "largest");
        let mut notices = Vec::new();
        let command = view(&daily, &mut notices).unwrap();
        assert_eq!(command.document.get_str("create").unwrap(), "events_daily");
        assert_eq!(command.document.get_str("viewOn").unwrap(), "events");
        let pipeline = command.document.get_array("pipeline").unwrap();
        assert_eq!(pipeline.len(), 3);
        let group = pipeline[0].as_document().unwrap().get_document("$group").unwrap();
        assert_eq!(group.get_document("total").unwrap(), &doc! { "$sum": 1 });
        assert!(group.get_document("_id").unwrap().contains_key("day"));
        let sql = ContinuousAggregate::new("x", "SELECT 1");
        assert!(view(&sql, &mut notices).unwrap_err().to_string().contains("written in SQL"));
        let odd = ContinuousAggregate::over("y", "events").aggregate("percentile(a)", "p");
        assert!(view(&odd, &mut notices).is_err());
    }

    #[test]
    fn commands_render_for_the_shell() {
        let command = SchemaCommand::admin(doc! { "renameCollection": "app.a", "to": "app.b" });
        assert_eq!(
            command.to_string(),
            r#"db.adminCommand({"renameCollection": "app.a", "to": "app.b"})"#
        );
    }
}
