use navree_macros::{injectable, module};

#[injectable]
struct Repo;

#[module(providers = Repo)]
struct DataModule;

fn main() {}
