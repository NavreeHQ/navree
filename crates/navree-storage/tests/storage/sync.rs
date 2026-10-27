//! `Storage::sync` and the `storage:sync` command.

use navree_kernel::Navree;
use navree_storage::{Storage, StorageError, StorageModule, SyncOptions};

use crate::support::{options, storage};

#[tokio::test]
async fn sync_copies_what_is_missing_or_different() {
    let root = tempfile::tempdir().unwrap();
    let storage = storage(root.path());
    let memory = storage.disk("memory").unwrap();
    let local = storage.disk("local").unwrap();
    memory.put("reports/q1.csv", "a,b\n").await.unwrap();
    memory.put("reports/q2.csv", "a,b,c\n").await.unwrap();
    memory.put("avatars/me.png", "png").await.unwrap();
    local.put("reports/q1.csv", "x,y\n").await.unwrap(); // same size: unchanged
    local.put("reports/q2.csv", "old").await.unwrap(); // other size: copied
    local.put("reports/old.csv", "gone").await.unwrap();

    // A dry run changes nothing and says what would happen.
    let dry = storage
        .sync(
            "memory",
            "local",
            &SyncOptions {
                prefix: "reports".into(),
                delete: true,
                dry_run: true,
            },
        )
        .await
        .unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.copied, ["reports/q2.csv"]);
    assert_eq!(dry.deleted, ["reports/old.csv"]);
    assert_eq!((dry.unchanged, dry.bytes), (1, 6));
    assert_eq!(local.get("reports/q2.csv").await.unwrap(), "old");
    assert!(local.exists("reports/old.csv").await.unwrap());

    // The prefix limits the sync; without `delete` nothing is removed.
    let report = storage
        .sync(
            "memory",
            "local",
            &SyncOptions {
                prefix: "reports".into(),
                ..SyncOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (report.from.as_str(), report.to.as_str()),
        ("memory", "local")
    );
    assert_eq!(report.copied, ["reports/q2.csv"]);
    assert!(report.deleted.is_empty());
    assert_eq!(local.get("reports/q2.csv").await.unwrap(), "a,b,c\n");
    assert!(local.exists("reports/old.csv").await.unwrap());
    assert!(!local.exists("avatars/me.png").await.unwrap());

    let everything = storage
        .sync("memory", "local", &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(everything.copied, ["avatars/me.png"]);
    assert_eq!(everything.unchanged, 2);
}

#[tokio::test]
async fn failures_are_reported_and_the_sync_goes_on() {
    let root = tempfile::tempdir().unwrap();
    let storage = storage(root.path());
    let memory = storage.disk("memory").unwrap();
    memory
        .put("taken", "a file where the target has a directory")
        .await
        .unwrap();
    memory.put("fine.txt", "ok").await.unwrap();
    std::fs::create_dir_all(root.path().join("taken").join("inside")).unwrap();
    std::fs::write(root.path().join("taken").join("inside").join("x"), "x").unwrap();

    let report = storage
        .sync("memory", "local", &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(report.copied, ["fine.txt"]);
    assert_eq!(report.failed.len(), 1, "{report:?}");
    assert_eq!(report.failed[0].path, "taken");
}

#[tokio::test]
async fn sync_needs_two_known_disks() {
    let root = tempfile::tempdir().unwrap();
    let storage = storage(root.path());
    assert!(matches!(
        storage.sync("memory", "memory", &SyncOptions::default()).await,
        Err(StorageError::InvalidOptions(message)) if message.contains("itself")
    ));
    assert!(matches!(
        storage.sync("memory", "s3", &SyncOptions::default()).await,
        Err(StorageError::UnknownDisk { disk }) if disk == "s3"
    ));
}

#[tokio::test]
async fn the_command_syncs_named_disks() {
    let root = tempfile::tempdir().unwrap();
    let app = Navree::app()
        .module(StorageModule::for_root(options(root.path())))
        .boot()
        .await
        .unwrap();
    let storage = app.ctx().get::<Storage>().await.unwrap();
    storage
        .disk("local")
        .unwrap()
        .put("a/b.txt", "hello")
        .await
        .unwrap();
    let args = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect();

    app.run_command("storage:sync", args(&["local", "memory", "--dry-run"]))
        .await
        .unwrap();
    assert!(
        !storage
            .disk("memory")
            .unwrap()
            .exists("a/b.txt")
            .await
            .unwrap()
    );

    app.run_command("storage:sync", args(&["local", "memory", "--prefix", "a"]))
        .await
        .unwrap();
    assert_eq!(
        storage
            .disk("memory")
            .unwrap()
            .get("a/b.txt")
            .await
            .unwrap(),
        "hello"
    );

    for bad in [
        &["local"][..],
        &["local", "memory", "--force"],
        &["local", "nope"],
    ] {
        let error = app
            .run_command("storage:sync", args(bad))
            .await
            .unwrap_err();
        assert!(!error.to_string().is_empty(), "{bad:?}");
    }
}
