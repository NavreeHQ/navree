use navree_macros::Searchable;

#[derive(Searchable)]
pub struct Deal {
    pub id: u32,
    #[model(skip)]
    #[searchable(filter)]
    pub label: String,
    #[model(columns(amount: i64, currency: String))]
    #[searchable(sort)]
    pub price: Money,
}

pub struct Money;

fn main() {}
