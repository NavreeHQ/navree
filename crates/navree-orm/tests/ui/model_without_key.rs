use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
#[model(table = "deals")]
struct Deal {
    deal_id: i64,
    title: String,
}

fn main() {}
