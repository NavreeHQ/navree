//! The LDAP source against a real OpenLDAP server: simple bind, paged
//! searches past the server's size limit, StartTLS and LDAPS with the server's
//! certificate, a refused bind, and the groups of the three OpenLDAP kinds
//! (`groupOfNames`, `groupOfUniqueNames`, `posixGroup`). The server's entries
//! carry a real `entryUUID`, which an operational attribute only a real server
//! returns.
//!
//! These run in the integration workflow, which starts OpenLDAP with
//! `scripts/integration-services.sh start identity` (directory
//! `scripts/services/integrations/openldap/base.ldif`, plus 600 people under
//! `ou=bulk`); they are `#[ignore]`d in the other suites. Run them with
//! `-- --ignored`. The variables:
//!
//! - `NAVREE_TEST_LDAP_URL` and `NAVREE_TEST_LDAPS_URL`: the server on 127.0.0.1
//!   without TLS (StartTLS is offered) and over TLS;
//! - `NAVREE_TEST_LDAP_CA`: the path of the server's certificate (PEM), which
//!   names `localhost` and 127.0.0.1;
//! - `NAVREE_TEST_LDAP_BIND_DN` and `NAVREE_TEST_LDAP_PASSWORD`: the
//!   administrator's login.
//!
//! A missing variable fails the test when `NAVREE_TEST_REQUIRE_SERVICES=1` and
//! prints `SKIPPED` otherwise.
//!
//! What stays a mock: Active Directory's own behaviour (`objectGUID`,
//! `memberOf`, referrals, its paging limits), which OpenLDAP does not have. A
//! Samba AD domain controller would need a container with a domain
//! provisioned at start; the fixture server in `ldap_server.rs` keeps covering
//! the Active Directory flavor.

use navree_directory::{RunStatus, SyncOptions};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::sources::{ldap_connection, loopback_ldap, synced};
use crate::support::{Harness, real};

const BASE_DN: &str = "dc=acme,dc=test";
/// The people of `base.ldif` (the 600 in `ou=bulk` come on top).
const NAMED: usize = 4;
const BULK: usize = 600;

/// The value of `variable`; `None` after a notice, or a panic when
/// `NAVREE_TEST_REQUIRE_SERVICES=1`.
fn service(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: {variable} is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

/// The server's settings: the URL variable named, the login, and `extra`.
fn settings(url: &str, extra: Value, bind_dn: &str) -> Value {
    let mut settings = json!({
        "url": url,
        "base_dn": BASE_DN,
        "bind_dn": bind_dn,
        "flavor": "openldap",
        "page_size": 100,
        "attributes": ["departmentNumber"],
    });
    for (key, value) in extra.as_object().into_iter().flatten() {
        settings[key] = value.clone();
    }
    settings
}

/// Syncs a connection with `settings` and `password`; the run must succeed.
async fn sync(h: &Harness, settings: Value, password: &str) -> Uuid {
    let id = ldap_connection(h, settings, Some(password)).await;
    let run = h
        .directory()
        .await
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded, "{run:?}");
    id
}

fn pem(test: &str) -> Option<String> {
    let path = service("NAVREE_TEST_LDAP_CA", test)?;
    Some(
        std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("{test}: cannot read the certificate at {path}: {error}")
        }),
    )
}

/// The users and groups the directory holds after a sync: sign-in names, and
/// the members of each group by sign-in name.
async fn read_back(h: &Harness, id: Uuid) -> (Vec<String>, Vec<(String, Vec<String>)>) {
    let directory = h.directory().await;
    let (users, groups) = synced(&directory, id).await;
    let name_of = |external_id: &str| {
        users
            .iter()
            .find(|user| user.0 == external_id)
            .map(|user| user.1.clone())
            .unwrap_or_else(|| format!("unknown member {external_id}"))
    };
    let mut names: Vec<String> = users.iter().map(|user| user.1.clone()).collect();
    names.sort();
    let mut groups: Vec<(String, Vec<String>)> = groups
        .iter()
        .map(|group| {
            let mut members: Vec<String> = group.2.iter().map(|id| name_of(id)).collect();
            members.sort();
            (group.1.clone(), members)
        })
        .collect();
    groups.sort();
    (names, groups)
}

/// What the three groups of `base.ldif` hold, by sign-in name.
fn expected_groups() -> Vec<(String, Vec<String>)> {
    let names = |list: &[&str]| list.iter().map(|name| (*name).to_owned()).collect();
    vec![
        (
            "engineering".to_owned(),
            names(&["gurpreet.kaur", "harpreet.singh"]),
        ),
        (
            "operations".to_owned(),
            names(&["manpreet.gill", "simran.sandhu"]),
        ),
        (
            "support".to_owned(),
            names(&["harpreet.singh", "simran.sandhu"]),
        ),
    ]
}

/// Checks the directory read from OpenLDAP.
async fn check(h: &Harness, id: Uuid) {
    let (names, groups) = read_back(h, id).await;
    assert_eq!(
        names.len(),
        NAMED + BULK,
        "every person, past the size limit"
    );
    for named in [
        "gurpreet.kaur",
        "harpreet.singh",
        "manpreet.gill",
        "simran.sandhu",
    ] {
        assert!(names.iter().any(|name| name == named), "{named} is missing");
    }
    assert!(
        names.iter().any(|name| name == "user0600"),
        "the last bulk user"
    );
    assert_eq!(groups, expected_groups());

    let directory = h.directory().await;
    let users = directory.users(id).await.unwrap();
    let gurpreet = users
        .iter()
        .find(|u| u.user.user_name == "gurpreet.kaur")
        .expect("Gurpreet");
    assert_eq!(gurpreet.user.full_name(), "Gurpreet Kaur");
    assert_eq!(
        gurpreet.user.primary_email(),
        Some("gurpreet.kaur@acme.test")
    );
    assert_eq!(gurpreet.user.title.as_deref(), Some("Principal engineer"));
    assert_eq!(
        gurpreet.user.attributes["departmentNumber"],
        json!("Engineering")
    );
    // `entryUUID` is the id: an operational attribute the server gave because
    // the source asked for it by name.
    assert!(
        Uuid::parse_str(&gurpreet.user.external_id).is_ok(),
        "{}",
        gurpreet.user.external_id
    );
}

fn login(test: &str) -> Option<(String, String)> {
    Some((
        service("NAVREE_TEST_LDAP_BIND_DN", test)?,
        service("NAVREE_TEST_LDAP_PASSWORD", test)?,
    ))
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn openldap_gives_users_and_groups_through_paged_searches() {
    let test = "openldap_gives_users_and_groups_through_paged_searches";
    let (Some(url), Some((bind_dn, password))) =
        (service("NAVREE_TEST_LDAP_URL", test), login(test))
    else {
        return;
    };
    let h = real(loopback_ldap).await;

    let id = sync(&h, settings(&url, json!({}), &bind_dn), &password).await;

    check(&h, id).await;
    // A second sync changes nothing.
    let again = h
        .directory()
        .await
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(again.status, RunStatus::Succeeded, "{again:?}");
    assert_eq!(read_back(&h, id).await.0.len(), NAMED + BULK);
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn openldap_is_read_after_starttls_with_the_servers_certificate() {
    let test = "openldap_is_read_after_starttls_with_the_servers_certificate";
    let (Some(url), Some((bind_dn, password)), Some(ca)) = (
        service("NAVREE_TEST_LDAP_URL", test),
        login(test),
        pem(test),
    ) else {
        return;
    };
    let h = real(loopback_ldap).await;

    let id = sync(
        &h,
        settings(
            &url,
            json!({ "starttls": true, "ca_certificate": ca }),
            &bind_dn,
        ),
        &password,
    )
    .await;

    check(&h, id).await;

    // Without the certificate the handshake fails, and the password is not
    // sent in plain text instead.
    let untrusted = ldap_connection(
        &h,
        settings(&url, json!({ "starttls": true }), &bind_dn),
        Some(&password),
    )
    .await;
    let run = h
        .directory()
        .await
        .sync_now(untrusted, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed, "{run:?}");
    assert!(
        !run.error.unwrap_or_default().contains(&password),
        "the password is not echoed"
    );
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn openldap_is_read_over_ldaps_with_the_servers_certificate() {
    let test = "openldap_is_read_over_ldaps_with_the_servers_certificate";
    let (Some(url), Some((bind_dn, password)), Some(ca)) = (
        service("NAVREE_TEST_LDAPS_URL", test),
        login(test),
        pem(test),
    ) else {
        return;
    };
    let h = real(loopback_ldap).await;

    let id = sync(
        &h,
        settings(&url, json!({ "ca_certificate": ca }), &bind_dn),
        &password,
    )
    .await;

    check(&h, id).await;

    let untrusted = ldap_connection(&h, settings(&url, json!({}), &bind_dn), Some(&password)).await;
    let run = h
        .directory()
        .await
        .sync_now(untrusted, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed, "{run:?}");
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_wrong_password_fails_the_run_without_echoing_it() {
    let test = "a_wrong_password_fails_the_run_without_echoing_it";
    let (Some(url), Some((bind_dn, _))) = (service("NAVREE_TEST_LDAP_URL", test), login(test))
    else {
        return;
    };
    let h = real(loopback_ldap).await;
    let id = ldap_connection(
        &h,
        settings(&url, json!({}), &bind_dn),
        Some("not-the-password"),
    )
    .await;

    let run = h
        .directory()
        .await
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();

    assert_eq!(run.status, RunStatus::Failed, "{run:?}");
    let error = run.error.unwrap();
    // 49: invalid credentials.
    assert!(
        error.contains(&format!("the bind as {bind_dn} failed: result code 49")),
        "{error}"
    );
    assert!(!error.contains("not-the-password"), "{error}");
}
