use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
struct Deal(i64, String);

fn main() {}
