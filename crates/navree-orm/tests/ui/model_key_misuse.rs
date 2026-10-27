use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
struct Optional {
    id: Option<i64>,
}

#[derive(Model, Clone, serde::Serialize)]
struct Cast {
    #[model(cast = "json")]
    id: Vec<u8>,
}

fn main() {}
