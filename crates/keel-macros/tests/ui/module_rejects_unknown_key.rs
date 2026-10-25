use keel_macros::{injectable, module};

#[injectable]
struct Repo;

#[module(providers = [Repo])]
struct DataModule;

#[module(import = [DataModule], providers = [Repo])]
struct UsersModule;

fn main() {}
