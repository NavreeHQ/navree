use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
enum Stage {
    Lead,
    Won,
}

fn main() {}
