//! What navree-sso logs about connections owned by workspaces and
//! organizations that have `allow_unverified_emails` on.
//!
//! A test binary of its own: `tracing` caches per-callsite interest
//! process-wide, so events first hit by other tests' threads (with no
//! subscriber) could be skipped here if they ran in this process.

#[path = "sso/servers.rs"]
mod servers;
#[path = "sso/support.rs"]
mod support;

use std::sync::{Arc, Mutex};

use navree_sso::{ConnectionUpdate, Provisioning, SsoFake, SsoOwner};
use navree_workspace::{NewOrganization, NewWorkspace, Organizations, Workspaces};
use uuid::Uuid;

use crate::support::World;

/// Captures formatted tracing output written while the guard is alive (on
/// this thread: use a current-thread runtime).
#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(self.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }

    fn lines(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.0.lock().unwrap())
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl std::io::Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Provisioning that creates accounts outside the connection's verified
/// domains.
fn open() -> Provisioning {
    Provisioning {
        allow_unverified_emails: true,
        ..Provisioning::default()
    }
}

/// A workspace of the verified user Gurpreet Singh.
async fn workspace_of(world: &World) -> Uuid {
    let owner = world
        .create_verified_user("Gurpreet Singh", "gurpreet@ludhiana.test")
        .await;
    let workspaces = world.app.get::<Workspaces>().await;
    world
        .write(|uow| async move {
            workspaces
                .create(&uow, owner.id, NewWorkspace::named("Ludhiana Traders"))
                .await
        })
        .await
        .expect("the workspace is created")
        .id
}

/// The lines of `lines` that contain every one of `parts`.
fn lines_with<'a>(lines: &'a [String], parts: &[&str]) -> Vec<&'a String> {
    lines
        .iter()
        .filter(|line| parts.iter().all(|part| line.contains(*part)))
        .collect()
}

/// Boots an app on the SQLite database at `url` and saves four connections
/// in it: a workspace's (`ludhiana-okta`), an organization's
/// (`punjab-entra`) and the app's (`staff-okta`) with
/// `allow_unverified_emails` on, and the app's `plain-okta` without it.
async fn seed(url: &str) {
    let world = support::world_with(support::builder().config("database.url", url)).await;
    let fake = SsoFake::new();
    let owner = world
        .create_verified_user("Gurpreet Singh", "gurpreet@ludhiana.test")
        .await
        .id;
    let workspaces = world.app.get::<Workspaces>().await;
    let workspace = world
        .write(|uow| async move {
            workspaces
                .create(&uow, owner, NewWorkspace::named("Ludhiana Traders"))
                .await
        })
        .await
        .expect("the workspace is created");
    let organizations = world.app.get::<Organizations>().await;
    let organization = world
        .write(|uow| async move {
            organizations
                .create(&uow, owner, NewOrganization::named("Punjab Group"))
                .await
        })
        .await
        .expect("the organization is created");
    world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace.id),
            "ludhiana-okta",
            open(),
        )
        .await;
    world
        .saml_connection(
            &fake,
            SsoOwner::Organization(organization.id),
            "punjab-entra",
            open(),
        )
        .await;
    world
        .saml_connection(&fake, SsoOwner::App, "staff-okta", open())
        .await;
    world
        .saml_connection(&fake, SsoOwner::App, "plain-okta", Provisioning::default())
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn repeated_refusals_through_one_connection_log_one_line() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let world = support::world().await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace),
            "ludhiana-okta",
            open(),
        )
        .await;
    for email in [
        "simran.kaur@gmail.test",
        "gurpreet.singh@gmail.test",
        "harpreet@gmail.test",
    ] {
        world
            .saml_sign_in(&fake, "ludhiana-okta", email, &[])
            .await
            .assert_error_code(403, "unverified_domain");
    }
    let refused: Vec<String> = capture
        .lines()
        .into_iter()
        .filter(|l| {
            l.contains("WARN") && l.contains("navree::security") && l.contains("ludhiana-okta")
        })
        .collect();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].contains("refusals=1"));
    assert!(refused[0].contains("sso.tenants.unverified_emails"));
}

#[tokio::test(flavor = "current_thread")]
async fn a_customer_connection_without_the_flag_refuses_quietly_as_before() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let world = support::world().await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace),
            "ludhiana-okta",
            Provisioning::default(),
        )
        .await;
    world
        .saml_sign_in(&fake, "ludhiana-okta", "simran.kaur@gmail.test", &[])
        .await
        .assert_error_code(403, "unverified_domain");
    let lines = capture.lines();
    assert!(lines_with(&lines, &["WARN", "ludhiana-okta"]).is_empty());
    let actions = world.audit.actions();
    assert!(!actions.contains(&"sso.login_refused".to_owned()));
}

#[tokio::test(flavor = "current_thread")]
async fn the_boot_names_customer_connections_with_the_flag() {
    if servers::Server::current() != servers::Server::Sqlite {
        return;
    }
    // One SQLite file for two boots.
    let (url, _database) = navree_testing::temp_sqlite_url();
    seed(&url).await;
    let capture = LogCapture::default();
    let _guard = capture.install();
    let builder = support::builder().config("database.url", url.as_str());
    let world = support::world_with(builder).await;
    let lines = capture.lines();
    let warnings = lines_with(&lines, &["WARN", "allow_unverified_emails on"]);
    let warn = warnings.first().expect("a warning");
    assert!(warn.contains("ludhiana-okta (workspace "), "{warn}");
    assert!(warn.contains("punjab-entra (organization "), "{warn}");
    assert!(!warn.contains("staff-okta"), "{warn}");
    assert!(!warn.contains("plain-okta"), "{warn}");
    let app = lines_with(&lines, &["INFO", "staff-okta owned by the app"]);
    assert!(!app.is_empty(), "{lines:#?}");
    drop(world);
}

#[tokio::test(flavor = "current_thread")]
async fn with_the_tenants_key_the_boot_says_so_at_info() {
    if servers::Server::current() != servers::Server::Sqlite {
        return;
    }
    let (url, _database) = navree_testing::temp_sqlite_url();
    seed(&url).await;
    let capture = LogCapture::default();
    let _guard = capture.install();
    let builder = support::builder()
        .config("database.url", url.as_str())
        .config("sso.tenants.unverified_emails", true);
    let _world = support::world_with(builder).await;
    let lines = capture.lines();
    assert!(lines_with(&lines, &["WARN", "ludhiana-okta"]).is_empty());
    let allowed = "(sso.tenants.unverified_emails = true)";
    assert!(!lines_with(&lines, &["INFO", allowed]).is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn the_boot_says_nothing_without_the_flag_or_before_the_first_migration() {
    if servers::Server::current() != servers::Server::Sqlite {
        return;
    }
    let capture = LogCapture::default();
    let _guard = capture.install();
    let _plain = support::world().await;
    let lines = capture.lines();
    assert!(lines_with(&lines, &["allow_unverified_emails"]).is_empty());
    // Without migrations the table doesn't exist: the app boots, and the
    // check says so at debug only.
    let unmigrated = support::builder().config("database.migrate_on_boot", false);
    let _unmigrated = support::world_with(unmigrated).await;
    let lines = capture.lines();
    let failed = "could not check the single sign-on connections at boot";
    assert!(lines_with(&lines, &[failed]).is_empty());
    let not_yet = "their table doesn't exist yet";
    assert!(!lines_with(&lines, &["DEBUG", not_yet]).is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn saving_a_connection_with_the_flag_is_logged_by_owner() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let world = support::world().await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace),
            "ludhiana-okta",
            open(),
        )
        .await;
    world
        .saml_connection(&fake, SsoOwner::App, "staff-okta", open())
        .await;
    let sso = world.sso().await;
    let rename = ConnectionUpdate {
        name: Some("Ludhiana Login".to_owned()),
        ..ConnectionUpdate::default()
    };
    world
        .write(|uow| async move { sso.update_connection(&uow, "ludhiana-okta", rename).await })
        .await
        .expect("the connection is updated");
    let lines = capture.lines();
    let no_effect = lines_with(&lines, &["WARN", "ludhiana-okta", "has no effect"]);
    assert_eq!(no_effect.len(), 2, "{lines:#?}");
    let app = lines_with(&lines, &["INFO", "staff-okta owned by the app"]);
    assert!(!app.is_empty(), "{lines:#?}");
}
