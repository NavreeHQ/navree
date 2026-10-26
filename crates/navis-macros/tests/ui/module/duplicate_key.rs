use navis_macros::{injectable, module};

#[injectable]
struct Repo;

#[injectable]
struct Cache;

#[module(providers = [Repo], providers = [Cache])]
struct DataModule;

fn main() {}
