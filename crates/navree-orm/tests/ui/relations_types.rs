use navree_orm::Model;
use navree_orm::relations::BelongsTo;

#[derive(Model, Clone, Default, serde::Serialize)]
struct Company {
    id: i64,
}

// Relation definitions are read from a default instance.
#[derive(Model, Clone, serde::Serialize)]
struct Deal {
    id: i64,
    company_id: Option<i64>,
}

#[navree_orm::relations]
impl Deal {
    fn company(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
    }
}

// A relation method returns a relation.
#[navree_orm::relations]
impl Company {
    fn label(&self) -> String {
        String::new()
    }
}

fn main() {}
