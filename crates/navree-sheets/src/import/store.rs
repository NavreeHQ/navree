//! The SQL of `imports`, `import_rows` and `import_mappings`. Ids are
//! stored as text, times as Unix milliseconds and JSON as text, so the
//! same statements run on SQLite, PostgreSQL and MySQL.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use navree_db::{DbError, Executor, Row, Statement, Value as SqlValue};
use serde_json::Value;
use uuid::Uuid;

use super::{
    DuplicateMode, FileKind, ImportCounts, ImportRecord, ImportRowRecord, ImportStatus, Mapping,
    RowStatus,
};
use crate::support::to_i64;

/// The imports table.
pub const IMPORTS_TABLE: &str = "imports";
/// The staged rows table.
pub const IMPORT_ROWS_TABLE: &str = "import_rows";
/// The saved mappings table.
pub const IMPORT_MAPPINGS_TABLE: &str = "import_mappings";

/// The columns of `imports` read back, in [`record`]'s order.
const COLUMNS: &str = "id, importer, status, filename, disk, path, format, encoding, sheet, \
     locale, headers, mapping, mapping_chosen, save_mapping, missing, duplicates, counts, \
     operation_id, error_path, error, owner_id, workspace_id, created_at, updated_at, finished_at";

/// Rows per multi-row `INSERT` (SQLite allows 32,766 bound values).
const INSERT_BATCH: usize = 500;

/// A staged row to store.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NewRow {
    pub(crate) row: u64,
    pub(crate) status: RowStatus,
    pub(crate) key: Option<String>,
    pub(crate) cells: BTreeMap<String, Value>,
    pub(crate) errors: BTreeMap<String, Vec<String>>,
}

/// A stored row to apply.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StoredRow {
    pub(crate) row: u64,
    pub(crate) status: RowStatus,
    pub(crate) cells: BTreeMap<String, Value>,
    pub(crate) errors: BTreeMap<String, Vec<String>>,
}

pub(crate) fn millis(at: DateTime<Utc>) -> i64 {
    at.timestamp_millis()
}

pub(crate) fn from_millis(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default()
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

fn parse<T: serde::de::DeserializeOwned + Default>(text: Option<String>) -> T {
    text.and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn id_text(id: Option<Uuid>) -> SqlValue {
    id.map(|id| id.to_string()).into()
}

fn uuid_of(text: Option<String>) -> Option<Uuid> {
    text.and_then(|text| Uuid::parse_str(&text).ok())
}

/// The values of every column but `id` and `created_at`, in
/// [`UPDATE_SET`]'s order.
fn mutable_values(record: &ImportRecord) -> Vec<SqlValue> {
    vec![
        record.status.as_str().into(),
        record.format.map(|f| f.as_str().to_owned()).into(),
        record.encoding.clone().into(),
        record.sheet.clone().into(),
        record.locale.clone().into(),
        json(&record.headers).into(),
        json(&record.mapping).into(),
        record.mapping_chosen.into(),
        record.save_mapping.into(),
        json(&record.missing).into(),
        record.duplicates.map(|d| d.as_str().to_owned()).into(),
        json(&record.counts).into(),
        id_text(record.operation_id),
        record.error_path.clone().into(),
        record.error.clone().into(),
        millis(record.updated_at).into(),
        record.finished_at.map(millis).into(),
    ]
}

const UPDATE_SET: &str = "status = ?, format = ?, encoding = ?, sheet = ?, locale = ?, \
     headers = ?, mapping = ?, mapping_chosen = ?, save_mapping = ?, missing = ?, \
     duplicates = ?, counts = ?, operation_id = ?, error_path = ?, error = ?, updated_at = ?, \
     finished_at = ?";

pub(crate) async fn insert_import<E: Executor + ?Sized>(
    exec: &E,
    record: &ImportRecord,
) -> Result<(), DbError> {
    let mut values = vec![
        record.id.to_string().into(),
        record.importer.clone().into(),
        record.filename.clone().into(),
        record.disk.clone().into(),
        record.path.clone().into(),
        id_text(record.owner_id),
        id_text(record.workspace_id),
        millis(record.created_at).into(),
    ];
    values.extend(mutable_values(record));
    let sql = format!(
        "INSERT INTO {IMPORTS_TABLE} (id, importer, filename, disk, path, owner_id, \
         workspace_id, created_at, status, format, encoding, sheet, locale, headers, mapping, \
         mapping_chosen, save_mapping, missing, duplicates, counts, operation_id, error_path, \
         error, updated_at, finished_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
         ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    );
    exec.execute(&Statement::portable(&sql, values)).await?;
    Ok(())
}

pub(crate) async fn update_import<E: Executor + ?Sized>(
    exec: &E,
    record: &ImportRecord,
) -> Result<(), DbError> {
    let mut values = mutable_values(record);
    values.push(record.id.to_string().into());
    let sql = format!("UPDATE {IMPORTS_TABLE} SET {UPDATE_SET} WHERE id = ?");
    exec.execute(&Statement::portable(&sql, values)).await?;
    Ok(())
}

fn record(row: &Row) -> Result<ImportRecord, DbError> {
    let text = |name: &str| row.get::<Option<String>>(name);
    let id: String = row.get("id")?;
    Ok(ImportRecord {
        id: Uuid::parse_str(&id)
            .map_err(|e| DbError::new(navree_db::ErrorKind::Decode, e.to_string()))?,
        importer: row.get("importer")?,
        status: ImportStatus::parse(&row.get::<String>("status")?).unwrap_or(ImportStatus::Failed),
        filename: row.get("filename")?,
        format: text("format")?.and_then(|f| FileKind::parse(&f)),
        encoding: text("encoding")?,
        sheet: text("sheet")?,
        headers: parse(text("headers")?),
        mapping: parse(text("mapping")?),
        missing: parse(text("missing")?),
        duplicates: text("duplicates")?.and_then(|d| DuplicateMode::parse(&d)),
        counts: parse(text("counts")?),
        operation_id: uuid_of(text("operation_id")?),
        has_error_file: text("error_path")?.is_some(),
        error: text("error")?,
        owner_id: uuid_of(text("owner_id")?),
        workspace_id: uuid_of(text("workspace_id")?),
        created_at: from_millis(row.get("created_at")?),
        updated_at: from_millis(row.get("updated_at")?),
        finished_at: row.get::<Option<i64>>("finished_at")?.map(from_millis),
        disk: row.get("disk")?,
        path: row.get("path")?,
        error_path: text("error_path")?,
        locale: text("locale")?,
        mapping_chosen: row.get("mapping_chosen")?,
        save_mapping: row.get("save_mapping")?,
    })
}

pub(crate) async fn find_import<E: Executor + ?Sized>(
    exec: &E,
    id: Uuid,
) -> Result<Option<ImportRecord>, DbError> {
    let sql = format!("SELECT {COLUMNS} FROM {IMPORTS_TABLE} WHERE id = ?");
    let row = exec
        .fetch_optional(&Statement::portable(&sql, vec![id.to_string().into()]))
        .await?;
    row.as_ref().map(record).transpose()
}

/// The imports `owner` uploaded, newest first.
pub(crate) async fn imports_of<E: Executor + ?Sized>(
    exec: &E,
    owner: Uuid,
    workspace: Option<Uuid>,
    limit: u64,
) -> Result<Vec<ImportRecord>, DbError> {
    let (filter, mut values) = match workspace {
        Some(workspace) => (
            "owner_id = ? AND workspace_id = ?",
            vec![owner.to_string().into(), workspace.to_string().into()],
        ),
        None => ("owner_id = ?", vec![owner.to_string().into()]),
    };
    values.push(to_i64(limit).into());
    let sql = format!(
        "SELECT {COLUMNS} FROM {IMPORTS_TABLE} WHERE {filter} ORDER BY created_at DESC, id DESC \
         LIMIT ?"
    );
    // A list of `?` placeholders and fixed fragments built here; every value is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    let rows = exec.fetch_all(&Statement::portable(&sql, values)).await?;
    rows.iter().map(record).collect()
}

/// Imports not changed since `before` and not being applied.
pub(crate) async fn stale_imports<E: Executor + ?Sized>(
    exec: &E,
    before: DateTime<Utc>,
) -> Result<Vec<ImportRecord>, DbError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM {IMPORTS_TABLE} WHERE updated_at < ? AND status <> ? \
         ORDER BY updated_at"
    );
    let rows = exec
        .fetch_all(&Statement::portable(
            &sql,
            vec![
                millis(before).into(),
                ImportStatus::Applying.as_str().into(),
            ],
        ))
        .await?;
    rows.iter().map(record).collect()
}

/// Deletes the import and its rows.
pub(crate) async fn delete_import<E: Executor + ?Sized>(exec: &E, id: Uuid) -> Result<(), DbError> {
    delete_rows(exec, id).await?;
    let sql = format!("DELETE FROM {IMPORTS_TABLE} WHERE id = ?");
    exec.execute(&Statement::portable(&sql, vec![id.to_string().into()]))
        .await?;
    Ok(())
}

pub(crate) async fn delete_rows<E: Executor + ?Sized>(exec: &E, id: Uuid) -> Result<(), DbError> {
    let sql = format!("DELETE FROM {IMPORT_ROWS_TABLE} WHERE import_id = ?");
    exec.execute(&Statement::portable(&sql, vec![id.to_string().into()]))
        .await?;
    Ok(())
}

pub(crate) async fn insert_rows<E: Executor + ?Sized>(
    exec: &E,
    import_id: Uuid,
    rows: &[NewRow],
) -> Result<(), DbError> {
    for batch in rows.chunks(INSERT_BATCH) {
        let placeholders = vec!["(?, ?, ?, ?, ?, ?)"; batch.len()].join(", ");
        let sql = format!(
            "INSERT INTO {IMPORT_ROWS_TABLE} (import_id, row_no, status, row_key, cells, errors) \
             VALUES {placeholders}"
        );
        let mut values: Vec<SqlValue> = Vec::with_capacity(batch.len() * 6);
        for row in batch {
            values.push(import_id.to_string().into());
            values.push(to_i64(row.row).into());
            values.push(row.status.as_str().into());
            values.push(row.key.clone().into());
            values.push(json(&row.cells).into());
            values.push(json(&row.errors).into());
        }
        // A list of `?` placeholders and fixed fragments built here; every value is a bound parameter.
        // nosemgrep: navree-sql-built-from-strings
        exec.execute(&Statement::portable(&sql, values)).await?;
    }
    Ok(())
}

fn stored(row: &Row) -> Result<StoredRow, DbError> {
    Ok(StoredRow {
        row: u64::try_from(row.get::<i64>("row_no")?).unwrap_or(0),
        status: RowStatus::parse(&row.get::<String>("status")?).unwrap_or(RowStatus::Invalid),
        cells: parse(row.get::<Option<String>>("cells")?),
        errors: parse(row.get::<Option<String>>("errors")?),
    })
}

fn status_list(statuses: &[RowStatus]) -> (String, Vec<SqlValue>) {
    let placeholders = vec!["?"; statuses.len()].join(", ");
    let values = statuses.iter().map(|s| s.as_str().into()).collect();
    (placeholders, values)
}

/// A page of rows (`statuses` empty: every row), ordered by row number,
/// and how many there are in all.
pub(crate) async fn rows_page<E: Executor + ?Sized>(
    exec: &E,
    import_id: Uuid,
    statuses: &[RowStatus],
    offset: u64,
    limit: u64,
) -> Result<(Vec<ImportRowRecord>, u64), DbError> {
    let mut filter = "import_id = ?".to_owned();
    let mut values: Vec<SqlValue> = vec![import_id.to_string().into()];
    if !statuses.is_empty() {
        let (placeholders, statuses) = status_list(statuses);
        filter.push_str(&format!(" AND status IN ({placeholders})"));
        values.extend(statuses);
    }
    let count_sql = format!("SELECT COUNT(*) AS total FROM {IMPORT_ROWS_TABLE} WHERE {filter}");
    // A list of `?` placeholders and fixed fragments built here; every value is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    let total = exec
        .fetch_optional(&Statement::portable(&count_sql, values.clone()))
        .await?
        .map(|row| row.get::<i64>("total"))
        .transpose()?
        .unwrap_or(0);
    let sql = format!(
        "SELECT row_no, status, cells, errors FROM {IMPORT_ROWS_TABLE} WHERE {filter} \
         ORDER BY row_no LIMIT ? OFFSET ?"
    );
    values.push(to_i64(limit).into());
    values.push(to_i64(offset).into());
    // A list of `?` placeholders and fixed fragments built here; every value is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    let rows = exec.fetch_all(&Statement::portable(&sql, values)).await?;
    let records = rows
        .iter()
        .map(|row| {
            stored(row).map(|row| ImportRowRecord {
                row: row.row,
                status: row.status,
                values: row.cells,
                errors: row.errors,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok((records, u64::try_from(total).unwrap_or(0)))
}

/// How many rows are in each state.
pub(crate) async fn counts<E: Executor + ?Sized>(
    exec: &E,
    import_id: Uuid,
) -> Result<ImportCounts, DbError> {
    let sql = format!(
        "SELECT status, COUNT(*) AS n FROM {IMPORT_ROWS_TABLE} WHERE import_id = ? GROUP BY status"
    );
    let rows = exec
        .fetch_all(&Statement::portable(
            &sql,
            vec![import_id.to_string().into()],
        ))
        .await?;
    let mut counts = ImportCounts::default();
    for row in rows {
        let status: String = row.get("status")?;
        let n = u64::try_from(row.get::<i64>("n")?).unwrap_or(0);
        if let Some(status) = RowStatus::parse(&status) {
            counts.add(status, n);
            counts.total += n;
        }
    }
    Ok(counts)
}

/// The row numbers in `statuses`, cut into ranges of at most `size` rows:
/// one apply job each.
pub(crate) async fn row_ranges<E: Executor + ?Sized>(
    exec: &E,
    import_id: Uuid,
    statuses: &[RowStatus],
    size: u64,
) -> Result<Vec<(u64, u64)>, DbError> {
    let (placeholders, statuses) = status_list(statuses);
    let sql = format!(
        "SELECT row_no FROM {IMPORT_ROWS_TABLE} WHERE import_id = ? AND status IN \
         ({placeholders}) ORDER BY row_no"
    );
    let mut values: Vec<SqlValue> = vec![import_id.to_string().into()];
    values.extend(statuses);
    // A list of `?` placeholders and fixed fragments built here; every value is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    let rows = exec.fetch_all(&Statement::portable(&sql, values)).await?;
    let numbers: Vec<u64> = rows
        .iter()
        .map(|row| {
            row.get::<i64>("row_no")
                .map(|n| u64::try_from(n).unwrap_or(0))
        })
        .collect::<Result<_, _>>()?;
    let size = usize::try_from(size.max(1)).unwrap_or(usize::MAX);
    Ok(numbers
        .chunks(size)
        .filter_map(|chunk| Some((*chunk.first()?, *chunk.last()?)))
        .collect())
}

/// The rows numbered `from..=to` in `statuses`.
pub(crate) async fn rows_between<E: Executor + ?Sized>(
    exec: &E,
    import_id: Uuid,
    from: u64,
    to: u64,
    statuses: &[RowStatus],
) -> Result<Vec<StoredRow>, DbError> {
    let (placeholders, statuses) = status_list(statuses);
    let sql = format!(
        "SELECT row_no, status, cells, errors FROM {IMPORT_ROWS_TABLE} WHERE import_id = ? AND \
         row_no >= ? AND row_no <= ? AND status IN ({placeholders}) ORDER BY row_no"
    );
    let mut values: Vec<SqlValue> = vec![
        import_id.to_string().into(),
        to_i64(from).into(),
        to_i64(to).into(),
    ];
    values.extend(statuses);
    // A list of `?` placeholders and fixed fragments built here; every value is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    let rows = exec.fetch_all(&Statement::portable(&sql, values)).await?;
    rows.iter().map(stored).collect()
}

/// The rows in `statuses` after row `after`, at most `limit`.
pub(crate) async fn rows_after<E: Executor + ?Sized>(
    exec: &E,
    import_id: Uuid,
    after: u64,
    limit: u64,
    statuses: &[RowStatus],
) -> Result<Vec<StoredRow>, DbError> {
    let (placeholders, statuses) = status_list(statuses);
    let sql = format!(
        "SELECT row_no, status, cells, errors FROM {IMPORT_ROWS_TABLE} WHERE import_id = ? AND \
         row_no > ? AND status IN ({placeholders}) ORDER BY row_no LIMIT ?"
    );
    let mut values: Vec<SqlValue> = vec![import_id.to_string().into(), to_i64(after).into()];
    values.extend(statuses);
    values.push(to_i64(limit).into());
    // A list of `?` placeholders and fixed fragments built here; every value is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    let rows = exec.fetch_all(&Statement::portable(&sql, values)).await?;
    rows.iter().map(stored).collect()
}

/// Sets one row's status and errors.
pub(crate) async fn set_row<E: Executor + ?Sized>(
    exec: &E,
    import_id: Uuid,
    row: u64,
    status: RowStatus,
    errors: &BTreeMap<String, Vec<String>>,
) -> Result<(), DbError> {
    let sql = format!(
        "UPDATE {IMPORT_ROWS_TABLE} SET status = ?, errors = ? WHERE import_id = ? AND row_no = ?"
    );
    exec.execute(&Statement::portable(
        &sql,
        vec![
            status.as_str().into(),
            json(errors).into(),
            import_id.to_string().into(),
            to_i64(row).into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Sets the status of many rows (their errors are kept).
pub(crate) async fn set_rows<E: Executor + ?Sized>(
    exec: &E,
    import_id: Uuid,
    rows: &[u64],
    status: RowStatus,
) -> Result<(), DbError> {
    for batch in rows.chunks(INSERT_BATCH) {
        let placeholders = vec!["?"; batch.len()].join(", ");
        let sql = format!(
            "UPDATE {IMPORT_ROWS_TABLE} SET status = ? WHERE import_id = ? AND row_no IN \
             ({placeholders})"
        );
        let mut values: Vec<SqlValue> = vec![status.as_str().into(), import_id.to_string().into()];
        values.extend(batch.iter().map(|n| SqlValue::from(to_i64(*n))));
        // A list of `?` placeholders and fixed fragments built here; every value is a bound parameter.
        // nosemgrep: navree-sql-built-from-strings
        exec.execute(&Statement::portable(&sql, values)).await?;
    }
    Ok(())
}

/// The saved mapping of `importer` for files with these headers.
pub(crate) async fn find_mapping<E: Executor + ?Sized>(
    exec: &E,
    importer: &str,
    workspace: Option<Uuid>,
    fingerprint: &str,
) -> Result<Option<Mapping>, DbError> {
    let (filter, mut values) = workspace_filter(workspace);
    let sql = format!(
        "SELECT mapping FROM {IMPORT_MAPPINGS_TABLE} WHERE importer = ? AND fingerprint = ? AND \
         {filter} ORDER BY updated_at DESC LIMIT 1"
    );
    let mut all: Vec<SqlValue> = vec![importer.into(), fingerprint.into()];
    all.append(&mut values);
    // A list of `?` placeholders and fixed fragments built here; every value is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    let row = exec.fetch_optional(&Statement::portable(&sql, all)).await?;
    Ok(row
        .map(|row| row.get::<Option<String>>("mapping"))
        .transpose()?
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok()))
}

fn workspace_filter(workspace: Option<Uuid>) -> (&'static str, Vec<SqlValue>) {
    match workspace {
        Some(id) => ("workspace_id = ?", vec![id.to_string().into()]),
        None => ("workspace_id IS NULL", Vec::new()),
    }
}

/// Saves `mapping` for `importer` and files with these headers, replacing
/// the one saved before.
pub(crate) async fn save_mapping<E: Executor + ?Sized>(
    exec: &E,
    importer: &str,
    workspace: Option<Uuid>,
    fingerprint: &str,
    mapping: &Mapping,
    now: DateTime<Utc>,
) -> Result<(), DbError> {
    let (filter, values) = workspace_filter(workspace);
    let sql = format!(
        "DELETE FROM {IMPORT_MAPPINGS_TABLE} WHERE importer = ? AND fingerprint = ? AND {filter}"
    );
    let mut all: Vec<SqlValue> = vec![importer.into(), fingerprint.into()];
    all.extend(values);
    // A list of `?` placeholders and fixed fragments built here; every value is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    exec.execute(&Statement::portable(&sql, all)).await?;
    let sql = format!(
        "INSERT INTO {IMPORT_MAPPINGS_TABLE} (id, importer, workspace_id, fingerprint, mapping, \
         created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
    );
    exec.execute(&Statement::portable(
        &sql,
        vec![
            Uuid::now_v7().to_string().into(),
            importer.into(),
            id_text(workspace),
            fingerprint.into(),
            json(mapping).into(),
            millis(now).into(),
            millis(now).into(),
        ],
    ))
    .await?;
    Ok(())
}
