use navree_orm::Model;
use navree_orm::factory::{FactoryContext, ModelFactory};

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(route_key = "slug", workspace)]
struct Deal {
    id: i64,
    title: String,
}

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(workspace = "team_id")]
struct Note {
    id: i64,
    #[model(cast = "json")]
    team_id: Vec<u8>,
}

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(factory = "not a path")]
struct Tag {
    id: i64,
}

/// A factory of companies...
#[derive(Default)]
struct CompanyFactory;

#[derive(Model, Clone, Default, serde::Serialize)]
struct Company {
    id: i64,
}

impl ModelFactory for CompanyFactory {
    type Model = Company;

    fn definition(&self, _ctx: &mut FactoryContext<'_, Company>) -> Company {
        Company::default()
    }
}

/// ...named as the factory of another model.
#[derive(Model, Clone, Default, serde::Serialize)]
#[model(factory = CompanyFactory)]
struct Person {
    id: i64,
}

fn main() {}
