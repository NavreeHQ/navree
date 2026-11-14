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

use navree_sso::{Provisioning, SsoFake, SsoOwner};
use navree_workspace::{NewWorkspace, Workspaces};
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
