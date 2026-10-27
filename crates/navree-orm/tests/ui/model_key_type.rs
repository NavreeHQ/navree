use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
struct Deal {
    id: f64,
}

fn main() {}
