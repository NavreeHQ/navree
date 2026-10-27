//! MongoDB migrations in an app: modules contribute sets, the migrator
//! applies them in batches (in a transaction when a migration asks),
//! records them, rolls them back, refreshes, starts fresh, pretends, and
//! runs from the `mongo:migrate` commands and at boot.

use navree_kernel::{Module, ModuleBuilder};
use navree_mongo::{
    ModuleBuilderMongoExt, MongoDb, MongoError, MongoMigrateOptions, MongoMigrationSet,
    MongoMigrator, MongoModule, MongoOptions, MongoRollbackOptions, mongo_migration,
};
use navree_orm::Model;
use navree_testing::TestApp;

use crate::models::Company;
use crate::support::TestMongo;

mod m20261027_000001_create_companies {
    use navree_mongo::{MongoMigration, MongoResult, MongoSchema};

    pub struct Migration;

    impl MongoMigration for Migration {
        async fn up(&self, schema: &MongoSchema) -> MongoResult {
            schema
                .create("companies", |t| {
                    t.object_id_primary();
                    t.string("name");
                    t.string("country").nullable();
                    t.integer("employees");
                    t.timestamps_tz();
                })
                .await
        }

        async fn down(&self, schema: &MongoSchema) -> MongoResult {
            schema.drop_if_exists("companies").await
        }
    }
}

mod m20261027_000002_seed_companies {
    use navree_mongo::{MongoError, MongoMigration, MongoResult, MongoSchema};
    use navree_orm::Model;

    use crate::models::Company;

    pub struct Migration;

    impl MongoMigration for Migration {
        async fn up(&self, schema: &MongoSchema) -> MongoResult {
            // Pretending, a data migration writes nothing (and has no
            // transaction).
            if schema.is_pretend() {
                return Ok(());
            }
            let uow = schema
                .uow()
                .ok_or_else(|| MongoError::Custom("a data migration runs in a transaction".into()))?;
            Company::create(
                uow,
                Company {
                    name: "Acme".into(),
                    employees: 12,
                    ..Company::default()
                },
            )
            .await?;
            Ok(())
        }

        async fn down(&self, schema: &MongoSchema) -> MongoResult {
            if let Some(uow) = schema.uow() {
                Company::query().delete(uow).await?;
            }
            Ok(())
        }

        fn within_transaction(&self) -> bool {
            true
        }
    }
}

mod m20261027_000003_create_deals {
    use navree_mongo::{MongoMigration, MongoResult, MongoSchema};

    pub struct CreateDeals;

    impl MongoMigration for CreateDeals {
        async fn up(&self, schema: &MongoSchema) -> MongoResult {
            schema
                .create("deals", |t| {
                    t.object_id_primary();
                    t.object_id("company_id").index();
                    t.string("title");
                })
                .await
        }

        async fn down(&self, schema: &MongoSchema) -> MongoResult {
            schema.drop_if_exists("deals").await
        }
    }
}

mod m20261027_000004_refuse {
    use navree_mongo::{MongoError, MongoMigration, MongoResult, MongoSchema};
    use navree_orm::Model;

    use crate::models::Company;

    pub struct Migration;

    impl MongoMigration for Migration {
        async fn up(&self, schema: &MongoSchema) -> MongoResult {
            if let Some(uow) = schema.uow() {
                Company::create(
                    uow,
                    Company {
                        name: "Half-done".into(),
                        ..Company::default()
                    },
                )
                .await?;
            }
            Err(MongoError::Custom("the data is not ready".into()))
        }

        async fn down(&self, _schema: &MongoSchema) -> MongoResult {
            Ok(())
        }

        fn within_transaction(&self) -> bool {
            true
        }
    }
}

/// Contributes the CRM's migrations (and, `broken`, one that fails).
struct Migrations {
    broken: bool,
}

impl Module for Migrations {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mongo_migrations(
            MongoMigrationSet::new("deals")
                .after(["crm"])
                .migration(mongo_migration!(m20261027_000003_create_deals::CreateDeals)),
        );
        m.mongo_migrations(
            MongoMigrationSet::new("crm")
                .migration(mongo_migration!(m20261027_000002_seed_companies))
                .migration(mongo_migration!(m20261027_000001_create_companies)),
        );
        if self.broken {
            m.mongo_migrations(
                MongoMigrationSet::new("zz_broken")
                    .after(["crm"])
                    .migration(mongo_migration!(m20261027_000004_refuse)),
            );
        }
    }
}

async fn boot(options: MongoOptions, broken: bool) -> TestApp {
    TestApp::builder()
        .module(MongoModule::for_root(options))
        .module(Migrations { broken })
        .build()
        .await
}

const ALL: [&str; 3] = [
    "crm/m20261027_000001_create_companies",
    "crm/m20261027_000002_seed_companies",
    "deals/m20261027_000003_create_deals",
];

#[tokio::test]
async fn migrations_apply_record_and_roll_back_in_batches() {
    let Some(t) = TestMongo::connect("migrations_apply_record_and_roll_back_in_batches").await else {
        return;
    };
    let app = boot(t.options(), false).await;
    let mongo = app.get::<MongoDb>().await;
    let migrator = MongoMigrator::of(app.ctx()).await.unwrap();
    let status = migrator.status().await.unwrap();
    assert_eq!(status.len(), 3);
    assert!(status.iter().all(|migration| !migration.applied));

    let report = migrator.up(&MongoMigrateOptions::default()).await.unwrap();
    assert_eq!(report.labels(), ALL, "sets after their dependencies, migrations by name");
    assert!(report.migrations[0].statements[0].starts_with(r#"db.runCommand({"create": "companies""#));
    assert_eq!(Company::query().count(&*mongo).await.unwrap(), 1, "the data migration committed");
    let status = migrator.status().await.unwrap();
    assert!(status.iter().all(|migration| migration.applied && migration.batch == Some(1)));
    assert!(status[0].applied_at.is_some());
    assert!(migrator.up(&MongoMigrateOptions::default()).await.unwrap().migrations.is_empty());

    // The last batch rolls back, newest first.
    let reverted = migrator.rollback(&MongoRollbackOptions::default()).await.unwrap();
    assert_eq!(reverted.labels(), [ALL[2], ALL[1], ALL[0]]);
    let schema = mongo.schema();
    assert!(!schema.has_table("companies").await.unwrap());
    assert!(migrator.status().await.unwrap().iter().all(|migration| !migration.applied));

    // Step by step: a batch each.
    migrator
        .up(&MongoMigrateOptions {
            step: true,
            ..MongoMigrateOptions::default()
        })
        .await
        .unwrap();
    let batches: Vec<Option<i64>> = migrator
        .status()
        .await
        .unwrap()
        .iter()
        .map(|migration| migration.batch)
        .collect();
    assert_eq!(batches, [Some(1), Some(2), Some(3)]);
    let one = migrator
        .rollback(&MongoRollbackOptions {
            step: Some(1),
            ..MongoRollbackOptions::default()
        })
        .await
        .unwrap();
    assert_eq!(one.labels(), [ALL[2]]);
    assert!(!schema.has_table("deals").await.unwrap());
    let batch_two = migrator
        .rollback(&MongoRollbackOptions {
            batch: Some(2),
            ..MongoRollbackOptions::default()
        })
        .await
        .unwrap();
    assert_eq!(batch_two.labels(), [ALL[1]]);
    assert_eq!(Company::query().count(&*mongo).await.unwrap(), 0, "the seed was undone");

    // Refresh: everything back, then forward.
    let (reverted, migrated) = migrator.refresh(None).await.unwrap();
    assert_eq!(reverted.labels(), [ALL[0]]);
    assert_eq!(migrated.labels(), ALL);
    // Fresh: every collection dropped, every migration in batch 1.
    mongo.collection("stray").insert_one(bson::doc! { "x": 1 }).await.unwrap();
    let (dropped, migrated) = migrator.fresh().await.unwrap();
    assert!(dropped.contains(&"stray".to_owned()) && dropped.contains(&"navree_migrations".to_owned()));
    assert_eq!(migrated.labels(), ALL);
    assert!(!schema.has_table("stray").await.unwrap());
    let reset = migrator.reset(false).await.unwrap();
    assert_eq!(reset.labels(), [ALL[2], ALL[1], ALL[0]]);
    t.cleanup().await;
}

#[tokio::test]
async fn pretending_changes_nothing() {
    let Some(t) = TestMongo::connect("pretending_changes_nothing").await else {
        return;
    };
    let app = boot(t.options(), false).await;
    let mongo = app.get::<MongoDb>().await;
    let migrator = MongoMigrator::of(app.ctx()).await.unwrap();
    let report = migrator
        .up(&MongoMigrateOptions {
            pretend: true,
            ..MongoMigrateOptions::default()
        })
        .await
        .unwrap();
    assert_eq!(report.labels(), ALL);
    assert!(report.migrations[2].statements.iter().any(|statement| statement.contains("createIndexes")));
    assert!(mongo.schema().collections().await.unwrap().is_empty());
    assert!(migrator.status().await.unwrap().iter().all(|migration| !migration.applied));
    t.cleanup().await;
}

#[tokio::test]
async fn a_failed_migration_rolls_back_its_transaction_and_is_not_recorded() {
    let Some(t) = TestMongo::connect("a_failed_migration_rolls_back_its_transaction_and_is_not_recorded").await
    else {
        return;
    };
    let app = boot(t.options(), true).await;
    let mongo = app.get::<MongoDb>().await;
    let migrator = MongoMigrator::of(app.ctx()).await.unwrap();
    let error = migrator.up(&MongoMigrateOptions::default()).await.unwrap_err();
    assert!(matches!(error, MongoError::Migration { .. }), "{error:?}");
    assert_eq!(
        error.to_string(),
        "migration `zz_broken/m20261027_000004_refuse` failed: the data is not ready"
    );
    let names: Vec<String> = Company::query().pluck(&*mongo, Company::NAME).await.unwrap();
    assert_eq!(names, ["Acme"], "the failed migration's write rolled back");
    let status = migrator.status().await.unwrap();
    let applied: Vec<bool> = status.iter().map(|migration| migration.applied).collect();
    assert_eq!(applied, [true, true, true, false], "the ones before it stay applied");
    t.cleanup().await;
}

#[tokio::test]
async fn commands_and_boot_run_the_migrations() {
    let Some(t) = TestMongo::connect("commands_and_boot_run_the_migrations").await else {
        return;
    };
    let app = boot(t.options(), false).await;
    let mongo = app.get::<MongoDb>().await;
    app.run_command("mongo:migrate:status", &[]).await.unwrap();
    app.run_command("mongo:migrate", &["--pretend"]).await.unwrap();
    assert!(!mongo.schema().has_table("companies").await.unwrap());
    app.run_command("mongo:migrate", &[]).await.unwrap();
    assert!(mongo.schema().has_table("deals").await.unwrap());
    app.run_command("mongo:migrate:rollback", &["--step", "1"]).await.unwrap();
    assert!(!mongo.schema().has_table("deals").await.unwrap());
    app.run_command("mongo:migrate:refresh", &[]).await.unwrap();
    app.run_command("mongo:migrate:fresh", &[]).await.unwrap();
    app.run_command("mongo:migrate:reset", &[]).await.unwrap();
    assert!(!mongo.schema().has_table("companies").await.unwrap());
    let error = app.run_command("mongo:migrate:rollback", &["--step", "x"]).await.unwrap_err();
    assert!(error.to_string().contains("--step"), "{error}");
    drop(app);

    // With migrate_on_boot, the app migrates while it boots.
    let options = MongoOptions {
        migrate_on_boot: true,
        ..t.options()
    };
    let booted = boot(options, false).await;
    let mongo = booted.get::<MongoDb>().await;
    assert!(mongo.schema().has_table("deals").await.unwrap());
    assert_eq!(Company::query().count(&*mongo).await.unwrap(), 1);
    t.cleanup().await;
}
