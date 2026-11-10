// ruleid: navree-no-sea-orm
use sea_orm::{ActiveModelTrait, DatabaseConnection};
// ruleid: navree-no-sea-orm
use sea_orm_migration::prelude::*;
// ruleid: navree-no-sea-orm
use sea_query::{Expr, Query};
// ruleid: navree-no-sea-orm
extern crate sea_query;
// ok: navree-no-sea-orm
use navree_orm::Model;
// ok: navree-no-sea-orm
use navree_db::{Executor, Statement};

// A comment that mentions sea_orm::DatabaseConnection is not a use.
/// Doc text about sea_query::Expr is not a use either.
// ok: navree-no-sea-orm
const NAME: &str = "sea_orm::DbErr";
// ok: navree-no-sea-orm
const SEAHASH: &str = "seahash";

// ruleid: navree-no-sea-orm
fn f(db: &sea_orm::DatabaseConnection) {
    // ruleid: navree-no-sea-orm
    let _ = sea_query::Query::select();
}

// ruleid: navree-no-sea-orm
#[derive(sea_orm::DeriveEntityModel)]
// ruleid: navree-no-sea-orm
#[sea_orm(table_name = "users")]
struct Model;

// ok: navree-no-sea-orm
fn seasons(sea_level: i32) -> i32 {
    sea_level
}
