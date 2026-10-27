use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
struct Deal {
    id: i64,
    #[model(column = "title")]
    name: String,
    title: String,
    #[model(skip, column = "x")]
    cache: String,
}

fn main() {}
