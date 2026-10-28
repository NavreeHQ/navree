//! The `evidence` seeder: `evidence-app db:seed evidence` (or
//! `migrate:fresh --seed`). It creates:
//!
//! - the operator [`OPERATOR_EMAIL`], a super admin (the tools open for
//!   them), owner of every seeded workspace;
//! - three workspaces ([`WORKSPACES`]) with projects and tasks;
//! - [`LOAD_USERS`] load-test accounts, `load-001@evidence.test` and up,
//!   members of the first workspace, so each k6 virtual user signs in as
//!   its own account.
//!
//! Every account uses [`PASSWORD`]. Running it again changes nothing: what
//! exists is found again.

use navree::access::AccessService;
use navree::auth::{AuthService, User};
use navree::db::{Db, Seeder, Uow};
use navree::workspace::{
    Members, NewWorkspace, Workspace, WorkspaceRepo, WorkspaceRole, Workspaces,
};
use navree::{BoxFuture, prelude::*};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, IntoActiveModel, PaginatorTrait};
use uuid::Uuid;

use crate::abilities::SUPER_ADMIN;
use crate::modules::projects::entity::{ActiveModel as ProjectModel, Project};
use crate::modules::tasks::Task;

/// The seeder's name.
pub const SEEDER: &str = "evidence";

/// The password of every seeded account.
pub const PASSWORD: &str = "evidence-password";

/// The operator: a super admin who owns every seeded workspace.
pub const OPERATOR_EMAIL: &str = "operator@evidence.test";

/// The seeded workspaces: slug and name. The load profiles work in the
/// first.
pub const WORKSPACES: &[(&str, &str)] = &[
    ("acme", "Acme Logistics"),
    ("globex", "Globex Research"),
    ("initech", "Initech Software"),
];

/// How many load-test accounts the seeder creates (members of `acme`).
pub const LOAD_USERS: usize = 200;

/// Project names, cycled through per workspace.
const PROJECTS: &[(&str, &str)] = &[
    ("Website relaunch", "New pages, new pricing, one launch date."),
    ("Mobile app", "Sign-in, projects and notifications on the phone."),
    ("Onboarding emails", "Five emails over a customer's first two weeks."),
    ("Warehouse scanners", "Replace the handheld scanners at the north depot."),
    ("Quarterly planning", "Goals, owners and budgets for next quarter."),
    ("Billing migration", "Move every plan to usage-based invoices."),
    ("Support handbook", "Answers to the twenty most asked questions."),
    ("Data retention", "Delete what we no longer need, on a schedule."),
    ("Partner portal", "Self-serve access for resellers."),
    ("Security review", "Pen test findings and their fixes."),
    ("Route optimisation", "Shorter delivery routes from live traffic."),
    ("Hiring pipeline", "Three engineers and a designer by spring."),
];

/// Task titles, cycled through per project.
const TASKS: &[&str] = &[
    "Write the brief",
    "Agree the budget",
    "Draft the designs",
    "Review with the team",
    "Build the first version",
    "Test on real devices",
    "Fix what testing found",
    "Write the announcement",
    "Ship it",
    "Measure the results",
];

/// Registers the `evidence` seeder.
#[module(register = Self::wire)]
pub struct SeedModule;

impl SeedModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.seeder::<EvidenceSeeder>();
    }
}

/// `db:seed evidence`; see the [module docs](self).
#[injectable]
pub struct EvidenceSeeder;

impl Seeder for EvidenceSeeder {
    fn name(&self) -> &'static str {
        SEEDER
    }

    fn run<'a>(&'a self, ctx: &'a AppContext, _db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            seed(ctx).await?;
            Ok(())
        })
    }
}

/// Creates (or finds) everything the seeder seeds.
pub async fn seed(ctx: &AppContext) -> Result<(), BoxError> {
    let operator = ensure_account(ctx, "Olivia Operator", OPERATOR_EMAIL).await?.id;
    let db = ctx.get::<Db>().await?;

    let access = ctx.get::<AccessService>().await?;
    in_uow(&db, |uow| async move {
        if access.role(SUPER_ADMIN).await?.is_none() {
            access
                .create_role(&uow, SUPER_ADMIN, Some("Super admins: every check passes"))
                .await?;
        }
        access.assign_role(&uow, operator, SUPER_ADMIN).await?;
        Ok(())
    })
    .await?;

    let mut workspaces = Vec::new();
    for (index, (slug, name)) in WORKSPACES.iter().enumerate() {
        let workspace = ensure_workspace(ctx, operator, slug, name).await?;
        seed_projects(ctx, &workspace, operator, index).await?;
        workspaces.push(workspace);
    }

    // The load-test accounts: members of the first workspace.
    let first = workspaces[0].id;
    let members = ctx.get::<Members>().await?;
    let directory = ctx.get::<Workspaces>().await?;
    for n in 1..=LOAD_USERS {
        let email = format!("load-{n:03}@evidence.test");
        let user = ensure_account(ctx, &format!("Load User {n:03}"), &email).await?.id;
        if directory.membership(first, user).await?.is_none() {
            let members = members.clone();
            in_uow(&db, |uow| async move {
                members.add(&uow, first, user, WorkspaceRole::Member).await?;
                Ok(())
            })
            .await?;
        }
    }
    tracing::info!(
        workspaces = workspaces.len(),
        load_users = LOAD_USERS,
        "seeded the evidence data"
    );
    Ok(())
}

/// The workspace `slug`, created with `owner` when it does not exist.
async fn ensure_workspace(
    ctx: &AppContext,
    owner: Uuid,
    slug: &str,
    name: &str,
) -> Result<Workspace, BoxError> {
    let workspaces = ctx.get::<Workspaces>().await?;
    if let Some(workspace) = workspaces.find_by_slug(slug).await? {
        return Ok(workspace);
    }
    let db = ctx.get::<Db>().await?;
    let (slug, name) = (slug.to_owned(), name.to_owned());
    in_uow(&db, |uow| async move {
        let input = NewWorkspace::named(name).slug(slug);
        Ok(workspaces.create(&uow, owner, input).await?)
    })
    .await
}

/// Projects and their tasks in `workspace`, unless it has some already.
async fn seed_projects(
    ctx: &AppContext,
    workspace: &Workspace,
    author: Uuid,
    offset: usize,
) -> Result<(), BoxError> {
    let db = ctx.get::<Db>().await?;
    let workspaces = ctx.get::<Workspaces>().await?;
    let id = workspace.id;
    in_uow(&db, |uow| async move {
        // On PostgreSQL, row-level security admits this workspace's rows only.
        workspaces.enter(&uow, id).await?;
        let existing = WorkspaceRepo::<Project>::select(id)
            .count(&*uow.tx().await?)
            .await?;
        if existing > 0 {
            return Ok(());
        }
        let now = chrono::Utc::now();
        for n in 0..PROJECTS.len() {
            let (name, description) = PROJECTS[(n + offset) % PROJECTS.len()];
            let status = if n % 5 == 4 { "archived" } else { "active" };
            let project = WorkspaceRepo::<Project>::insert(
                &uow,
                id,
                ProjectModel {
                    id: Set(Uuid::now_v7()),
                    name: Set(name.to_owned()),
                    description: Set(Some(description.to_owned())),
                    status: Set(status.to_owned()),
                    created_by: Set(author),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                },
            )
            .await?;
            for (t, title) in TASKS.iter().enumerate() {
                let status = match (t + n) % 3 {
                    0 => "todo",
                    1 => "doing",
                    _ => "done",
                };
                Task::create(
                    &uow,
                    Task {
                        id: Uuid::now_v7(),
                        project_id: project.id,
                        title: (*title).to_owned(),
                        status: status.to_owned(),
                        priority: i32::try_from(t % 4 + 1).unwrap_or(3),
                        ..Default::default()
                    },
                )
                .await?;
            }
        }
        Ok(())
    })
    .await
}

/// The account for `email`, created with [`PASSWORD`] if it does not
/// exist, with its address marked verified.
pub async fn ensure_account(ctx: &AppContext, name: &str, email: &str) -> Result<User, BoxError> {
    let auth = ctx.get::<AuthService>().await?;
    let user = match auth.find_by_email(email).await? {
        Some(user) => user,
        None => auth.create_user(name, email, PASSWORD).await?,
    };
    if user.email_verified_at.is_some() {
        return Ok(user);
    }
    let db = ctx.get::<Db>().await?;
    let verified_at = user.created_at;
    let mut active = user.into_active_model();
    active.email_verified_at = Set(Some(verified_at));
    Ok(active.update(db.conn()).await?)
}

/// Runs `f` in a new unit of work: commits when it succeeds, rolls back
/// when it fails.
async fn in_uow<T, F, Fut>(db: &Db, f: F) -> Result<T, BoxError>
where
    F: FnOnce(Uow) -> Fut,
    Fut: Future<Output = Result<T, BoxError>>,
{
    let uow = db.uow(None, None).await;
    match f(uow.clone()).await {
        Ok(value) => {
            uow.commit().await?;
            Ok(value)
        }
        Err(error) => {
            uow.rollback().await?;
            Err(error)
        }
    }
}
