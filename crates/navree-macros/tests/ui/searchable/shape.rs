use navree_macros::Searchable;

#[derive(Searchable)]
pub enum Status {
    Lead,
}

#[derive(Searchable)]
pub struct Pair(u32, String);

#[derive(Searchable)]
pub struct Contact {
    pub id: u32,
    #[searchable]
    #[searchable(filter)]
    pub status: String,
    #[searchable]
    pub email: String,
    #[searchable(rename = "email")]
    pub work_email: String,
}

fn main() {}
