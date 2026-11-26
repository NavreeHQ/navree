use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
#[model(timestamps, soft_deletes)]
struct Deal {
    id: i64,
    updated_at: navree_datetime::Timestamp,
    deleted_at: navree_datetime::Timestamp,
}

#[derive(Model, Clone, serde::Serialize)]
#[model(timestamps(created_at = true, updated_at = false))]
struct Note {
    id: i64,
}

fn main() {}
