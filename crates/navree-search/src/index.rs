//! Registered indexes: the type-erased operations of each `Searchable`
//! entity, and building their documents.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;

use navree_db::Db;
use navree_kernel::BoxFuture;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DatabaseTransaction, DbErr, IdenStatic, Iterable,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};

use crate::document::index_value;
use crate::searchable::{
    SearchContext, column_is_set, column_string, key_column, key_value, primary_key_string,
};
use crate::{Document, IndexSettings, SearchError, Searchable};

/// Documents by record id: `None` for a record that is gone or kept out of
/// the index.
pub(crate) type Documents = Vec<(String, Option<Document>)>;

/// What `SearchModule::for_feature::<E>()` contributes.
pub(crate) struct IndexRegistration {
    pub(crate) ops: Arc<dyn IndexOps>,
}

impl IndexRegistration {
    pub(crate) fn of<E: Searchable>() -> Self {
        Self {
            ops: Arc::new(EntityIndex::<E>(PhantomData)),
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

/// The operations on one entity's records that the service needs without
/// knowing the entity type.
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
    fn count_rows<'a>(
        &'a self,
        conn: &'a DatabaseConnection,
    ) -> BoxFuture<'a, Result<u64, SearchError>>;

    /// The workspace (column `column`) of each of these records.
    fn workspaces<'a>(
        &'a self,
        tx: &'a DatabaseTransaction,
        column: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<HashMap<String, String>, DbErr>>;

    /// Which of `ids` satisfy `condition` (an access rule's SQL filter).
    fn readable_ids<'a>(
        &'a self,
        db: &'a Db,
        ids: &'a [String],
        condition: SimpleExpr,
    ) -> BoxFuture<'a, Result<HashSet<String>, SearchError>>;
}

struct EntityIndex<E>(PhantomData<fn() -> E>);

impl<E: Searchable> IndexOps for EntityIndex<E> {
    fn entity_type(&self) -> TypeId {
        TypeId::of::<E>()
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<E>()
    }

    fn settings(&self) -> IndexSettings {
        E::settings()
    }

    fn table(&self) -> String {
        sea_orm::EntityName::table_name(&E::default()).to_owned()
    }

    fn documents<'a>(
        &'a self,
        ctx: &'a SearchContext,
        settings: &'a IndexSettings,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<Documents, SearchError>> {
        Box::pin(async move {
            let column = key_column::<E>()?;
            let values: Vec<_> = ids.iter().filter_map(|id| key_value(&column, id)).collect();
            let models = if values.is_empty() {
                Vec::new()
            } else {
                E::find()
                    .filter(column.is_in(values))
                    .all(ctx.conn())
                    .await?
            };
            let mut by_id: HashMap<String, E::Model> = models
                .into_iter()
                .map(|m| (primary_key_string(&m), m))
                .collect();
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                let document = match by_id.remove(id) {
                    Some(model) => build_document::<E>(&model, ctx, settings).await?,
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
            let key = key_column::<E>()?;
            let mut select = E::find().order_by_asc(key);
            if let (Some(workspace), Some(name)) = (workspace, settings.workspace_column()) {
                let column = E::Column::iter()
                    .find(|c| c.as_str() == name)
                    .ok_or_else(|| missing_column::<E>(name))?;
                if let Some(value) = key_value(&column, workspace) {
                    select = select.filter(column.eq(value));
                }
            }
            let models = select.offset(offset).limit(limit).all(ctx.conn()).await?;
            let mut chunk = Chunk {
                rows: models.len() as u64,
                documents: Vec::with_capacity(models.len()),
                skipped: Vec::new(),
            };
            for model in &models {
                match build_document::<E>(model, ctx, settings).await? {
                    Some(document) => chunk.documents.push(document),
                    None => chunk.skipped.push(primary_key_string(model)),
                }
            }
            Ok(chunk)
        })
    }

    fn count_rows<'a>(
        &'a self,
        conn: &'a DatabaseConnection,
    ) -> BoxFuture<'a, Result<u64, SearchError>> {
        Box::pin(async move { Ok(E::find().count(conn).await?) })
    }

    fn workspaces<'a>(
        &'a self,
        tx: &'a DatabaseTransaction,
        column: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<HashMap<String, String>, DbErr>> {
        Box::pin(async move {
            let key = key_column::<E>().map_err(|e| DbErr::Custom(e.to_string()))?;
            let values: Vec<_> = ids.iter().filter_map(|id| key_value(&key, id)).collect();
            if values.is_empty() {
                return Ok(HashMap::new());
            }
            let models = E::find().filter(key.is_in(values)).all(tx).await?;
            Ok(models
                .iter()
                .filter_map(|m| Some((primary_key_string(m), column_string(m, column)?)))
                .collect())
        })
    }

    fn readable_ids<'a>(
        &'a self,
        db: &'a Db,
        ids: &'a [String],
        condition: SimpleExpr,
    ) -> BoxFuture<'a, Result<HashSet<String>, SearchError>> {
        Box::pin(async move {
            let key = key_column::<E>()?;
            let values: Vec<_> = ids.iter().filter_map(|id| key_value(&key, id)).collect();
            if values.is_empty() {
                return Ok(HashSet::new());
            }
            let models = E::find()
                .filter(key.is_in(values))
                .filter(condition)
                .all(&db.reader().await)
                .await?;
            Ok(models.iter().map(primary_key_string).collect())
        })
    }
}

fn missing_column<E: sea_orm::EntityTrait>(column: &str) -> SearchError {
    SearchError::Other(format!(
        "`{}` has no column `{column}`",
        sea_orm::EntityName::table_name(&E::default())
    ))
}

/// The document of `model`: `None` when its soft-delete column is set or
/// [`Searchable::document`] keeps it out; the workspace filled in from the
/// workspace column when the document has none.
pub(crate) async fn build_document<E: Searchable>(
    model: &E::Model,
    ctx: &SearchContext,
    settings: &IndexSettings,
) -> Result<Option<Document>, SearchError> {
    if let Some(column) = settings.soft_delete_column()
        && column_is_set(model, column)
    {
        return Ok(None);
    }
    let document = E::document(model, ctx)
        .await
        .map_err(|source| SearchError::Document {
            index: settings.index().to_owned(),
            id: primary_key_string(model),
            source,
        })?;
    let Some(mut document) = document else {
        return Ok(None);
    };
    if document.workspace_id().is_none()
        && let Some(column) = settings.workspace_column()
    {
        document.set_workspace(column_string(model, column));
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
