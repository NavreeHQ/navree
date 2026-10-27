use navree_orm::{Builder, Model, scopes};

#[derive(Model, Clone, Default, serde::Serialize)]
struct Deal {
    id: i64,
}

#[scopes]
impl Deal {
    #[scope]
    fn open(&self, query: Builder<Self>) -> Builder<Self> {
        query
    }

    #[scope]
    async fn later(query: Builder<Self>) -> Builder<Self> {
        query
    }
}

struct Other;

#[scopes]
impl Other {
    fn helper() {}
}

fn main() {}
