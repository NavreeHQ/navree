use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
struct Deal<T: Clone + serde::Serialize> {
    id: i64,
    extra: T,
}

fn main() {}
