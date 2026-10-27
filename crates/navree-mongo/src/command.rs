//! [`MongoCommand`]: what a query or a write compiles to on MongoDB, as the
//! driver runs it and as `to_mongo()` shows it.

use std::fmt::{self, Write as _};

use bson::{Bson, Document};

/// A MongoDB command: a `find`, an aggregation pipeline, a count, an
/// insert, an update or a delete on one collection.
///
/// `Display` writes it the way MongoDB's shell takes it, with values in
/// relaxed [extended JSON], keys in document order:
///
/// ```
/// use bson::doc;
/// use navree_mongo::{Find, MongoCommand};
///
/// let find = MongoCommand::Find(Find {
///     collection: "deals".into(),
///     filter: doc! { "stage": "won" },
///     sort: Some(doc! { "created_at": -1 }),
///     projection: None,
///     skip: None,
///     limit: Some(10),
/// });
/// assert_eq!(
///     find.to_string(),
///     r#"db.deals.find({"stage": "won"}).sort({"created_at": -1}).limit(10)"#
/// );
/// assert_eq!(find.shape(), r#"db.deals.find({"stage": ?}).sort({"created_at": -1}).limit(?)"#);
/// ```
///
/// The alternate form (`{:#}`) puts each stage of a pipeline on a line of
/// its own:
///
/// ```
/// use bson::doc;
/// use navree_mongo::{Aggregate, MongoCommand};
///
/// let pipeline = MongoCommand::Aggregate(Aggregate {
///     collection: "deals".into(),
///     pipeline: vec![doc! { "$match": { "stage": "won" } }, doc! { "$count": "won" }],
/// });
/// assert_eq!(
///     format!("{pipeline:#}"),
///     "db.deals.aggregate([\n  {\"$match\": {\"stage\": \"won\"}},\n  {\"$count\": \"won\"}\n])"
/// );
/// ```
///
/// [extended JSON]: https://www.mongodb.com/docs/manual/reference/mongodb-extended-json/
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum MongoCommand {
    /// `find`.
    Find(Find),
    /// `aggregate`.
    Aggregate(Aggregate),
    /// `countDocuments`.
    Count(Count),
    /// `insertMany`.
    Insert(Insert),
    /// `insertMany` with `ordered: false`, skipping the documents a unique
    /// index refuses as duplicates.
    InsertOrIgnore(Insert),
    /// `updateMany`.
    Update(Update),
    /// `bulkWrite` of `updateOne`s with `upsert: true`.
    Upsert(Upsert),
    /// `deleteMany`.
    Delete(Delete),
}

/// A `find`: the filter, sort, projection, skip and limit.
#[derive(Clone, Debug, PartialEq)]
pub struct Find {
    /// The collection.
    pub collection: String,
    /// The filter document.
    pub filter: Document,
    /// The sort document.
    pub sort: Option<Document>,
    /// The projection document.
    pub projection: Option<Document>,
    /// Documents to skip.
    pub skip: Option<u64>,
    /// The most documents to return (never 0: a query limited to none is
    /// not sent).
    pub limit: Option<u64>,
}

/// An aggregation pipeline.
#[derive(Clone, Debug, PartialEq)]
pub struct Aggregate {
    /// The collection the pipeline starts from.
    pub collection: String,
    /// The stages.
    pub pipeline: Vec<Document>,
}

/// A `countDocuments`.
#[derive(Clone, Debug, PartialEq)]
pub struct Count {
    /// The collection.
    pub collection: String,
    /// The filter document.
    pub filter: Document,
}

/// An `insertMany`.
#[derive(Clone, Debug, PartialEq)]
pub struct Insert {
    /// The collection.
    pub collection: String,
    /// The documents, in order.
    pub documents: Vec<Document>,
}

/// An `updateMany`: update operators (`$set`, `$inc`, ...) or an update
/// pipeline.
#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    /// The collection.
    pub collection: String,
    /// The filter document.
    pub filter: Document,
    /// The update.
    pub update: UpdateDocument,
}

/// The update half of an [`Update`].
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateDocument {
    /// Update operators: `{"$set": {..}, "$inc": {..}}`.
    Operators(Document),
    /// An update pipeline, for values computed from the document.
    Pipeline(Vec<Document>),
}

/// A `bulkWrite` of upserts: per row, the document matching its filter is
/// updated, or one is inserted from the filter's fields and the update's
/// when none matches.
#[derive(Clone, Debug, PartialEq)]
pub struct Upsert {
    /// The collection.
    pub collection: String,
    /// `(filter, update)` per row, in order.
    pub rows: Vec<(Document, Document)>,
}

/// A `deleteMany`.
#[derive(Clone, Debug, PartialEq)]
pub struct Delete {
    /// The collection.
    pub collection: String,
    /// The filter document.
    pub filter: Document,
}

impl MongoCommand {
    /// The collection the command runs on.
    pub fn collection(&self) -> &str {
        match self {
            MongoCommand::Find(find) => &find.collection,
            MongoCommand::Aggregate(aggregate) => &aggregate.collection,
            MongoCommand::Count(count) => &count.collection,
            MongoCommand::Insert(insert) | MongoCommand::InsertOrIgnore(insert) => &insert.collection,
            MongoCommand::Update(update) => &update.collection,
            MongoCommand::Upsert(upsert) => &upsert.collection,
            MongoCommand::Delete(delete) => &delete.collection,
        }
    }

    /// A read as an aggregation pipeline (a `find` becomes `$match`,
    /// `$sort`, `$skip`, `$limit` and `$project` stages); `None` for a
    /// count or a write.
    pub fn into_aggregate(self) -> Option<Aggregate> {
        match self {
            MongoCommand::Aggregate(aggregate) => Some(aggregate),
            MongoCommand::Find(find) => {
                let mut pipeline = Vec::new();
                if !find.filter.is_empty() {
                    pipeline.push(bson::doc! { "$match": find.filter });
                }
                if let Some(sort) = find.sort {
                    pipeline.push(bson::doc! { "$sort": sort });
                }
                if let Some(skip) = find.skip {
                    pipeline.push(bson::doc! { "$skip": i64::try_from(skip).unwrap_or(i64::MAX) });
                }
                if let Some(limit) = find.limit {
                    pipeline.push(bson::doc! { "$limit": i64::try_from(limit).unwrap_or(i64::MAX) });
                }
                if let Some(projection) = find.projection {
                    pipeline.push(bson::doc! { "$project": projection });
                }
                Some(Aggregate {
                    collection: find.collection,
                    pipeline,
                })
            }
            _ => None,
        }
    }

    /// The command with every value replaced by `?` (keys, operators and
    /// sort directions stay): what telemetry and logs record, so no value
    /// leaves the process.
    pub fn shape(&self) -> String {
        let mut out = String::new();
        self.render(&mut out, Mode::Shape, false);
        out
    }

    /// Writes the command; `pretty` puts each stage of a pipeline and each
    /// inserted document on a line of its own.
    fn render(&self, out: &mut String, mode: Mode, pretty: bool) {
        match self {
            MongoCommand::Find(find) => {
                let _ = write!(out, "db.{}.find(", find.collection);
                document(out, &find.filter, mode);
                if let Some(projection) = &find.projection {
                    out.push_str(", ");
                    document(out, projection, Mode::Plain);
                }
                out.push(')');
                if let Some(sort) = &find.sort {
                    out.push_str(".sort(");
                    document(out, sort, Mode::Plain);
                    out.push(')');
                }
                for (name, value) in [("skip", find.skip), ("limit", find.limit)] {
                    if let Some(value) = value {
                        match mode {
                            Mode::Plain => {
                                let _ = write!(out, ".{name}({value})");
                            }
                            Mode::Shape => {
                                let _ = write!(out, ".{name}(?)");
                            }
                        }
                    }
                }
            }
            MongoCommand::Aggregate(aggregate) => {
                let _ = write!(out, "db.{}.aggregate([", aggregate.collection);
                stages(out, &aggregate.pipeline, mode, pretty);
                out.push_str("])");
            }
            MongoCommand::Count(count) => {
                let _ = write!(out, "db.{}.countDocuments(", count.collection);
                document(out, &count.filter, mode);
                out.push(')');
            }
            MongoCommand::Insert(insert) => {
                let _ = write!(out, "db.{}.insertMany([", insert.collection);
                rows(out, &insert.documents, mode, pretty, |out, doc| document(out, doc, mode));
                out.push_str("])");
            }
            MongoCommand::InsertOrIgnore(insert) => {
                let _ = write!(out, "db.{}.insertMany([", insert.collection);
                rows(out, &insert.documents, mode, pretty, |out, doc| document(out, doc, mode));
                out.push_str("], {\"ordered\": false})");
            }
            MongoCommand::Upsert(upsert) => {
                let _ = write!(out, "db.{}.bulkWrite([", upsert.collection);
                rows(out, &upsert.rows, mode, pretty, |out, (filter, update)| {
                    out.push_str("{\"updateOne\": {\"filter\": ");
                    document(out, filter, mode);
                    out.push_str(", \"update\": ");
                    document(out, update, mode);
                    out.push_str(", \"upsert\": true}}");
                });
                out.push_str("], {\"ordered\": false})");
            }
            MongoCommand::Update(update) => {
                let _ = write!(out, "db.{}.updateMany(", update.collection);
                document(out, &update.filter, mode);
                out.push_str(", ");
                match &update.update {
                    UpdateDocument::Operators(operators) => document(out, operators, mode),
                    UpdateDocument::Pipeline(pipeline) => {
                        out.push('[');
                        stages(out, pipeline, mode, pretty);
                        out.push(']');
                    }
                }
                out.push(')');
            }
            MongoCommand::Delete(delete) => {
                let _ = write!(out, "db.{}.deleteMany(", delete.collection);
                document(out, &delete.filter, mode);
                out.push(')');
            }
        }
    }
}

/// One line; the alternate form (`{:#}`) puts each stage of a pipeline and
/// each inserted document on a line of its own.
impl fmt::Display for MongoCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        self.render(&mut out, Mode::Plain, f.alternate());
        f.write_str(&out)
    }
}

/// How values are written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// As they are.
    Plain,
    /// As `?`.
    Shape,
}

/// The stages of a pipeline: separated by commas, or each on a line of its
/// own, indented, when `pretty`. In shape mode every stage shows (they are
/// structure).
fn stages(out: &mut String, pipeline: &[Document], mode: Mode, pretty: bool) {
    list(out, pipeline, pretty, |out, stage| document(out, stage, mode));
}

/// The rows of a write: like [`stages`], but in shape mode only the first,
/// then `...` (the rest only repeat its shape with other values).
fn rows<T>(out: &mut String, rows: &[T], mode: Mode, pretty: bool, item: impl Fn(&mut String, &T)) {
    match mode {
        Mode::Plain => list(out, rows, pretty, item),
        Mode::Shape => {
            if let Some(first) = rows.first() {
                item(out, first);
                if rows.len() > 1 {
                    out.push_str(", ...");
                }
            }
        }
    }
}

/// Items separated by commas, or each on a line of its own, indented, when
/// `pretty`.
fn list<T>(out: &mut String, items: &[T], pretty: bool, item: impl Fn(&mut String, &T)) {
    for (i, value) in items.iter().enumerate() {
        match (pretty, i) {
            (true, 0) => out.push_str("\n  "),
            (true, _) => out.push_str(",\n  "),
            (false, 0) => {}
            (false, _) => out.push_str(", "),
        }
        item(out, value);
    }
    if pretty && !items.is_empty() {
        out.push('\n');
    }
}

/// `document` as relaxed extended JSON, keys in document order. In shape
/// mode scalar values become `?`, except where they say how rather than
/// what: sort directions and `$project` inclusions (`1`, `-1`, `0`), field
/// paths (`"$amount"`) and the options of operators.
pub(crate) fn render_document(document: &Document) -> String {
    let mut out = String::new();
    self::document(&mut out, document, Mode::Plain);
    out
}

fn document(out: &mut String, document: &Document, mode: Mode) {
    out.push('{');
    for (i, (key, value)) in document.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        string(out, key);
        out.push_str(": ");
        let mode = match (mode, key.as_str()) {
            // Structure, not data: directions, inclusions, collection and
            // field names of stages.
            (Mode::Shape, "$sort" | "$project" | "$unset" | "from" | "as" | "localField"
            | "foreignField" | "coll" | "unit" | "format") => Mode::Plain,
            (mode, _) => mode,
        };
        bson(out, value, mode);
    }
    out.push('}');
}

fn bson(out: &mut String, value: &Bson, mode: Mode) {
    match value {
        Bson::Document(inner) => document(out, inner, mode),
        Bson::Array(items) => {
            out.push('[');
            if mode == Mode::Shape && items.iter().all(|item| !is_structure(item)) {
                if !items.is_empty() {
                    out.push('?');
                }
            } else {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    bson(out, item, mode);
                }
            }
            out.push(']');
        }
        // A field path or variable says where, not what.
        Bson::String(text) if mode == Mode::Shape && text.starts_with('$') => string(out, text),
        _ if mode == Mode::Shape => out.push('?'),
        Bson::String(text) => string(out, text),
        other => json(out, &other.clone().into_relaxed_extjson()),
    }
}

/// A JSON value with the same spacing as documents, object keys sorted
/// (extended JSON wrappers: `{"$date": ..}`, `{"$oid": ..}`).
fn json(out: &mut String, value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (key, value)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                string(out, key);
                out.push_str(": ");
                json(out, value);
            }
            out.push('}');
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                json(out, item);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Whether `value` holds structure (documents, field paths) rather than
/// only data.
fn is_structure(value: &Bson) -> bool {
    match value {
        Bson::Document(_) => true,
        Bson::Array(items) => items.iter().any(is_structure),
        Bson::String(text) => text.starts_with('$'),
        _ => false,
    }
}

fn string(out: &mut String, text: &str) {
    out.push_str(&serde_json::to_string(text).unwrap_or_else(|_| format!("\"{text}\"")));
}

#[cfg(test)]
mod tests {
    use bson::doc;

    use super::*;

    #[test]
    fn pipelines_render_in_document_order() {
        let aggregate = MongoCommand::Aggregate(Aggregate {
            collection: "events".into(),
            pipeline: vec![
                doc! { "$match": { "kind": { "$in": ["a", "b"] }, "at": { "$gte": bson::DateTime::from_millis(0) } } },
                doc! { "$group": { "_id": "$kind", "total": { "$sum": 1 } } },
                doc! { "$sort": { "total": -1, "_id": 1 } },
            ],
        });
        assert_eq!(
            aggregate.to_string(),
            r#"db.events.aggregate([{"$match": {"kind": {"$in": ["a", "b"]}, "at": {"$gte": {"$date": "1970-01-01T00:00:00Z"}}}}, {"$group": {"_id": "$kind", "total": {"$sum": 1}}}, {"$sort": {"total": -1, "_id": 1}}])"#
        );
        assert_eq!(
            aggregate.shape(),
            r#"db.events.aggregate([{"$match": {"kind": {"$in": [?]}, "at": {"$gte": ?}}}, {"$group": {"_id": "$kind", "total": {"$sum": ?}}}, {"$sort": {"total": -1, "_id": 1}}])"#
        );
    }

    #[test]
    fn writes_render_and_shape() {
        let insert = MongoCommand::Insert(Insert {
            collection: "deals".into(),
            documents: vec![doc! { "title": "A" }, doc! { "title": "B" }],
        });
        assert_eq!(
            insert.to_string(),
            r#"db.deals.insertMany([{"title": "A"}, {"title": "B"}])"#
        );
        assert_eq!(insert.shape(), r#"db.deals.insertMany([{"title": ?}, ...])"#);
        let update = MongoCommand::Update(Update {
            collection: "deals".into(),
            filter: doc! { "_id": 7 },
            update: UpdateDocument::Operators(doc! { "$set": { "title": "C" }, "$inc": { "n": 1 } }),
        });
        assert_eq!(
            update.to_string(),
            r#"db.deals.updateMany({"_id": 7}, {"$set": {"title": "C"}, "$inc": {"n": 1}})"#
        );
        assert_eq!(
            update.shape(),
            r#"db.deals.updateMany({"_id": ?}, {"$set": {"title": ?}, "$inc": {"n": ?}})"#
        );
        assert_eq!(update.collection(), "deals");
    }

    #[test]
    fn the_alternate_form_puts_each_stage_on_its_own_line() {
        let aggregate = MongoCommand::Aggregate(Aggregate {
            collection: "events".into(),
            pipeline: vec![
                doc! { "$match": { "kind": "a" } },
                doc! { "$count": "n" },
            ],
        });
        assert_eq!(
            format!("{aggregate:#}"),
            "db.events.aggregate([\n  {\"$match\": {\"kind\": \"a\"}},\n  {\"$count\": \"n\"}\n])"
        );
        let insert = MongoCommand::Insert(Insert {
            collection: "deals".into(),
            documents: vec![doc! { "title": "A" }, doc! { "title": "B" }],
        });
        assert_eq!(
            format!("{insert:#}"),
            "db.deals.insertMany([\n  {\"title\": \"A\"},\n  {\"title\": \"B\"}\n])"
        );
        let update = MongoCommand::Update(Update {
            collection: "deals".into(),
            filter: doc! {},
            update: UpdateDocument::Pipeline(vec![doc! { "$set": { "n": 1 } }]),
        });
        assert_eq!(
            format!("{update:#}"),
            "db.deals.updateMany({}, [\n  {\"$set\": {\"n\": 1}}\n])"
        );
        let empty = MongoCommand::Aggregate(Aggregate {
            collection: "events".into(),
            pipeline: Vec::new(),
        });
        assert_eq!(format!("{empty:#}"), "db.events.aggregate([])");
        let find = MongoCommand::Find(Find {
            collection: "deals".into(),
            filter: doc! { "a": 1 },
            sort: None,
            projection: None,
            skip: None,
            limit: None,
        });
        assert_eq!(format!("{find:#}"), find.to_string(), "a find is one line");
    }

    #[test]
    fn bulk_writes_render_as_the_shell_takes_them() {
        let ignore = MongoCommand::InsertOrIgnore(Insert {
            collection: "tags".into(),
            documents: vec![doc! { "name": "vip" }, doc! { "name": "new" }],
        });
        assert_eq!(
            ignore.to_string(),
            r#"db.tags.insertMany([{"name": "vip"}, {"name": "new"}], {"ordered": false})"#
        );
        assert_eq!(
            ignore.shape(),
            r#"db.tags.insertMany([{"name": ?}, ...], {"ordered": false})"#
        );
        let upsert = MongoCommand::Upsert(Upsert {
            collection: "rates".into(),
            rows: vec![
                (doc! { "code": "EUR" }, doc! { "$set": { "rate": 1.1 } }),
                (doc! { "code": "NOK" }, doc! { "$set": { "rate": 0.09 } }),
            ],
        });
        assert_eq!(
            upsert.to_string(),
            r#"db.rates.bulkWrite([{"updateOne": {"filter": {"code": "EUR"}, "update": {"$set": {"rate": 1.1}}, "upsert": true}}, {"updateOne": {"filter": {"code": "NOK"}, "update": {"$set": {"rate": 0.09}}, "upsert": true}}], {"ordered": false})"#
        );
        assert_eq!(
            upsert.shape(),
            r#"db.rates.bulkWrite([{"updateOne": {"filter": {"code": ?}, "update": {"$set": {"rate": ?}}, "upsert": true}}, ...], {"ordered": false})"#
        );
        assert_eq!(
            format!("{upsert:#}"),
            "db.rates.bulkWrite([\n  {\"updateOne\": {\"filter\": {\"code\": \"EUR\"}, \"update\": {\"$set\": {\"rate\": 1.1}}, \"upsert\": true}},\n  {\"updateOne\": {\"filter\": {\"code\": \"NOK\"}, \"update\": {\"$set\": {\"rate\": 0.09}}, \"upsert\": true}}\n], {\"ordered\": false})"
        );
        assert_eq!(upsert.collection(), "rates");
        assert!(upsert.into_aggregate().is_none());
    }
}
