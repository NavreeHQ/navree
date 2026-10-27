use navree_orm::{Model, accessors};
use serde::Serialize;

#[derive(Model, Clone, Default, Serialize)]
#[model(appends(full_name))]
struct WithoutAccessor {
    id: i64,
}

#[derive(Model, Clone, Default, Serialize)]
struct WithAccessors {
    id: i64,
    name: String,
}

#[accessors]
impl WithAccessors {
    #[accessor]
    fn mutating(&mut self) -> String {
        String::new()
    }

    #[accessor]
    fn with_argument(&self, prefix: &str) -> String {
        prefix.to_owned()
    }

    #[accessor]
    async fn later(&self) -> String {
        String::new()
    }

    #[accessor]
    fn nothing(&self) {}

    #[accessor(rename = "x")]
    fn unknown_option(&self) -> u8 {
        1
    }

    #[accessor]
    fn first(&self) -> u8 {
        1
    }

    #[accessor(name = "first")]
    fn second(&self) -> u8 {
        2
    }
}

#[derive(Model, Clone, Default, Serialize)]
struct Empty {
    id: i64,
}

#[accessors]
impl Empty {
    fn plain(&self) -> u8 {
        0
    }
}

#[derive(Model, Clone, Default, Serialize)]
struct Setter {
    id: i64,
    #[model(set_with = keep)]
    key: String,
}

fn keep(setter: &mut Setter, key: String) {
    setter.key = key;
}

struct Loose;

impl Loose {
    #[navree_orm::accessor]
    fn alone(&self) -> u8 {
        0
    }
}

fn main() {}
