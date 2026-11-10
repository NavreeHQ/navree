use navree_orm::Model;
use serde::Serialize;

#[derive(Clone, Default, Serialize)]
struct Money {
    cents: i64,
    currency: String,
}

#[derive(Model, Clone, Default, Serialize)]
struct OneColumn {
    id: i64,
    #[model(cast = MoneyColumns, columns(cents: i64))]
    price: Money,
}

#[derive(Model, Clone, Default, Serialize)]
struct WithoutCast {
    id: i64,
    #[model(columns(cents: i64, currency: String))]
    price: Money,
}

#[derive(Model, Clone, Default, Serialize)]
struct BuiltinCast {
    id: i64,
    #[model(cast = "json", columns(cents: i64, currency: String))]
    price: Money,
}

#[derive(Model, Clone, Default, Serialize)]
struct CastAndInbound {
    id: i64,
    #[model(cast = "json", inbound = Slug)]
    slug: String,
}

fn main() {}
