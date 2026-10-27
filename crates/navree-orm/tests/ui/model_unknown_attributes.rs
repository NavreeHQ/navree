use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
#[model(tabel = "deals")]
struct Deal {
    id: i64,
    #[model(colum = "name")]
    title: String,
}

#[derive(Model, Clone, serde::Serialize)]
#[model(key_type = "serial")]
struct Note {
    id: i64,
    #[model(cast = "money")]
    amount: i64,
}

fn main() {}
