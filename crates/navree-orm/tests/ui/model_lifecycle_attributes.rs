use navree_orm::Model;

#[derive(Model, Clone, serde::Serialize)]
#[model(dispatches(creatd = DealCreated))]
struct Deal {
    id: i64,
}

#[derive(Model, Clone, serde::Serialize)]
#[model(prunable, mass_prunable)]
struct Log {
    id: i64,
}

#[derive(Model, Clone, serde::Serialize)]
#[model(broadcasts = true)]
struct Room {
    id: i64,
}

#[derive(Model, Clone, serde::Serialize)]
#[model(observers())]
struct Ticket {
    id: i64,
}

fn main() {}
