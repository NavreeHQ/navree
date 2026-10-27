//! Encrypted and hashed attributes: the app's encrypter and hasher reach
//! the casts, key rotation rewrites old payloads, models can name their
//! encrypter, and code outside an app sets the services explicitly.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use navree_crypto::{AppKey, Encrypter, Hasher, generate_key};
use navree_kernel::{Factory, Module, ModuleBuilder};
use navree_orm::attributes::CastContext;
use navree_orm::cast::is_password_hash;
use navree_orm::{Model, OrmError, Record, attrs};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::support::{crypto_app, crypto_app_with, fast_hashing, sqlite, stored_text};

const SCHEMA: &str = r#"
CREATE TABLE patients (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    ssn TEXT NOT NULL,
    notes TEXT NULL,
    history TEXT NOT NULL,
    scan TEXT NOT NULL,
    password TEXT NOT NULL,
    pin TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE vaults (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    secret TEXT NOT NULL
)
"#;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    pub on: NaiveDate,
    pub reason: String,
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "patients", timestamps, unguarded, hidden(password, pin))]
pub struct Patient {
    pub id: i64,
    pub name: String,
    #[model(cast = "encrypted")]
    pub ssn: String,
    #[model(cast = "encrypted")]
    pub notes: Option<String>,
    #[model(cast = "encrypted_json")]
    pub history: Vec<Visit>,
    #[model(cast = "encrypted")]
    pub scan: Vec<u8>,
    #[model(cast = "hashed")]
    pub password: String,
    #[model(cast = "hashed")]
    pub pin: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Encrypted with the app's `vault` encrypter.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "vaults", encrypter = "vault", unguarded)]
pub struct Vault {
    pub id: i64,
    #[model(cast = "encrypted")]
    pub secret: String,
}

fn patient() -> Patient {
    Patient {
        name: "Ada".into(),
        ssn: "078-05-1120".into(),
        notes: Some("allergic to penicillin".into()),
        history: vec![Visit {
            on: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            reason: "checkup".into(),
        }],
        scan: vec![0, 159, 146, 150],
        password: "correct horse battery staple".into(),
        ..Patient::default()
    }
}

#[tokio::test]
async fn encrypted_fields_are_stored_encrypted_and_read_plain() {
    let (app, db) = crypto_app(SCHEMA).await;
    let created = Patient::create(&*db, patient()).await.unwrap();
    let ssn = stored_text(&db, "patients", "ssn").await.unwrap();
    assert!(ssn.starts_with("v1."), "{ssn}");
    assert!(!ssn.contains("078"));
    let history = stored_text(&db, "patients", "history").await.unwrap();
    assert!(history.starts_with("v1.") && !history.contains("checkup"), "{history}");

    let stored = Patient::find_or_fail(&*db, created.id).await.unwrap();
    assert_eq!(stored.ssn, "078-05-1120");
    assert_eq!(stored.notes.as_deref(), Some("allergic to penicillin"));
    assert_eq!(stored.history, created.history);
    assert_eq!(stored.scan, [0, 159, 146, 150]);
    assert!(stored.is_clean(), "every write differs, the plain values do not");

    // The app's encrypter wrote them.
    let encrypter = app.get::<Encrypter>().await;
    assert_eq!(encrypter.decrypt_string(&ssn).unwrap(), "078-05-1120");

    let json = stored.to_value().unwrap();
    assert_eq!(json["ssn"], "078-05-1120", "serialized records hold the plain value");
    assert_eq!(json["history"], json!([{"on": "2026-03-01", "reason": "checkup"}]));
    assert!(json.get("password").is_none() && json.get("pin").is_none());

    let mut stored = stored;
    stored.notes = None;
    stored.save(&*db).await.unwrap();
    assert_eq!(stored_text(&db, "patients", "notes").await, None, "None is NULL");
    stored.ssn = "000-00-0000".into();
    assert!(stored.is_dirty_on(["ssn"]));
    stored.save(&*db).await.unwrap();
    assert_eq!(Patient::find_or_fail(&*db, created.id).await.unwrap().ssn, "000-00-0000");
}

#[tokio::test]
async fn hashed_fields_hold_the_hash_once_saved() {
    let (app, db) = crypto_app(SCHEMA).await;
    let hasher = app.get::<Hasher>().await;
    let mut created = Patient::create(&*db, Patient { pin: Some("1234".into()), ..patient() })
        .await
        .unwrap();
    assert!(is_password_hash(&created.password), "{}", created.password);
    assert!(hasher.verify("correct horse battery staple", &created.password).await.unwrap());
    assert!(hasher.verify("1234", created.pin.as_deref().unwrap()).await.unwrap());
    assert_eq!(stored_text(&db, "patients", "password").await.as_deref(), Some(created.password.as_str()));
    assert!(created.is_clean());

    let first = created.password.clone();
    created.save(&*db).await.unwrap();
    assert_eq!(created.password, first, "a hash is never hashed again");

    created.password = "new password".into();
    assert!(created.is_dirty_on(["password"]));
    let error = created.get_dirty().unwrap_err();
    assert!(error.to_string().contains("not hashed yet"), "{error}");
    created.save(&*db).await.unwrap();
    assert_ne!(created.password, first);
    assert!(hasher.verify("new password", &created.password).await.unwrap());
    let stored = Patient::find_or_fail(&*db, created.id).await.unwrap();
    assert_eq!(stored.password, created.password, "reading returns the hash");

    created.password = "$2y$10$abcdefghijklmnopqrstuuJ8Wbm3gx9z5HFRz8wE2PxDZtjVYZxq2".into();
    let error = created.save(&*db).await.unwrap_err();
    assert!(
        matches!(&error, OrmError::Cast { column, message, .. } if column == "password" && message.contains("bcrypt hash")),
        "{error:?}"
    );

    let filled = Patient::create_from(&*db, attrs! { "name" => "Grace", "ssn" => "1", "password" => "from a form" })
        .await
        .unwrap();
    assert!(hasher.verify("from a form", &filled.password).await.unwrap());
}

#[tokio::test]
async fn records_read_under_a_previous_key_are_re_encrypted_on_save() {
    let t = sqlite(SCHEMA).await;
    let old = AppKey::generate();
    let hasher = Hasher::new(&fast_hashing()).unwrap();
    let before = CastContext::new()
        .with_encrypter(Encrypter::new(old.clone()))
        .with_hasher(hasher.clone());
    let id = before
        .scope(Patient::create(&t.db, patient()))
        .await
        .unwrap()
        .id;
    let old_payload = stored_text(&t.db, "patients", "ssn").await.unwrap();

    let current = AppKey::generate();
    let current_kid = current.kid().to_owned();
    let rotated = CastContext::new()
        .with_encrypter(Encrypter::new(current).with_previous_keys([old]))
        .with_hasher(hasher);
    rotated
        .scope(async {
            let mut stored = Patient::find_or_fail(&t.db, id).await.unwrap();
            assert_eq!(stored.ssn, "078-05-1120", "previous keys still decrypt");
            assert!(stored.is_dirty_on(["ssn", "history", "scan", "notes"]));
            assert!(stored.is_clean_on(["name", "password"]));
            stored.save(&t.db).await.unwrap();
            assert!(stored.is_clean());
            let fresh = Patient::find_or_fail(&t.db, id).await.unwrap();
            assert!(fresh.is_clean(), "rewritten with the current key");
            assert_eq!(fresh.ssn, "078-05-1120");
        })
        .await;
    let new_payload = stored_text(&t.db, "patients", "ssn").await.unwrap();
    assert_ne!(new_payload, old_payload);
    assert!(new_payload.starts_with(&format!("v1.{current_kid}.")), "{new_payload}");
}

/// Registers the `vault` encrypter.
struct VaultKeys(AppKey);

impl Module for VaultKeys {
    fn register(&self, m: &mut ModuleBuilder) {
        let key = self.0.clone();
        m.factory(
            Factory::<Encrypter>::new(move |_| {
                let key = key.clone();
                async move { Ok(Arc::new(Encrypter::new(key))) }
            })
            .named("vault"),
        );
    }
}

#[tokio::test]
async fn a_model_can_name_its_encrypter() {
    let vault_key = AppKey::generate();
    let app = navree_testing::TestApp::builder()
        .temp_database()
        .module(navree_crypto::CryptoModule::for_root(navree_crypto::CryptoOptions::new(generate_key())))
        .module(navree_db::DatabaseModule::from_config())
        .module(VaultKeys(vault_key.clone()))
        .build()
        .await;
    let db = app.get::<navree_db::Db>().await;
    super::support::create(&db, SCHEMA).await;
    let vault = Vault::create(&*db, Vault { secret: "launch codes".into(), ..Vault::default() })
        .await
        .unwrap();
    let payload = stored_text(&db, "vaults", "secret").await.unwrap();
    assert!(app.get::<Encrypter>().await.decrypt_string(&payload).is_err(), "not the app's key");
    assert_eq!(Encrypter::new(vault_key).decrypt_string(&payload).unwrap(), "launch codes");
    assert_eq!(Vault::find_or_fail(&*db, vault.id).await.unwrap().secret, "launch codes");

    // An app without the named encrypter says what to register.
    let (_app, db) = crypto_app(SCHEMA).await;
    let error = Vault::create(&*db, Vault { secret: "x".into(), ..Vault::default() })
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("provides no Encrypter named `vault`"),
        "{error}"
    );
}

#[tokio::test]
async fn without_an_app_the_services_come_from_explicit_scopes() {
    let t = sqlite(SCHEMA).await;
    let error = Patient::create(&t.db, patient()).await.unwrap_err();
    assert!(
        matches!(&error, OrmError::Cast { column, message, .. } if column == "password" && message.contains("no Hasher")),
        "{error:?}"
    );
    let error = Patient::create(&t.db, Patient { password: "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$Tr6ffxUqo5TVOlcIZeVY0pIBn6bv7HJbNSuDn9t1Bzo".into(), ..patient() })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no Encrypter is available"), "{error}");

    let services = CastContext::new()
        .with_encrypter(Encrypter::new(AppKey::generate()))
        .with_hasher(Hasher::new(&fast_hashing()).unwrap());
    let created = services
        .clone()
        .scope(Patient::create(&t.db, patient()))
        .await
        .unwrap();
    let read = services
        .scope(Patient::find_or_fail(&t.db, created.id))
        .await
        .unwrap();
    assert_eq!(read.ssn, "078-05-1120");

    // An explicit scope wins over the app's services.
    let pinned = Encrypter::new(AppKey::generate());
    let (_app, db) = crypto_app_with(SCHEMA, generate_key(), &[]).await;
    CastContext::new()
        .with_encrypter(pinned.clone())
        .with_hasher(Hasher::new(&fast_hashing()).unwrap())
        .scope(Patient::create(&*db, patient()))
        .await
        .unwrap();
    let payload = stored_text(&db, "patients", "ssn").await.unwrap();
    assert_eq!(pinned.decrypt_string(&payload).unwrap(), "078-05-1120");
    let record = Record::new(patient());
    assert!(record.is_dirty(), "a new record is dirty without any service");
}
