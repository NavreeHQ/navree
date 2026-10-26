use navree_macros::Searchable;

#[derive(Searchable)]
#[searchable(table = "contacts")]
pub struct Model {
    pub id: u32,
}

#[derive(Searchable)]
pub struct Deal {
    pub id: u32,
    #[searchable(boost = 11)]
    pub title: String,
    #[searchable(fuzzy)]
    pub stage: String,
    #[searchable(rename = "_amount")]
    pub amount: i64,
}

fn main() {}
