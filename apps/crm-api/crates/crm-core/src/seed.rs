//! Seeding: the CRM's roles (always needed) and the four demo accounts,
//! shared by the `crm-demo` seeder and the test kit.

use std::collections::BTreeSet;
use std::sync::Arc;

use navree::BoxFuture;
use navree::access::AccessService;
use navree::auth::{AuthService, User};
use navree::db::{Db, Seeder, Uow};
use navree::prelude::*;

use crate::models;
use crate::roles::UserRole;

/// The name of the roles seeder: `db:seed crm-roles`.
pub const ROLES_SEEDER: &str = "crm-roles";

/// The password of every demo account.
pub const DEMO_PASSWORD: &str = "password123";

/// One of the demo accounts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoAccount {
    /// The display name.
    pub name: &'static str,
    /// The email address (log in with it and [`DEMO_PASSWORD`]).
    pub email: &'static str,
    /// The role.
    pub role: UserRole,
}

/// The demo accounts: an admin, a manager and two reps (the names
/// match crm-web's mock data).
pub const DEMO_ACCOUNTS: [DemoAccount; 4] = [
    DemoAccount {
        name: "Daljit Kaur",
        email: "admin@navree.test",
        role: UserRole::Admin,
    },
    DemoAccount {
        name: "Manpreet Gill",
        email: "manager@navree.test",
        role: UserRole::Manager,
    },
    DemoAccount {
        name: "Priya Verma",
        email: "rep1@navree.test",
        role: UserRole::Rep,
    },
    DemoAccount {
        name: "Tarun Bhatia",
        email: "rep2@navree.test",
        role: UserRole::Rep,
    },
];

/// Creates the CRM's roles that do not exist yet and makes each role's
/// permissions exactly [`UserRole::permissions`]. Running it again changes
/// nothing.
pub async fn seed_roles(access: &AccessService, db: &Db) -> Result<(), BoxError> {
    let existing: BTreeSet<String> = access
        .roles()
        .await?
        .into_iter()
        .map(|role| role.name)
        .collect();
    in_uow(db, |uow| async move {
        for role in UserRole::ALL {
            if !existing.contains(role.as_str()) {
                access
                    .create_role(&uow, role.as_str(), Some(role.description()))
                    .await?;
            }
            access
                .sync_role_permissions(&uow, role.as_str(), role.permissions().iter().copied())
                .await?;
        }
        Ok(())
    })
    .await
}

/// The demo accounts (created when missing, with a verified email and their
/// role), in [`DEMO_ACCOUNTS`] order. Seeds the roles first. Running it
/// again changes nothing.
pub async fn ensure_demo_accounts(ctx: &AppContext) -> Result<Vec<User>, BoxError> {
    let auth = ctx.get::<AuthService>().await?;
    let access = ctx.get::<AccessService>().await?;
    let db = ctx.get::<Db>().await?;
    seed_roles(&access, &db).await?;
    let mut users = Vec::with_capacity(DEMO_ACCOUNTS.len());
    for account in DEMO_ACCOUNTS {
        let user = match auth.find_by_email(account.email).await? {
            Some(user) => user,
            None => {
                let user = auth
                    .create_user(account.name, account.email, DEMO_PASSWORD)
                    .await?;
                // The demo addresses count as verified from the start.
                let verified_at = user.created_at;
                models::User::query()
                    .where_key(user.id)
                    .update(&*db, [models::User::EMAIL_VERIFIED_AT.set(verified_at)])
                    .await?;
                User {
                    email_verified_at: Some(verified_at),
                    ..user
                }
            }
        };
        let id = user.id;
        let access = access.clone();
        in_uow(&db, |uow| async move {
            access.assign_role(&uow, id, account.role.as_str()).await?;
            Ok(())
        })
        .await?;
        users.push(user);
    }
    Ok(users)
}

/// Runs `f` in a new unit of work: commits when it succeeds, rolls back
/// when it fails. Writes through the unit of work are audited.
pub async fn in_uow<F, Fut>(db: &Db, f: F) -> Result<(), BoxError>
where
    F: FnOnce(Uow) -> Fut,
    Fut: Future<Output = Result<(), BoxError>>,
{
    let uow = db.uow(None, None).await;
    match f(uow.clone()).await {
        Ok(()) => Ok(uow.commit().await?),
        Err(error) => {
            uow.rollback().await?;
            Err(error)
        }
    }
}

/// `db:seed crm-roles`: [`seed_roles`]. Registered by
/// [`CrmCoreModule`](crate::CrmCoreModule), so every environment can seed
/// the roles without the demo data.
#[injectable]
pub struct RolesSeeder {
    access: Arc<AccessService>,
}

impl Seeder for RolesSeeder {
    fn name(&self) -> &'static str {
        ROLES_SEEDER
    }

    fn run<'a>(&'a self, _ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(seed_roles(&self.access, db))
    }
}
