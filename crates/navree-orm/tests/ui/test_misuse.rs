use navree_orm::testing::database::{Template, TestDb};

static DATABASE: Template = Template::new("ui", |_| Box::pin(async { Ok(()) }));

#[navree_orm::test]
fn not_async() {}

#[navree_orm::test]
async fn two_databases(first: TestDb, second: TestDb) {}

#[navree_orm::test(template = DATABASE, database = "memory")]
async fn memory_from_a_template(db: TestDb) {}

#[navree_orm::test(database = "oracle")]
async fn unknown_database(db: TestDb) {}

#[navree_orm::test(template = DATABASE)]
async fn no_parameter() {}

#[navree_orm::test(templates = DATABASE)]
async fn unknown_option(db: TestDb) {}

fn main() {}
