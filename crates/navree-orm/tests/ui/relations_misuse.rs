use navree_orm::prelude::*;

#[derive(Model, Clone, Default, serde::Serialize)]
struct Company {
    id: i64,
}

#[derive(Model, Clone, Default, serde::Serialize)]
struct Deal {
    id: i64,
    company_id: Option<i64>,
}

#[navree_orm::relations]
impl Deal {
    async fn company(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
    }

    fn by_kind(&self, kind: &str) -> HasMany<Company, Deal> {
        let _ = kind;
        Company::default().has_many(Deal::COMPANY_ID)
    }

    fn generic<T>(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
    }

    fn owned(self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
    }

    fn nothing(&self) {}

    fn no_receiver() -> BelongsTo<Deal, Company> {
        Deal::default().belongs_to(Deal::COMPANY_ID)
    }
}

trait Named {
    fn label(&self) -> String;
}

#[navree_orm::relations]
impl Named for Company {
    fn label(&self) -> String {
        String::new()
    }
}

#[navree_orm::relations(deals)]
impl Company {
    fn deals(&self) -> HasMany<Company, Deal> {
        self.has_many(Deal::COMPANY_ID)
    }
}

struct Empty;

#[navree_orm::relations]
impl Empty {}

fn main() {}
