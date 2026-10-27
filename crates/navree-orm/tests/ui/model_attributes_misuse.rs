use navree_orm::{Decimal, Model};
use serde::Serialize;

#[derive(Model, Clone, Default, Serialize)]
#[model(fillable(titel, "nope->path"), hidden(secrett), visible(id))]
struct Misspelled {
    id: i64,
    title: String,
    secret: String,
}

#[derive(Model, Clone, Default, Serialize)]
#[model(unguarded, fillable(title))]
struct UnguardedAndFillable {
    id: i64,
    title: String,
}

#[derive(Model, Clone, Default, Serialize)]
#[model(fillable(title), guarded(title))]
struct FillableAndGuarded {
    id: i64,
    title: String,
}

#[derive(Model, Clone, Default, Serialize)]
struct BadCasts {
    id: i64,
    #[model(cast = "decimal")]
    amount: Decimal,
    #[model(cast = "decimal(30)")]
    precise: Decimal,
    #[model(cast = "datetime_format(\"%Q\")")]
    at: chrono::DateTime<chrono::Utc>,
    #[model(cast = "encrypt")]
    secret: String,
}

#[derive(Model, Clone, Default, Serialize)]
struct SkippedColumns {
    id: i64,
    #[model(skip, fillable)]
    cache: String,
}

#[derive(Model, Clone, Default, Serialize)]
#[model(fillable)]
struct ListWithoutNames {
    id: i64,
}

fn main() {}
