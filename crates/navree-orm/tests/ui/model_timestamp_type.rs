use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
#[model(timestamps(updated_at = false))]
struct Deal {
    id: i64,
    created_at: String,
}

fn main() {}
