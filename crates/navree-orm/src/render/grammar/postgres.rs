//! PostgreSQL and TimescaleDB: the shared SQL plus `DISTINCT ON`. Every
//! other default (`ILIKE`, `IS [NOT] DISTINCT FROM`, `->`/`->>`, `ON
//! CONFLICT`, `RETURNING`, `FOR NO KEY UPDATE`, `TRUNCATE .. RESTART
//! IDENTITY`) is PostgreSQL's already.

use super::Grammar;
use crate::error::{OrmError, Result};
use crate::render::Writer;
use crate::render::ast::Distinct;
use crate::sql::Dialect;

/// The PostgreSQL (and TimescaleDB) grammar.
#[derive(Clone, Copy, Debug, Default)]
pub struct Postgres;

impl Grammar for Postgres {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }

    fn distinct(&self, w: &mut Writer, distinct: &Distinct) -> Result<()> {
        match distinct {
            Distinct::All => {}
            Distinct::Distinct => w.push("DISTINCT "),
            Distinct::On(exprs) => {
                if exprs.is_empty() {
                    return Err(OrmError::invalid(
                        "DISTINCT ON needs at least one expression",
                    ));
                }
                w.push("DISTINCT ON (");
                self.expr_list(w, exprs)?;
                w.push(") ");
            }
        }
        Ok(())
    }
}
