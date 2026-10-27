//! What exports and imports share: the clock, the locale they run in, the
//! header translator, the disks and SQL placeholders.

use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_contracts::Clock;
use navree_kernel::AppContext;
use navree_storage::{Disk, Storage};
use sea_orm::{DbBackend, Statement, Value};

use crate::{HeaderTranslator, SheetsError};

/// The time, from the app's `dyn Clock` when it has one (tests fix it).
pub(crate) async fn now(app: &AppContext) -> DateTime<Utc> {
    match app.try_get::<dyn Clock>().await {
        Some(clock) => clock.now(),
        None => Utc::now(),
    }
}

/// The locale of the running code: the request's (navree-i18n's
/// `Locale::current`, with the `i18n` feature), else `None`.
pub(crate) fn current_locale() -> Option<String> {
    #[cfg(feature = "i18n")]
    {
        Some(navree_i18n::Locale::current().tag().to_owned())
    }
    #[cfg(not(feature = "i18n"))]
    {
        None
    }
}

/// Runs `future` in `locale` when the app has navree-i18n's `I18n` (the
/// `i18n` feature): queued work runs in the locale of the request that
/// queued it.
pub(crate) async fn in_locale<F: Future>(
    app: &AppContext,
    locale: Option<&str>,
    future: F,
) -> F::Output {
    #[cfg(feature = "i18n")]
    if let Some(tag) = locale
        && let Some(i18n) = app.try_get::<navree_i18n::I18n>().await
    {
        return i18n.scope(tag, future).await;
    }
    let _ = (app, locale);
    future.await
}

/// The app's header translator: its `dyn HeaderTranslator`, else (with the
/// `i18n` feature) its `I18n`, else none.
pub(crate) async fn header_translator(app: &AppContext) -> Option<Arc<dyn HeaderTranslator>> {
    if let Some(translator) = app.try_get::<dyn HeaderTranslator>().await {
        return Some(translator);
    }
    #[cfg(feature = "i18n")]
    if let Some(i18n) = app.try_get::<navree_i18n::I18n>().await {
        return Some(Arc::new(I18nHeaders(i18n)));
    }
    None
}

/// Headers translated with navree-i18n: a header is a message id (or
/// `id.attribute`) of the Fluent files.
#[cfg(feature = "i18n")]
struct I18nHeaders(Arc<navree_i18n::I18n>);

#[cfg(feature = "i18n")]
impl HeaderTranslator for I18nHeaders {
    fn translate(&self, key: &str, locale: Option<&str>) -> Option<String> {
        let default = self.0.default_locale();
        let tag = locale.unwrap_or(&default);
        if !self.0.has(tag, key) && !self.0.has(&default, key) {
            return None;
        }
        self.0.t(tag, key, navree_i18n::Args::new()).ok()
    }
}

/// The disk named `name` (`""`: the default disk).
pub(crate) async fn disk(app: &AppContext, name: &str) -> Result<Disk, SheetsError> {
    let storage = app.try_get::<Storage>().await.ok_or(SheetsError::Missing(
        "files need StorageModule in the app (navree-storage)",
    ))?;
    if name.trim().is_empty() {
        Ok(storage.default_disk())
    } else {
        Ok(storage.disk(name)?)
    }
}

/// `sql` with `?` placeholders, numbered (`$1`) on PostgreSQL.
pub(crate) fn statement(backend: DbBackend, sql: &str, values: Vec<Value>) -> Statement {
    let sql = if backend == DbBackend::Postgres {
        let mut numbered = String::with_capacity(sql.len() + 8);
        let mut n = 0;
        for c in sql.chars() {
            if c == '?' {
                n += 1;
                numbered.push('$');
                numbered.push_str(&n.to_string());
            } else {
                numbered.push(c);
            }
        }
        numbered
    } else {
        sql.to_owned()
    };
    Statement::from_sql_and_values(backend, sql, values)
}

/// `secs` seconds as a duration, at most a hundred years (larger configured
/// values would overflow date arithmetic).
pub(crate) fn seconds(secs: u64) -> chrono::Duration {
    const CENTURY: u64 = 100 * 365 * 86_400;
    chrono::Duration::seconds(i64::try_from(secs.min(CENTURY)).unwrap_or(0))
}

/// `n` as an `i64` column value (saturating).
pub(crate) fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_numbered_on_postgres() {
        let stmt = statement(
            DbBackend::Postgres,
            "a = ? AND b = ?",
            vec![1.into(), 2.into()],
        );
        assert_eq!(stmt.sql, "a = $1 AND b = $2");
        let stmt = statement(DbBackend::Sqlite, "a = ?", vec![1.into()]);
        assert_eq!(stmt.sql, "a = ?");
    }
}
