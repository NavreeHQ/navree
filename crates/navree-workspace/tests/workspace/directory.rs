//! The user directory over a table: users by id and by email address.

use navree_workspace::{TableUserDirectory, UserDirectory};
use uuid::Uuid;

use crate::support::World;

#[tokio::test]
async fn users_are_found_by_id_and_by_email() {
    let world = World::new().await;
    let asha = world.user("Asha Kaur", "asha@example.com").await;
    let rohit = world.user("Rohit Sharma", "rohit@example.com").await;
    let directory = TableUserDirectory::new(world.db().await, Some("users".to_owned()));

    let mut found = directory
        .users(&[asha, rohit, Uuid::now_v7()])
        .await
        .unwrap();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        found
            .iter()
            .map(|user| (user.id, user.name.as_str(), user.email.as_str()))
            .collect::<Vec<_>>(),
        [
            (asha, "Asha Kaur", "asha@example.com"),
            (rohit, "Rohit Sharma", "rohit@example.com"),
        ]
    );
    assert!(directory.users(&[]).await.unwrap().is_empty());

    // The address is trimmed and lower-cased before it is looked up.
    let by_email = directory
        .user_by_email("  Rohit@Example.com ")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_email.id, rohit);
    assert_eq!(by_email.name, "Rohit Sharma");
    assert!(
        directory
            .user_by_email("nobody@example.com")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_directory_without_its_table_knows_no_one() {
    let world = World::new().await;
    let asha = world.user("Asha Kaur", "asha@example.com").await;
    let db = world.db().await;

    for table in [None, Some("no_such_users".to_owned())] {
        let directory = TableUserDirectory::new(db.clone(), table);
        assert!(directory.users(&[asha]).await.unwrap().is_empty());
        assert!(
            directory
                .user_by_email("asha@example.com")
                .await
                .unwrap()
                .is_none()
        );
    }
}
