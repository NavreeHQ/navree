//! Registered indexes: the type-erased operations of each `Searchable`
//! model, and building their documents.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;

use navree_db::{Db, Uow};
use navree_kernel::BoxFuture;
use navree_orm::{Builder, Filter as RowFilter, Record, col};

use crate::document::index_value;
use crate::searchable::{
    SearchContext, column_is_set, column_string, key_column, key_value, primary_key_string,
    typed_value,
};
use crate::{Document, IndexSettings, SearchError, Searchable};

/// Documents by record id: `None` for a record that is gone or kept out of
/// the index.
pub(crate) type Documents = Vec<(String, Option<Document>)>;

/// What `SearchModule::for_feature::<M>()` contributes.
pub(crate) struct IndexRegistration {
    pub(crate) ops: Arc<dyn IndexOps>,
}

impl IndexRegistration {
    pub(crate) fn of<M: Searchable>() -> Self {
        Self {
            ops: Arc::new(ModelIndex::<M>(PhantomData)),
        }
    }
}

/// One batch of an import.
pub(crate) struct Chunk {
    /// Rows read.
    pub(crate) rows: u64,
    /// Their documents.
    pub(crate) documents: Vec<Document>,
    /// Rows kept out of the index (soft-deleted, or `document` returned
    /// `None`).
    pub(crate) skipped: Vec<String>,
}

/// The operations on one model's records that the service needs without
/// knowing the model type.
pub(crate) trait IndexOps: Send + Sync + 'static {
    fn entity_type(&self) -> TypeId;
    fn type_name(&self) -> &'static str;
    fn settings(&self) -> IndexSettings;
    fn table(&self) -> String;

    /// The documents of the records with these ids; `None` for records
    /// that are gone or kept out of the index.
    fn documents<'a>(
        &'a self,
        ctx: &'a SearchContext,
        settings: &'a IndexSettings,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<Documents, SearchError>>;

    /// Rows `offset..offset + limit` by primary key (of `workspace` only,
    /// when given), as documents.
    fn chunk<'a>(
        &'a self,
        ctx: &'a SearchContext,
        settings: &'a IndexSettings,
        workspace: Option<&'a str>,
        offset: u64,
        limit: u64,
    ) -> BoxFuture<'a, Result<Chunk, SearchError>>;

    /// How many rows the table has.
    fn count_rows<'a>(&'a self, db: &'a Db) -> BoxFuture<'a, Result<u64, SearchError>>;

    /// The workspace (column `column`) of each of these records, read
    /// through `uow`.
    fn workspaces<'a>(
        &'a self,
        uow: &'a Uow,
        column: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<HashMap<String, String>, SearchError>>;

    /// Which of `ids` satisfy `condition` (an access rule's SQL filter).
    fn readable_ids<'a>(
        &'a self,
        db: &'a Db,
        ids: &'a [String],
        condition: RowFilter,
    ) -> BoxFuture<'a, Result<HashSet<String>, SearchError>>;
}

struct ModelIndex<M>(PhantomData<fn() -> M>);

/// Every row of `M`, without its global scopes: soft-deleted rows are read
/// (the index's `soft_delete` column keeps them out), rows of every
/// workspace too (each document names its workspace).
pub(crate) fn unscoped<M: Searchable>() -> Builder<M> {
    M::query().without_global_scopes()
}

/// [`unscoped`] rows whose key is one of `ids` (ids that cannot be keys
/// are left out); `None` when none can.
pub(crate) fn by_ids<M: Searchable>(ids: &[String]) -> Option<Builder<M>> {
    let values: Vec<_> = ids.iter().filter_map(|id| key_value::<M>(id)).collect();
    (!values.is_empty()).then(|| unscoped::<M>().where_(col(key_column::<M>()).is_in(values)))
}

impl<M: Searchable> IndexOps for ModelIndex<M> {
    fn entity_type(&self) -> TypeId {
        TypeId::of::<M>()
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<M>()
    }

    fn settings(&self) -> IndexSettings {
        M::settings()
    }

    fn table(&self) -> String {
        M::meta().table().to_owned()
    }

    fn documents<'a>(
        &'a self,
        ctx: &'a SearchContext,
        settings: &'a IndexSettings,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<Documents, SearchError>> {
        Box::pin(async move {
            let records = match by_ids::<M>(ids) {
                Some(query) => query.get(ctx.uow()).await?.into_vec(),
                None => Vec::new(),
            };
            let mut by_id: HashMap<String, Record<M>> = records
                .into_iter()
                .map(|r| (primary_key_string(r.model()), r))
                .collect();
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                let document = match by_id.remove(id) {
                    Some(record) => build_document::<M>(&record, ctx, settings).await?,
                    None => None,
                };
                out.push((id.clone(), document));
            }
            Ok(out)
        })
    }

    fn chunk<'a>(
        &'a self,
        ctx: &'a SearchContext,
        settings: &'a IndexSettings,
        workspace: Option<&'a str>,
        offset: u64,
        limit: u64,
    ) -> BoxFuture<'a, Result<Chunk, SearchError>> {
        Box::pin(async move {
            let mut query = unscoped::<M>().order_by(col(key_column::<M>()));
            if let (Some(workspace), Some(name)) = (workspace, settings.workspace_column()) {
                let column = M::meta()
                    .column(name)
                    .ok_or_else(|| missing_column::<M>(name))?;
                if let Some(value) = typed_value(column, workspace) {
                    query = query
                        .where_(col(format!("{}.{}", M::meta().table(), column.name())).eq(value));
                }
            }
            let records = query
                .offset(offset)
                .limit(limit)
                .get(ctx.uow())
                .await?
                .into_vec();
            let mut chunk = Chunk {
                rows: records.len() as u64,
                documents: Vec::with_capacity(records.len()),
                skipped: Vec::new(),
            };
            for record in &records {
                match build_document::<M>(record, ctx, settings).await? {
                    Some(document) => chunk.documents.push(document),
                    None => chunk.skipped.push(primary_key_string(record.model())),
                }
            }
            Ok(chunk)
        })
    }

    fn count_rows<'a>(&'a self, db: &'a Db) -> BoxFuture<'a, Result<u64, SearchError>> {
        Box::pin(async move { Ok(unscoped::<M>().count(db).await?) })
    }

    fn workspaces<'a>(
        &'a self,
        uow: &'a Uow,
        column: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<HashMap<String, String>, SearchError>> {
        Box::pin(async move {
            let Some(query) = by_ids::<M>(ids) else {
                return Ok(HashMap::new());
            };
            Ok(query
                .get(uow)
                .await?
                .iter()
                .filter_map(|r| {
                    Some((
                        primary_key_string(r.model()),
                        column_string(r.model(), column)?,
                    ))
                })
                .collect())
        })
    }

    fn readable_ids<'a>(
        &'a self,
        db: &'a Db,
        ids: &'a [String],
        condition: RowFilter,
    ) -> BoxFuture<'a, Result<HashSet<String>, SearchError>> {
        Box::pin(async move {
            let Some(query) = by_ids::<M>(ids) else {
                return Ok(HashSet::new());
            };
            Ok(query
                .where_(condition)
                .get(db)
                .await?
                .iter()
                .map(|r| primary_key_string(r.model()))
                .collect())
        })
    }
}

fn missing_column<M: Searchable>(column: &str) -> SearchError {
    SearchError::Other(format!("`{}` has no column `{column}`", M::meta().table()))
}

/// The document of `record`: `None` when its soft-delete column is set or
/// [`Searchable::document`] keeps it out; the workspace filled in from the
/// workspace column when the document has none.
pub(crate) async fn build_document<M: Searchable>(
    record: &Record<M>,
    ctx: &SearchContext,
    settings: &IndexSettings,
) -> Result<Option<Document>, SearchError> {
    if let Some(column) = settings.soft_delete_column()
        && column_is_set(record.model(), column)
    {
        return Ok(None);
    }
    let document = M::document(record, ctx)
        .await
        .map_err(|source| SearchError::Document {
            index: settings.index().to_owned(),
            id: primary_key_string(record.model()),
            source,
        })?;
    let Some(mut document) = document else {
        return Ok(None);
    };
    if document.workspace_id().is_none()
        && let Some(column) = settings.workspace_column()
    {
        document.set_workspace(column_string(record.model(), column));
    }
    Ok(Some(document))
}

/// The document as engines store it: only the declared fields, filter and
/// sort values normalized (dates as Unix milliseconds).
pub(crate) fn engine_document(mut document: Document, settings: &IndexSettings) -> Document {
    let fields = std::mem::take(document.fields_mut());
    for (name, value) in fields {
        let Some(field) = settings.field(&name) else {
            continue;
        };
        let value = if !field.is_text() && (field.filter || field.sort) {
            index_value(&value)
        } else {
            value
        };
        document.fields_mut().insert(name, value);
    }
    document
}

/// The text the embedding of `document` is made from: the embed fields'
/// text, one per line.
pub(crate) fn embedding_text(document: &Document, settings: &IndexSettings) -> Option<String> {
    let parts: Vec<String> = settings
        .embed_fields()
        .filter_map(|field| crate::text::field_text(document.get(field)?))
        .filter(|text| !text.trim().is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn engine_documents_keep_declared_fields_and_normalize_values() {
        let settings = IndexSettings::new("Deal", "deals")
            .text("title", 3)
            .filter("stage")
            .sort("closes_on")
            .embed("notes");
        let doc = Document::new(1)
            .field("title", "2026-10-26 kickoff")
            .field("stage", "won")
            .field("closes_on", "1970-01-02")
            .field("notes", "renewal risk")
            .field("secret", "not indexed");
        let doc = engine_document(doc, &settings);
        assert_eq!(doc.get("title"), Some(&json!("2026-10-26 kickoff")));
        assert_eq!(doc.get("closes_on"), Some(&json!(86_400_000)));
        assert_eq!(doc.get("secret"), None);
        assert_eq!(
            embedding_text(&doc, &settings).as_deref(),
            Some("renewal risk")
        );
    }
}
