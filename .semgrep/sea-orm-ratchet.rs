// ruleid: navree-sea-orm-ratchet
use sea_orm::{ActiveModelTrait, DatabaseConnection};
// ruleid: navree-sea-orm-ratchet
use sea_orm_migration::prelude::*;
// ok: navree-sea-orm-ratchet
use navree_orm::Model;

// A comment that mentions sea_orm::DatabaseConnection is not a use.
/// Doc text about sea_orm::Statement is not a use either.
// ok: navree-sea-orm-ratchet
const NAME: &str = "sea-orm";

// ruleid: navree-sea-orm-ratchet
fn f(db: &sea_orm::DatabaseConnection) {
    // ruleid: navree-sea-orm-ratchet
    let _ = sea_orm::Statement::from_string(sea_orm::DbBackend::Postgres, "x");
}

// ruleid: navree-sea-orm-ratchet
#[derive(sea_orm::DeriveEntityModel)]
// ruleid: navree-sea-orm-ratchet
#[sea_orm(table_name = "users")]
struct Model;
