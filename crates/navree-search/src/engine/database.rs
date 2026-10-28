//! The `postgres` engine: full-text search in the app's database.
//!
//! On PostgreSQL, documents live in `search_documents` with a weighted
//! `tsvector` (GIN index); with `trigram = true` a `pg_trgm` index on the
//! text adds typo tolerance. On SQLite (development and tests) the same
//! engine uses an FTS5 table, `search_documents_fts`, with one column per
//! boost and BM25 ranking; prefixes match, typos do not.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_db::{Db, Reader};
use navree_kernel::BoxFuture;
use sea_orm::{
    ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, QueryResult, Statement, TransactionTrait,
};
use serde_json::Value;

use crate::engine::{EngineHit, EngineQuery, EngineResults, SearchEngine};
use crate::migration::{DOCUMENTS_TABLE, FTS_TABLE};
use crate::options::PostgresOptions;
use crate::settings::WORKSPACE_FIELD;
use crate::sql::Sql;
use crate::text::{QueryTerms, Term, field_text, highlight};
use crate::{Document, IndexSettings, SearchError};

/// Full-text search in the app's database (see the module docs).
pub struct DatabaseEngine {
    db: Db,
    options: PostgresOptions,
    trigram_ready: AtomicBool,
}

impl DatabaseEngine {
    /// The engine on `db`.
    pub fn new(db: Db, options: PostgresOptions) -> Self {
        Self {
            db,
            options,
            trigram_ready: AtomicBool::new(false),
        }
    }

    fn backend(&self) -> DbBackend {
        self.db.backend()
    }

    fn sql(&self) -> Sql {
        Sql::new(self.backend())
    }

    /// Runs `f` in the current unit of work's open transaction when there
    /// is one on this database (so a request that already wrote does not
    /// wait for its own write lock), else in a new transaction.
    async fn write<F>(&self, f: F) -> Result<(), SearchError>
    where
        F: for<'c> FnOnce(&'c DatabaseTransaction) -> BoxFuture<'c, Result<(), DbErr>>,
    {
        if let Reader::Transaction(tx) = self.db.reader().await {
            return Ok(f(&tx).await?);
        }
        let tx = self.db.conn().begin().await?;
        f(&tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn query_all(&self, sql: Sql) -> Result<Vec<QueryResult>, SearchError> {
        Ok(self
            .db
            .reader()
            .await
            .query_all_raw(sql.statement())
            .await?)
    }

    async fn query_count(&self, sql: Sql) -> Result<u64, SearchError> {
        let row = self
            .db
            .reader()
            .await
            .query_one_raw(sql.statement())
            .await?;
        let n: i64 = match row {
            Some(row) => row.try_get_by_index(0)?,
            None => 0,
        };
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Runs the count and the page of a trigram query with
    /// `pg_trgm.word_similarity_threshold` set to the configured typo
    /// threshold for just these statements: `<%` compares against that
    /// setting, whose default of 0.6 misses most one-letter typos.
    ///
    /// On the pool the statements run in a transaction of their own
    /// (`SET LOCAL` needs one); inside the unit of work's transaction the
    /// previous value is put back afterwards.
    async fn query_with_threshold(
        &self,
        count: Sql,
        select: Option<Sql>,
    ) -> Result<(u64, Vec<QueryResult>), SearchError> {
        const SETTING: &str = "pg_trgm.word_similarity_threshold";
        let set = |value: String| {
            let mut sql = self.sql();
            sql.push(&format!("SELECT set_config('{SETTING}', "))
                .bind(value)
                .push(", true)");
            sql.statement()
        };
        let threshold = self.options.typo_threshold.to_string();
        match self.db.reader().await {
            Reader::Pool(pool) => {
                let tx = pool.begin().await?;
                tx.execute_raw(set(threshold)).await?;
                let out = count_and_page(&tx, count, select).await?;
                tx.commit().await?;
                Ok(out)
            }
            Reader::Transaction(tx) => {
                let previous = tx
                    .query_one_raw(Statement::from_string(
                        DbBackend::Postgres,
                        format!("SELECT current_setting('{SETTING}', true)"),
                    ))
                    .await?
                    .and_then(|row| row.try_get_by_index::<Option<String>>(0).ok().flatten())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "0.6".to_owned());
                tx.execute_raw(set(threshold)).await?;
                let out = count_and_page(&*tx, count, select).await;
                tx.execute_raw(set(previous)).await?;
                Ok(out?)
            }
        }
    }

    /// Enables `pg_trgm` and indexes the text with it, once.
    async fn ensure_trigram(&self) -> Result<(), SearchError> {
        if !self.options.trigram
            || self.backend() != DbBackend::Postgres
            || self.trigram_ready.load(Ordering::Relaxed)
        {
            return Ok(());
        }
        let conn = self.db.conn();
        conn.execute_unprepared("CREATE EXTENSION IF NOT EXISTS pg_trgm")
            .await?;
        conn.execute_unprepared(&format!(
            "CREATE INDEX IF NOT EXISTS {DOCUMENTS_TABLE}_content_trgm ON {DOCUMENTS_TABLE} USING GIN (content gin_trgm_ops)"
        ))
        .await?;
        self.trigram_ready.store(true, Ordering::Relaxed);
        Ok(())
    }
}

impl std::fmt::Debug for DatabaseEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseEngine")
            .field("backend", &self.backend())
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// The total of `count` and the rows of `select` (when there is one), on
/// `conn`.
async fn count_and_page<C: ConnectionTrait>(
    conn: &C,
    count: Sql,
    select: Option<Sql>,
) -> Result<(u64, Vec<QueryResult>), DbErr> {
    let total = match conn.query_one_raw(count.statement()).await? {
        Some(row) => u64::try_from(row.try_get_by_index::<i64>(0)?).unwrap_or(0),
        None => 0,
    };
    let rows = match select {
        Some(select) => conn.query_all_raw(select.statement()).await?,
        None => Vec::new(),
    };
    Ok((total, rows))
}

/// The PostgreSQL weight class of a boost: 5 and up `A`, 3 and 4 `B`, 2
/// `C`, 1 `D` (ranked with weights 1.0, 0.4, 0.2 and 0.1).
pub(crate) fn weight_class(boost: u8) -> char {
    match boost {
        5.. => 'A',
        3..=4 => 'B',
        2 => 'C',
        _ => 'D',
    }
}

/// The text of each boost (1 to 10) of `document`.
fn texts_by_boost(document: &Document, settings: &IndexSettings) -> [String; 10] {
    let mut out: [String; 10] = Default::default();
    for (field, boost) in settings.text_fields() {
        if let Some(text) = document.get(field).and_then(field_text) {
            let slot = &mut out[usize::from(boost.clamp(1, 10)) - 1];
            if !slot.is_empty() {
                slot.push(' ');
            }
            slot.push_str(&text);
        }
    }
    out
}

/// The fields JSON stored for `document`, with `_workspace`.
fn stored_fields(document: &Document) -> String {
    let mut fields = document.fields().clone();
    if let Some(workspace) = document.workspace_id() {
        fields.insert(WORKSPACE_FIELD.to_owned(), Value::from(workspace));
    }
    serde_json::to_string(&fields).unwrap_or_else(|_| "{}".to_owned())
}

/// An FTS5 `MATCH` expression: every word (quoted; the last as a prefix
/// unless followed by a space), or one of its synonyms.
pub(crate) fn fts5_query(terms: &QueryTerms) -> String {
    terms
        .terms
        .iter()
        .map(|term| {
            let mut alternatives = vec![if term.prefix {
                format!("\"{}\"*", term.word)
            } else {
                format!("\"{}\"", term.word)
            }];
            for synonym in &term.synonyms {
                alternatives.push(format!("\"{}\"", synonym.join(" ")));
            }
            if alternatives.len() == 1 {
                alternatives.remove(0)
            } else {
                format!("({})", alternatives.join(" OR "))
            }
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// A `to_tsquery` text: words joined with `&`, the last as a prefix
/// (`:*`), synonyms as alternatives (`|`, phrases with `<->`).
pub(crate) fn tsquery(terms: &QueryTerms) -> String {
    terms
        .terms
        .iter()
        .map(term_tsquery)
        .collect::<Vec<_>>()
        .join(" & ")
}

/// [`tsquery`] with the words joined with `|`: what ranking uses when a
/// word may match by trigrams alone.
pub(crate) fn tsquery_any(terms: &QueryTerms) -> String {
    terms
        .terms
        .iter()
        .map(term_tsquery)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The `to_tsquery` text of one word: the word (a prefix when it is the
/// last one), or one of its synonyms.
fn term_tsquery(term: &Term) -> String {
    let mut alternatives = vec![if term.prefix {
        format!("{}:*", term.word)
    } else {
        term.word.clone()
    }];
    for synonym in &term.synonyms {
        alternatives.push(synonym.join(" <-> "));
    }
    if alternatives.len() == 1 {
        alternatives.remove(0)
    } else {
        format!("({})", alternatives.join(" | "))
    }
}

impl SearchEngine for DatabaseEngine {
    fn name(&self) -> &'static str {
        "postgres"
    }

    fn prepare<'a>(
        &'a self,
        _index: &'a str,
        _settings: &'a IndexSettings,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        // The `search` migrations create the tables.
        Box::pin(async move { self.ensure_trigram().await })
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
        documents: Vec<Document>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let backend = self.backend();
            let language = self.options.language.clone();
            let mut statements = Vec::new();
            for document in &documents {
                let texts = texts_by_boost(document, settings);
                let fields = stored_fields(document);
                if backend == DbBackend::Postgres {
                    let mut sql = Sql::new(backend);
                    sql.push(&format!(
                        "INSERT INTO {DOCUMENTS_TABLE} (index_name, id, fields, content, search_vector) VALUES ("
                    ));
                    sql.bind(index).push(", ").bind(document.id()).push(", ");
                    sql.bind(fields).push("::jsonb, ");
                    sql.bind(texts.join(" ").trim().to_owned()).push(", ");
                    let mut first = true;
                    for (i, text) in texts.iter().enumerate() {
                        if text.is_empty() {
                            continue;
                        }
                        if !first {
                            sql.push(" || ");
                        }
                        first = false;
                        let class = weight_class(u8::try_from(i + 1).unwrap_or(10));
                        sql.push("setweight(to_tsvector(");
                        sql.bind(language.clone()).push("::regconfig, ");
                        sql.bind(text.clone()).push(&format!("), '{class}')"));
                    }
                    if first {
                        sql.push("''::tsvector");
                    }
                    sql.push(
                        ") ON CONFLICT (index_name, id) DO UPDATE SET fields = EXCLUDED.fields, \
                         content = EXCLUDED.content, search_vector = EXCLUDED.search_vector",
                    );
                    statements.push(sql.statement());
                } else {
                    // The FTS row shares its document's rowid, so it is
                    // found through the documents' primary key (an index)
                    // and goes first, while the document still has a rowid.
                    let mut delete_fts = Sql::new(backend);
                    delete_fts
                        .push(&format!(
                            "DELETE FROM {FTS_TABLE} WHERE rowid IN (SELECT rowid FROM {DOCUMENTS_TABLE} WHERE index_name = "
                        ))
                        .bind(index)
                        .push(" AND id = ")
                        .bind(document.id())
                        .push(")");
                    statements.push(delete_fts.statement());
                    let mut delete = Sql::new(backend);
                    delete
                        .push(&format!(
                            "DELETE FROM {DOCUMENTS_TABLE} WHERE index_name = "
                        ))
                        .bind(index)
                        .push(" AND id = ")
                        .bind(document.id());
                    statements.push(delete.statement());
                    let mut insert = Sql::new(backend);
                    insert
                        .push(&format!(
                            "INSERT INTO {DOCUMENTS_TABLE} (index_name, id, fields) VALUES ("
                        ))
                        .bind(index)
                        .push(", ")
                        .bind(document.id())
                        .push(", ")
                        .bind(fields)
                        .push(")");
                    statements.push(insert.statement());
                    let mut fts = Sql::new(backend);
                    fts.push(&format!(
                        "INSERT INTO {FTS_TABLE} (rowid, index_name, id, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10) VALUES ((SELECT rowid FROM {DOCUMENTS_TABLE} WHERE index_name = "
                    ))
                    .bind(index)
                    .push(" AND id = ")
                    .bind(document.id())
                    .push("), ")
                    .bind(index)
                    .push(", ")
                    .bind(document.id());
                    for text in texts {
                        fts.push(", ").bind(text);
                    }
                    fts.push(")");
                    statements.push(fts.statement());
                }
            }
            self.write(move |tx| {
                Box::pin(async move {
                    for statement in statements {
                        tx.execute_raw(statement).await?;
                    }
                    Ok(())
                })
            })
            .await
        })
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let backend = self.backend();
            let mut statements = Vec::new();
            for chunk in ids.chunks(500) {
                statements.extend(delete_statements(backend, index, Some(chunk)));
            }
            self.write(move |tx| {
                Box::pin(async move {
                    for statement in statements {
                        tx.execute_raw(statement).await?;
                    }
                    Ok(())
                })
            })
            .await
        })
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let backend = self.backend();
            let statements = delete_statements(backend, index, None);
            self.write(move |tx| {
                Box::pin(async move {
                    for statement in statements {
                        tx.execute_raw(statement).await?;
                    }
                    Ok(())
                })
            })
            .await
        })
    }

    fn drop_index<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        self.flush(index)
    }

    fn swap<'a>(
        &'a self,
        index: &'a str,
        staged: &'a str,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let backend = self.backend();
            let mut statements = delete_statements(backend, index, None);
            // On SQLite the FTS rows are renamed first: they are found
            // through the documents that still carry the staged name.
            for table in tables(backend) {
                let mut rename = Sql::new(backend);
                rename
                    .push(&format!("UPDATE {table} SET index_name = "))
                    .bind(index)
                    .push(" WHERE ");
                scope(&mut rename, table, staged, None);
                statements.push(rename.statement());
            }
            self.write(move |tx| {
                Box::pin(async move {
                    for statement in statements {
                        tx.execute_raw(statement).await?;
                    }
                    Ok(())
                })
            })
            .await
        })
    }

    fn search<'a>(
        &'a self,
        index: &'a str,
        query: EngineQuery<'a>,
    ) -> BoxFuture<'a, Result<EngineResults, SearchError>> {
        Box::pin(async move {
            // The trigram index exists before the first typo-tolerant query,
            // even when the app never ran `prepare` (it is a cheap check
            // after the first time).
            self.ensure_trigram().await?;
            let settings = query.settings;
            let terms = QueryTerms::parse(
                query.text,
                settings.stop_word_list(),
                settings.synonym_map(),
            );
            let postgres = self.backend() == DbBackend::Postgres;
            let (mut select, mut count) = (self.sql(), self.sql());
            select.push("SELECT d.id, d.fields, ");
            count.push("SELECT COUNT(*) ");
            // The rank column, then FROM and the text condition.
            build_from(
                &mut select,
                &mut count,
                index,
                &terms,
                postgres,
                &self.options,
                query.text,
            );
            for sql in [&mut select, &mut count] {
                sql.push(" AND ");
                sql.filter(query.filter, "d.fields");
            }
            select.push(" ORDER BY ");
            select.order(query.sort, "d.fields");
            select.push("rank DESC, d.id ASC");
            select
                .push(" LIMIT ")
                .bind(i64::try_from(query.limit).unwrap_or(i64::MAX));
            select
                .push(" OFFSET ")
                .bind(i64::try_from(query.offset).unwrap_or(i64::MAX));
            let select = (query.limit > 0).then_some(select);
            let (total, rows) = if postgres && self.options.trigram && !terms.is_empty() {
                self.query_with_threshold(count, select).await?
            } else {
                let total = self.query_count(count).await?;
                let rows = match select {
                    Some(select) => self.query_all(select).await?,
                    None => Vec::new(),
                };
                (total, rows)
            };
            let mut hits = Vec::with_capacity(rows.len());
            for row in rows {
                let id: String = row.try_get("", "id")?;
                let fields = fields_of(&row, postgres)?;
                let rank: f64 = row.try_get("", "rank").unwrap_or(0.0);
                let highlights = match query.highlight {
                    Some(tags) if !terms.is_empty() => settings
                        .text_fields()
                        .filter_map(|(field, _)| {
                            let text = field_text(fields.get(field)?)?;
                            highlight(&text, &terms, tags).map(|h| (field.to_owned(), h))
                        })
                        .collect(),
                    _ => BTreeMap::new(),
                };
                let title = query
                    .with_title
                    .then(|| settings.title_field())
                    .flatten()
                    .and_then(|field| field_text(fields.get(field)?));
                hits.push(EngineHit {
                    id,
                    score: rank.clamp(0.0, 1.0),
                    highlights,
                    title,
                });
            }
            Ok(EngineResults { hits, total })
        })
    }

    fn count<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<u64, SearchError>> {
        Box::pin(async move {
            let mut sql = self.sql();
            sql.push(&format!(
                "SELECT COUNT(*) FROM {DOCUMENTS_TABLE} WHERE index_name = "
            ))
            .bind(index);
            self.query_count(sql).await
        })
    }

    fn contains<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<bool, SearchError>> {
        Box::pin(async move {
            let mut sql = self.sql();
            sql.push(&format!(
                "SELECT COUNT(*) FROM {DOCUMENTS_TABLE} WHERE index_name = "
            ))
            .bind(index)
            .push(" AND id = ")
            .bind(id);
            Ok(self.query_count(sql).await? > 0)
        })
    }
}

/// The tables holding an index's documents, the FTS table first: its rows
/// are found through their documents, so it is changed before them.
fn tables(backend: DbBackend) -> Vec<&'static str> {
    if backend == DbBackend::Postgres {
        vec![DOCUMENTS_TABLE]
    } else {
        vec![FTS_TABLE, DOCUMENTS_TABLE]
    }
}

/// Appends the condition selecting an index's rows in `table` (those of
/// `ids` when given). An FTS row shares the rowid of its document, so it is
/// selected through the documents' primary key; filtering the FTS table's
/// own `UNINDEXED` columns would scan all of it for every document.
fn scope(sql: &mut Sql, table: &str, index: &str, ids: Option<&[String]>) {
    let through_documents = table == FTS_TABLE;
    if through_documents {
        sql.push(&format!(
            "rowid IN (SELECT rowid FROM {DOCUMENTS_TABLE} WHERE "
        ));
    }
    sql.push("index_name = ").bind(index);
    if let Some(ids) = ids {
        sql.push(" AND id IN (");
        for (i, id) in ids.iter().enumerate() {
            if i > 0 {
                sql.push(", ");
            }
            sql.bind(id.clone());
        }
        sql.push(")");
    }
    if through_documents {
        sql.push(")");
    }
}

/// The statements deleting an index's documents (or just `ids`) from every
/// table.
fn delete_statements(
    backend: DbBackend,
    index: &str,
    ids: Option<&[String]>,
) -> Vec<Statement> {
    tables(backend)
        .into_iter()
        .map(|table| {
            let mut sql = Sql::new(backend);
            sql.push(&format!("DELETE FROM {table} WHERE "));
            scope(&mut sql, table, index, ids);
            sql.statement()
        })
        .collect()
}

/// Appends `<rank> AS rank FROM ... WHERE <text condition>` to `select`
/// and `FROM ... WHERE <text condition>` to `count`.
fn build_from(
    select: &mut Sql,
    count: &mut Sql,
    index: &str,
    terms: &QueryTerms,
    postgres: bool,
    options: &PostgresOptions,
    text: &str,
) {
    if terms.is_empty() {
        let rank = if postgres { "0.0::float8" } else { "0.0" };
        select.push(&format!(
            "{rank} AS rank FROM {DOCUMENTS_TABLE} d WHERE d.index_name = "
        ));
        select.bind(index);
        count.push(&format!("FROM {DOCUMENTS_TABLE} d WHERE d.index_name = "));
        count.bind(index);
        return;
    }
    if postgres {
        postgres_from(select, count, index, terms, options, text);
        return;
    }
    let matching = fts5_query(terms);
    // bm25() is negative, more negative is better: `-r / (1 - r)` maps it
    // into 0..1.
    select.push(&format!(
        "(-bm25({FTS_TABLE}, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10)) / (1.0 - bm25({FTS_TABLE}, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10)) AS rank "
    ));
    for sql in [select, count] {
        sql.push(&format!(
            "FROM {FTS_TABLE} JOIN {DOCUMENTS_TABLE} d ON d.rowid = {FTS_TABLE}.rowid \
             WHERE {FTS_TABLE} MATCH "
        ));
        sql.bind(matching.clone());
        sql.push(" AND d.index_name = ");
        sql.bind(index);
    }
}

/// [`build_from`] on PostgreSQL. Every word must match the `tsvector`
/// (one `to_tsquery` with the words joined by `&`). With `trigram`, each
/// word may instead match the text by trigrams (`word <% content`), so a
/// misspelled word still finds its record while the other words keep
/// narrowing the results; the rank then adds the query's
/// `word_similarity` to the `ts_rank` of any of the words. The rank is a
/// `float8`.
fn postgres_from(
    select: &mut Sql,
    count: &mut Sql,
    index: &str,
    terms: &QueryTerms,
    options: &PostgresOptions,
    text: &str,
) {
    let language = options.language.clone();
    let rank_query = if options.trigram {
        tsquery_any(terms)
    } else {
        tsquery(terms)
    };
    select.push("(ts_rank('{0.1, 0.2, 0.4, 1.0}', d.search_vector, to_tsquery(");
    select
        .bind(language.clone())
        .push("::regconfig, ")
        .bind(rank_query);
    select.push("), 32)");
    if options.trigram {
        select
            .push(" + 0.5 * word_similarity(")
            .bind(text.to_owned());
        select.push(", d.content)");
    }
    select.push(")::float8 AS rank ");
    for sql in [select, count] {
        sql.push(&format!("FROM {DOCUMENTS_TABLE} d WHERE d.index_name = "));
        sql.bind(index);
        if !options.trigram {
            sql.push(" AND d.search_vector @@ to_tsquery(");
            sql.bind(language.clone())
                .push("::regconfig, ")
                .bind(tsquery(terms));
            sql.push(")");
            continue;
        }
        for term in &terms.terms {
            sql.push(" AND (d.search_vector @@ to_tsquery(");
            sql.bind(language.clone())
                .push("::regconfig, ")
                .bind(term_tsquery(term));
            sql.push(") OR ")
                .bind(term.word.clone())
                .push(" <% d.content)");
        }
    }
}

/// The `fields` column of a result row.
fn fields_of(row: &QueryResult, postgres: bool) -> Result<BTreeMap<String, Value>, SearchError> {
    let value: Value = if postgres {
        row.try_get("", "fields")?
    } else {
        let text: String = row.try_get("", "fields")?;
        serde_json::from_str(&text).unwrap_or(Value::Null)
    };
    Ok(match value {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(text: &str) -> QueryTerms {
        let synonyms = BTreeMap::from([("acme".to_owned(), vec!["acme corp".to_owned()])]);
        QueryTerms::parse(text, &[], &synonyms)
    }

    #[test]
    fn fts5_queries_quote_words_and_prefix_the_last() {
        assert_eq!(fts5_query(&terms("ada lov")), "\"ada\" AND \"lov\"*");
        assert_eq!(fts5_query(&terms("acme ")), "(\"acme\" OR \"acme corp\")");
    }

    #[test]
    fn tsqueries_join_words_and_synonyms() {
        assert_eq!(tsquery(&terms("ada lov")), "ada & lov:*");
        assert_eq!(tsquery(&terms("acme ")), "(acme | acme <-> corp)");
    }

    fn postgres_sql(text: &str, trigram: bool) -> (String, String) {
        let (mut select, mut count) =
            (Sql::new(DbBackend::Postgres), Sql::new(DbBackend::Postgres));
        let options = PostgresOptions {
            trigram,
            ..PostgresOptions::default()
        };
        build_from(
            &mut select,
            &mut count,
            "crm_deals",
            &terms(text),
            true,
            &options,
            text,
        );
        (select.text().to_owned(), count.text().to_owned())
    }

    #[test]
    fn postgres_queries_match_every_word_and_rank_as_float8() {
        let (select, count) = postgres_sql("ada lov", false);
        assert_eq!(
            select,
            "(ts_rank('{0.1, 0.2, 0.4, 1.0}', d.search_vector, to_tsquery($1::regconfig, $2), 32))::float8 AS rank \
             FROM search_documents d WHERE d.index_name = $3 AND d.search_vector @@ to_tsquery($4::regconfig, $5)"
        );
        assert_eq!(
            count,
            "FROM search_documents d WHERE d.index_name = $1 AND d.search_vector @@ to_tsquery($2::regconfig, $3)"
        );
        let (select, _) = postgres_sql("", false);
        assert_eq!(
            select,
            "0.0::float8 AS rank FROM search_documents d WHERE d.index_name = $1"
        );
    }

    #[test]
    fn postgres_trigrams_let_each_word_match_on_its_own() {
        let (select, count) = postgres_sql("ada lovlace", true);
        assert_eq!(
            select,
            "(ts_rank('{0.1, 0.2, 0.4, 1.0}', d.search_vector, to_tsquery($1::regconfig, $2), 32) \
             + 0.5 * word_similarity($3, d.content))::float8 AS rank \
             FROM search_documents d WHERE d.index_name = $4 \
             AND (d.search_vector @@ to_tsquery($5::regconfig, $6) OR $7 <% d.content) \
             AND (d.search_vector @@ to_tsquery($8::regconfig, $9) OR $10 <% d.content)"
        );
        assert!(
            count.starts_with("FROM search_documents d WHERE d.index_name = $1 AND ("),
            "{count}"
        );
        assert_eq!(tsquery_any(&terms("ada lovlace")), "ada | lovlace:*");
    }

    #[test]
    fn boosts_map_to_weight_classes() {
        assert_eq!(weight_class(10), 'A');
        assert_eq!(weight_class(5), 'A');
        assert_eq!(weight_class(3), 'B');
        assert_eq!(weight_class(2), 'C');
        assert_eq!(weight_class(1), 'D');
    }
}
