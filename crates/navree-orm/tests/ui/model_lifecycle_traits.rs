use navree_orm::{Model, Record};

/// Marked prunable without saying which records go.
#[derive(Model, Clone, serde::Serialize)]
#[model(prunable)]
struct Log {
    id: i64,
}

/// Marked as broadcasting without `impl Broadcasts`.
#[derive(Model, Clone, serde::Serialize)]
#[model(broadcasts)]
struct Room {
    id: i64,
}

/// No soft deletes: nothing to restore.
#[derive(Model, Clone, serde::Serialize)]
struct Company {
    id: i64,
}

async fn restore(db: navree_db::Db, mut company: Record<Company>) {
    let _ = company.restore(&db).await;
    let _ = Company::query().only_trashed();
}

fn main() {}
