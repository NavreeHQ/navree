//! navree-db's own data layer on SQLx: [`Backend`]s, [`Value`]s,
//! [`Statement`]s and [`Row`]s, classified [`DbError`]s, [`Pool`]s,
//! [`Transaction`]s with savepoints, and the [`Executor`] they share.
//!
//! Nothing here depends on SeaORM except [`compat`], the bridge that runs
//! navree-db statements on SeaORM connections (and converts values and
//! errors) while crates move off SeaORM; see
//! `docs/design`.

mod backend;
pub mod compat;
pub(crate) mod driver;
mod error;
mod executor;
mod pool;
pub(crate) mod report;
mod row;
mod statement;
pub(crate) mod transaction;
mod value;

pub use backend::Backend;
pub use driver::{Executed, RowStream};
pub use error::{DbError, ErrorKind};
pub use executor::{Executor, ExecutorExt};
pub use pool::Pool;
pub use row::{ColumnIndex, Row};
pub use statement::Statement;
pub use transaction::{Transaction, TxOptions};
pub use value::{FromValue, HasKind, Kind, Value, ValueError};

#[cfg(test)]
mod tests {
    /// The engine stays free of SeaORM outside the bridge: every source
    /// file but `compat.rs` (and the row type's SeaORM variant, which the
    /// bridge needs) is checked.
    #[test]
    fn the_engine_does_not_use_sea_orm_outside_the_bridge() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine");
        let mut offenders = Vec::new();
        let mut stack = vec![dir];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("reads src/engine") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if name == "compat.rs" || name == "mod.rs" {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("reads a source file");
                for (number, line) in text.lines().enumerate() {
                    let allowed = name == "row.rs"
                        && (line.contains("sea_orm::QueryResult") || line.contains("SeaORM"));
                    let allowed = allowed
                        || (name == "report.rs" && line.contains("compat::"))
                        || line.trim_start().starts_with("//");
                    if !allowed && (line.contains("sea_orm") || line.contains("sea_query")) {
                        offenders.push(format!("{}:{}: {line}", path.display(), number + 1));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "SeaORM outside the bridge:\n{}",
            offenders.join("\n")
        );
    }
}
