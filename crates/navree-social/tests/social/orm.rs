//! Linked provider accounts as navree-orm models.

use navree_db::Db;
use navree_orm::Model as _;
use navree_social::{SocialUser, social_account};

use crate::support::{create_verified_user, fake_app, sign_in};

type Account = social_account::Model;

#[tokio::test]
async fn linked_accounts_are_orm_models() {
    let (app, fake) = fake_app(|b| b).await;
    let ada = create_verified_user(&app, "Anu", "anu@example.com").await;
    fake.on_code(
        "github",
        "code",
        SocialUser::new("github", "583231")
            .email(Some("anu@example.com"), true)
            .nickname("anu"),
    );
    sign_in(&app, "github", "code").await.assert_status(200);
    let db = app.get::<Db>().await;

    assert_eq!(Account::table_name(), "social_accounts");
    let linked = Account::query()
        .where_(Account::USER_ID.eq(ada.id))
        .where_(Account::PROVIDER.eq("github"))
        .first(&db)
        .await
        .unwrap()
        .expect("the linked account");
    assert_eq!(linked.provider_user_id, "583231");
    assert_eq!(linked.nickname.as_deref(), Some("anu"));
    let json = linked.to_value().unwrap();
    assert!(json.get("access_token").is_none(), "tokens never serialize");
    assert!(
        json.get("refresh_token").is_none(),
        "tokens never serialize"
    );
    assert_eq!(
        Account::query()
            .where_(Account::PROVIDER.eq("google"))
            .count(&db)
            .await
            .unwrap(),
        0
    );
}
