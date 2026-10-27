//! Seeders with model factories: run from a test (`app.seed::<S>()`), and
//! without model events when they ask.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use navree_db::{Db, ModuleBuilderDbExt, Seeder};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory as Provider, Injectable, KernelError, Module,
    ModuleBuilder, Resolver,
};
use navree_orm::hooks::{ModelEvent, ModelHook};
use navree_orm::testing::{TestAppExt, assert_database_count};
use navree_orm::{ExecContext, Record};

use crate::models::Tag;
use crate::support::app;

/// Counts the tags the app's hooks see created.
#[derive(Default)]
struct Created(AtomicUsize);

struct CountTags(Arc<Created>);

impl ModelHook<Tag> for CountTags {
    fn after<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        _record: &'a mut Record<Tag>,
    ) -> BoxFuture<'a, navree_orm::Result<()>> {
        Box::pin(async move {
            if event == ModelEvent::Created {
                self.0.0.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        })
    }
}

/// Three tags, with model events.
struct TagsSeeder;

impl Injectable for TagsSeeder {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(TagsSeeder) })
    }
}

impl Seeder for TagsSeeder {
    fn name(&self) -> &'static str {
        "tags"
    }

    fn run<'a>(&'a self, _ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            Tag::factory().count(3).create(db).await?;
            Ok(())
        })
    }
}

/// Five tags, without model events (not registered: built for the call).
struct QuietTagsSeeder;

impl Injectable for QuietTagsSeeder {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(QuietTagsSeeder) })
    }
}

impl Seeder for QuietTagsSeeder {
    fn name(&self) -> &'static str {
        "quiet-tags"
    }

    fn without_model_events(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, _ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            Tag::factory().count(5).create(db).await?;
            Ok(())
        })
    }
}

struct Seeding(Arc<Created>);

impl Module for Seeding {
    fn register(&self, m: &mut ModuleBuilder) {
        let created = self.0.clone();
        m.global()
            .multi(Provider::<dyn ModelHook<Tag>>::new(move |_| {
                let created = created.clone();
                async move { Ok(Arc::new(CountTags(created)) as Arc<dyn ModelHook<Tag>>) }
            }))
            .seeder::<TagsSeeder>();
    }
}

#[tokio::test]
async fn seeders_run_from_tests_with_or_without_model_events() {
    let created = Arc::new(Created::default());
    let (app, db, _clock) = app(vec![Box::new(Seeding(created.clone()))]).await;

    app.seed::<TagsSeeder>().await.unwrap();
    assert_eq!(
        created.0.load(Ordering::SeqCst),
        3,
        "the app's hooks saw them"
    );
    app.seed::<QuietTagsSeeder>().await.unwrap();
    assert_eq!(created.0.load(Ordering::SeqCst), 3, "no model events");
    assert_database_count(&*db, "tags", 8).await;

    let ran = app.seed_all().await.unwrap();
    assert_eq!(ran, ["tags"]);
    assert_database_count(&*db, "tags", 11).await;
}
