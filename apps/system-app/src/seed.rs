//! The `system` seeder: `system-app db:seed system` (or
//! `migrate:fresh --seed`). It creates:
//!
//! - the operator [`OPERATOR_EMAIL`], a super admin (the tools open for
//!   them), owner of every seeded workspace;
//! - three workspaces ([`WORKSPACES`]) with 30, 22 and 16 projects, about
//!   seven hundred tasks across statuses and priorities, and a month of
//!   reports;
//! - [`PEOPLE`], forty named people with workspace roles of their own
//!   (admin, member, viewer; a few belong to two workspaces);
//! - [`SUPPORT_EMAIL`], who signs in and belongs to a workspace but may not
//!   use the tools, and [`OPS_VIEWER_EMAIL`], whose global role
//!   [`OPS_VIEWER_ROLE`] lets them open Lighthouse, Fleet and Helm and read
//!   what they show, but resolve, operate or edit nothing;
//! - [`LOAD_USERS`] load-test accounts, `load-001@system.test` and up,
//!   members of the first workspace, so each k6 virtual user signs in as
//!   its own account;
//! - states for the feature flags, webhook endpoints for each workspace
//!   (with a first event sent to them) and settings values, all written
//!   through the packages' own services.
//!
//! This is deliberate demo data, back-dated: accounts, projects, tasks and
//! reports get creation times spread over the last [`HISTORY_DAYS`] days
//! (more on recent days than old ones, fewer at weekends), so trend charts
//! have a shape from the first minute. The times come from a fixed random
//! sequence, so every run makes the same pattern.
//!
//! Every account uses [`PASSWORD`]. Running the seeder again changes
//! nothing: what exists is found again.

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc, Weekday};
use navree::access::AccessService;
use navree::auth::{AuthService, User};
use navree::config::Config;
use navree::db::{Db, Seeder, Uow};
use navree::flags::{FlagScope, Flags, Rollout};
use navree::settings::{Scope, Settings};
use navree::webhooks::{NewEndpoint, Webhooks};
use navree::workspace::{
    Members, NewWorkspace, Workspace, WorkspaceRepo, WorkspaceRole, Workspaces,
};
use navree::{BoxFuture, prelude::*};
use serde_json::json;
use uuid::Uuid;

use crate::abilities::SUPER_ADMIN;
use crate::modules::admin::{Branding, TaskDefaults};
use crate::modules::demo::DemoOptions;
use crate::modules::projects::Project;
use crate::modules::reports::Report;
use crate::modules::showcase::{
    BoardView, InlineEditing, LegacyImport, ReportSchedules, TaskExport,
};
use crate::modules::tasks::Task;

/// The seeder's name.
pub const SEEDER: &str = "system";

/// The password of every seeded account.
pub const PASSWORD: &str = "system-password";

/// The operator: a super admin who owns every seeded workspace.
pub const OPERATOR_EMAIL: &str = "operator@system.test";

/// An account that is signed in and belongs to a workspace, and has no
/// permission on the tools.
pub const SUPPORT_EMAIL: &str = "support@system.test";

/// An account that may look at the tools and change nothing.
pub const OPS_VIEWER_EMAIL: &str = "ops-viewer@system.test";

/// The global role of [`OPS_VIEWER_EMAIL`].
pub const OPS_VIEWER_ROLE: &str = "ops-viewer";

/// What [`OPS_VIEWER_ROLE`] may do: open Lighthouse, Fleet and Helm, and
/// read the records Helm lists. Nothing that resolves, operates or edits.
pub const OPS_VIEWER_PERMISSIONS: &[&str] = &[
    "lighthouse.view",
    "fleet.view",
    "helm.view",
    "users.read",
    "workspace.read",
    "projects.read",
    "tasks.read",
    "reports.read",
    "audit.read",
];

/// The seeded workspaces: slug and name. The load profiles work in the
/// first.
pub const WORKSPACES: &[(&str, &str)] = &[
    ("agra", "Agra Logistics"),
    ("gulati", "Gulati Research"),
    ("ilaichi", "Ilaichi Software"),
];

/// How many projects each workspace gets, in [`WORKSPACES`] order.
const PROJECT_COUNTS: [usize; 3] = [30, 22, 16];

/// How many load-test accounts the seeder creates (members of `agra`).
pub const LOAD_USERS: usize = 200;

/// How many days back creation times reach.
pub const HISTORY_DAYS: i64 = 30;

/// How many reports each workspace gets.
const REPORTS_PER_WORKSPACE: usize = 18;

/// A named person and the workspaces they belong to, with their role in
/// each. The address is the name in lower case, with a dot between the
/// words, at `system.test`.
pub struct Person {
    /// The name.
    pub name: &'static str,
    /// Workspace slug and role (`admin`, `member` or `viewer`).
    pub memberships: &'static [(&'static str, &'static str)],
}

impl Person {
    /// The address.
    pub fn email(&self) -> String {
        format!("{}@system.test", self.name.to_lowercase().replace(' ', "."))
    }
}

/// The forty people.
#[rustfmt::skip]
pub const PEOPLE: &[Person] = &[
    Person { name: "Priya Raman", memberships: &[("agra", "admin"), ("gulati", "viewer")] },
    Person { name: "Manpreet Walia", memberships: &[("agra", "admin")] },
    Person { name: "Ekta Vohra", memberships: &[("agra", "admin")] },
    Person { name: "Tejinder Handa", memberships: &[("agra", "member")] },
    Person { name: "Aisha Karim", memberships: &[("agra", "member")] },
    Person { name: "Davinder Oberoi", memberships: &[("agra", "member")] },
    Person { name: "Sukhmani Lamba", memberships: &[("agra", "member")] },
    Person { name: "Ravi Menon", memberships: &[("agra", "member")] },
    Person { name: "Harleen Bhandari", memberships: &[("agra", "member")] },
    Person { name: "Jaspal Chadha", memberships: &[("agra", "member"), ("ilaichi", "viewer")] },
    Person { name: "Mehak Talwar", memberships: &[("agra", "member")] },
    Person { name: "Lakhwinder Gill", memberships: &[("agra", "member")] },
    Person { name: "Nikita Dhir", memberships: &[("agra", "member")] },
    Person { name: "Charu Mahajan", memberships: &[("agra", "viewer")] },
    Person { name: "Vandana Chawla", memberships: &[("agra", "viewer")] },
    Person { name: "Inderjit Sodhi", memberships: &[("gulati", "admin"), ("ilaichi", "member")] },
    Person { name: "Kulwant Mehta", memberships: &[("gulati", "admin")] },
    Person { name: "Balvinder Luthra", memberships: &[("gulati", "member"), ("agra", "member")] },
    Person { name: "Amarjit Puri", memberships: &[("gulati", "member")] },
    Person { name: "Yasmin Farouk", memberships: &[("gulati", "member")] },
    Person { name: "Charanjit Randhawa", memberships: &[("gulati", "member")] },
    Person { name: "Farah Siddiqui", memberships: &[("gulati", "member")] },
    Person { name: "Mohit Rana", memberships: &[("gulati", "member")] },
    Person { name: "Lovleen Ahluwalia", memberships: &[("gulati", "member")] },
    Person { name: "Gurleen Nagpal", memberships: &[("gulati", "viewer")] },
    Person { name: "Ishaan Kohli", memberships: &[("gulati", "viewer")] },
    Person { name: "Navjot Brar", memberships: &[("ilaichi", "admin"), ("agra", "member")] },
    Person { name: "Rahul Deshmukh", memberships: &[("ilaichi", "admin")] },
    Person { name: "Jyoti Saini", memberships: &[("ilaichi", "member")] },
    Person { name: "Pradeep Nanda", memberships: &[("ilaichi", "member"), ("gulati", "viewer")] },
    Person { name: "Amarpreet Dua", memberships: &[("ilaichi", "member")] },
    Person { name: "Eshan Chopra", memberships: &[("ilaichi", "member")] },
    Person { name: "Kiran Walia", memberships: &[("ilaichi", "member")] },
    Person { name: "Samir Aziz", memberships: &[("ilaichi", "member")] },
    Person { name: "Gagan Lamba", memberships: &[("ilaichi", "member")] },
    Person { name: "Deepak Marwah", memberships: &[("ilaichi", "member")] },
    Person { name: "Hardeep Kang", memberships: &[("ilaichi", "viewer")] },
    Person { name: "Onkar Bedi", memberships: &[("ilaichi", "viewer")] },
    Person { name: "Tanya Khurana", memberships: &[("agra", "member"), ("gulati", "member")] },
    Person { name: "Vikas Hans", memberships: &[("gulati", "member")] },
];

/// Project names and what they are about; each workspace takes a run of
/// them from its own starting point.
const PROJECTS: &[(&str, &str)] = &[
    (
        "Website relaunch",
        "New pages, new pricing, one launch date.",
    ),
    (
        "Mobile app",
        "Sign-in, projects and notifications on the phone.",
    ),
    (
        "Onboarding emails",
        "Five emails over a customer's first two weeks.",
    ),
    (
        "Warehouse scanners",
        "Replace the handheld scanners at the north depot.",
    ),
    (
        "Quarterly planning",
        "Goals, owners and budgets for next quarter.",
    ),
    (
        "Billing migration",
        "Move every plan to usage-based invoices.",
    ),
    (
        "Support handbook",
        "Answers to the twenty most asked questions.",
    ),
    (
        "Data retention",
        "Delete what we no longer need, on a schedule.",
    ),
    ("Partner portal", "Self-serve access for resellers."),
    ("Security review", "Pen test findings and their fixes."),
    (
        "Route optimisation",
        "Shorter delivery routes from live traffic.",
    ),
    (
        "Hiring pipeline",
        "Three engineers and a designer by spring.",
    ),
    (
        "Dock scheduling",
        "Time slots for inbound trucks at every depot.",
    ),
    (
        "Returns portal",
        "Customers start a return without calling anyone.",
    ),
    (
        "Carrier scorecards",
        "Rank carriers by delivery time and damage rate.",
    ),
    (
        "Pricing experiment",
        "Test two plan structures on new sign-ups.",
    ),
    (
        "Invoice reminders",
        "Polite nudges before and after the due date.",
    ),
    (
        "Customer interviews",
        "Twelve conversations about how teams plan work.",
    ),
    (
        "Analytics rebuild",
        "One source of truth for the weekly numbers.",
    ),
    (
        "Accessibility audit",
        "Keyboard, contrast and screen reader fixes.",
    ),
    (
        "Status page",
        "Show incidents and maintenance in one place.",
    ),
    (
        "Single sign-on",
        "Let larger customers bring their identity provider.",
    ),
    ("Audit export", "A monthly file for the compliance team."),
    (
        "Cold storage pilot",
        "Temperature logging for the chilled lane.",
    ),
    (
        "Driver app v2",
        "Offline proof of delivery and signature capture.",
    ),
    (
        "Knowledge base",
        "Articles that deflect the repeat questions.",
    ),
    ("Roadmap review", "Cut what has not earned its place."),
    (
        "Mail deliverability",
        "SPF, DKIM and a cleaner sending reputation.",
    ),
    ("Load testing", "Find the limit before the customers do."),
    ("Localisation", "French and German for the web app."),
    (
        "Referral programme",
        "Credit for customers who bring a colleague.",
    ),
    (
        "Inventory counts",
        "Cycle counts that do not stop the line.",
    ),
    (
        "Vendor review",
        "Renew, replace or drop the long-tail suppliers.",
    ),
    ("Team offsite", "Two days, one agenda, no slides."),
    (
        "Backup drills",
        "Restore from last night's copy, on purpose.",
    ),
    ("Search tuning", "Make the first result the right one."),
];

/// Task titles, drawn at random for each project.
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
    "Book the kickoff",
    "List the open questions",
    "Collect examples from customers",
    "Compare two vendors",
    "Update the runbook",
    "Prepare the demo",
    "Schedule the training",
    "Reconcile the numbers",
    "Check the edge cases",
    "Write the migration plan",
    "Draft the FAQ",
    "Review the permissions",
    "Archive the old documents",
    "Set up the dashboard",
    "Ask finance to sign off",
    "Run the dry run",
    "Record a walkthrough",
    "Send the status update",
    "Triage the backlog",
    "Pick a launch date",
    "Confirm the owner",
    "Clean up duplicate records",
    "Estimate the effort",
    "Document the decision",
    "Plan the rollout",
    "Check the rollback",
    "Interview two users",
    "Tidy the naming",
    "Add the missing tests",
    "Close the loop with support",
];

/// Registers the `system` seeder.
#[module(register = Self::wire)]
pub struct SeedModule;

impl SeedModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.seeder::<SystemSeeder>();
    }
}

/// `db:seed system`; see the [module docs](self).
#[injectable]
pub struct SystemSeeder;

impl Seeder for SystemSeeder {
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
    let db = ctx.get::<Db>().await?;
    let calendar = Calendar::new(Utc::now());
    let mut rng = Rng::new(0x5EED_0001);

    // The operator signed up first, at the start of the history.
    let operator =
        ensure_account_at(ctx, "Ojaswi Operator", OPERATOR_EMAIL, calendar.start()).await?;

    ensure_roles(ctx, &db, operator.id).await?;

    let mut workspaces = Vec::new();
    for (slug, name) in WORKSPACES {
        workspaces.push(ensure_workspace(ctx, operator.id, slug, name).await?);
    }

    // The people: named accounts, signed up on different days, with roles.
    let members = ctx.get::<Members>().await?;
    let directory = ctx.get::<Workspaces>().await?;
    let mut people: Vec<Vec<Uuid>> = vec![vec![operator.id]; workspaces.len()];
    for person in PEOPLE {
        let signed_up = calendar.when(&mut rng);
        let user = ensure_account_at(ctx, person.name, &person.email(), signed_up).await?;
        for (slug, role) in person.memberships {
            let Some(index) = WORKSPACES.iter().position(|(s, _)| s == slug) else {
                continue;
            };
            ensure_member(
                &db,
                &members,
                &directory,
                workspaces[index].id,
                user.id,
                role,
            )
            .await?;
            people[index].push(user.id);
        }
    }

    // The two accounts with a fixed purpose.
    let first = workspaces[0].id;
    let support =
        ensure_account_at(ctx, "Sahil Support", SUPPORT_EMAIL, calendar.when(&mut rng)).await?;
    ensure_member(&db, &members, &directory, first, support.id, "member").await?;
    let observer = ensure_account_at(
        ctx,
        "Ola Observer",
        OPS_VIEWER_EMAIL,
        calendar.when(&mut rng),
    )
    .await?;
    ensure_member(&db, &members, &directory, first, observer.id, "viewer").await?;
    let access = ctx.get::<AccessService>().await?;
    in_uow(&db, |uow| async move {
        access
            .assign_role(&uow, observer.id, OPS_VIEWER_ROLE)
            .await?;
        Ok(())
    })
    .await?;

    // The load-test accounts: members of the first workspace.
    for n in 1..=LOAD_USERS {
        let email = format!("load-{n:03}@system.test");
        let user = ensure_account_at(
            ctx,
            &format!("Load User {n:03}"),
            &email,
            calendar.when(&mut rng),
        )
        .await?;
        ensure_member(&db, &members, &directory, first, user.id, "member").await?;
    }

    // Projects, tasks and reports.
    for (index, workspace) in workspaces.iter().enumerate() {
        let mut rng = Rng::new(0xC0FFEE + u64::try_from(index).unwrap_or(0));
        seed_work(ctx, workspace, &people[index], index, &calendar, &mut rng).await?;
    }

    seed_flags(ctx, &workspaces).await?;
    seed_webhooks(ctx, &db, &workspaces).await?;
    seed_settings(ctx, &db, &workspaces).await?;

    tracing::info!(
        workspaces = workspaces.len(),
        people = PEOPLE.len(),
        load_users = LOAD_USERS,
        "seeded the system data"
    );
    Ok(())
}

/// The global roles: the super admin (the operator) and the viewer who
/// may only look at the tools.
async fn ensure_roles(ctx: &AppContext, db: &Db, operator: Uuid) -> Result<(), BoxError> {
    let access = ctx.get::<AccessService>().await?;
    in_uow(db, |uow| async move {
        if access.role(SUPER_ADMIN).await?.is_none() {
            access
                .create_role(&uow, SUPER_ADMIN, Some("Super admins: every check passes"))
                .await?;
        }
        access.assign_role(&uow, operator, SUPER_ADMIN).await?;
        if access.role(OPS_VIEWER_ROLE).await?.is_none() {
            access
                .create_role(
                    &uow,
                    OPS_VIEWER_ROLE,
                    Some("Opens Lighthouse, Fleet and Helm and reads what they show; changes nothing"),
                )
                .await?;
        }
        access
            .sync_role_permissions(&uow, OPS_VIEWER_ROLE, OPS_VIEWER_PERMISSIONS)
            .await?;
        Ok(())
    })
    .await
}

/// Makes `user` a member of `workspace` with `role`, unless they are one.
async fn ensure_member(
    db: &Db,
    members: &Arc<Members>,
    directory: &Arc<Workspaces>,
    workspace: Uuid,
    user: Uuid,
    role: &str,
) -> Result<(), BoxError> {
    if directory.membership(workspace, user).await?.is_some() {
        return Ok(());
    }
    let members = members.clone();
    let role = WorkspaceRole::parse(role);
    in_uow(db, |uow| async move {
        members.add(&uow, workspace, user, role).await?;
        Ok(())
    })
    .await
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

/// A workspace's projects, their tasks and its reports, unless it has its
/// projects already. `people` are the users who belong to it.
async fn seed_work(
    ctx: &AppContext,
    workspace: &Workspace,
    people: &[Uuid],
    index: usize,
    calendar: &Calendar,
    rng: &mut Rng,
) -> Result<(), BoxError> {
    let db = ctx.get::<Db>().await?;
    let workspaces = ctx.get::<Workspaces>().await?;
    let id = workspace.id;
    let wanted = PROJECT_COUNTS[index % PROJECT_COUNTS.len()];
    let offset = [0, 12, 24][index % 3];
    in_uow(&db, |uow| async move {
        // On PostgreSQL, row-level security admits this workspace's rows only.
        workspaces.enter(&uow, id).await?;
        let existing = WorkspaceRepo::<Project>::query(id).count(&uow).await?;
        if usize::try_from(existing).unwrap_or(usize::MAX) >= wanted {
            return Ok(());
        }
        let mut tasks = Vec::new();
        let mut totals = [0_u64; 3];
        for n in 0..wanted {
            let (name, description) = PROJECTS[(n + offset) % PROJECTS.len()];
            let created_at = calendar.when(rng);
            let age_days = (calendar.now - created_at).num_days();
            // Older projects are more often finished with.
            let archived = rng.percent(if age_days > 14 { 30 } else { 6 });
            let author = *rng.pick(people);
            let project = WorkspaceRepo::<Project>::insert(
                &uow,
                id,
                Project {
                    id: Uuid::now_v7(),
                    name: name.to_owned(),
                    description: Some(description.to_owned()),
                    status: if archived { "archived" } else { "active" }.to_owned(),
                    created_by: author,
                    created_at,
                    updated_at: created_at,
                    ..Project::default()
                },
            )
            .await?;
            for _ in 0..(6 + rng.below(9)) {
                let task = make_task(id, project.id, created_at, archived, people, calendar, rng);
                totals[match task.status.as_str() {
                    "todo" => 0,
                    "doing" => 1,
                    _ => 2,
                }] += 1;
                tasks.push(task);
            }
        }
        Task::fill_and_insert(&uow, tasks).await?;

        let mut reports = Vec::new();
        for _ in 0..REPORTS_PER_WORKSPACE {
            reports.push(make_report(id, totals, calendar, rng));
        }
        Report::fill_and_insert(&uow, reports).await?;
        Ok(())
    })
    .await
}

/// One task of a project created at `project_at`.
fn make_task(
    workspace: Uuid,
    project: Uuid,
    project_at: DateTime<Utc>,
    archived: bool,
    people: &[Uuid],
    calendar: &Calendar,
    rng: &mut Rng,
) -> Task {
    let created_at = calendar.between(project_at, rng);
    let status = if archived || rng.percent(40) {
        "done"
    } else if rng.percent(42) {
        "doing"
    } else {
        "todo"
    };
    let updated_at = if status == "todo" {
        created_at
    } else {
        calendar.between(created_at, rng)
    };
    let priority = match rng.below(100) {
        0..=9 => 1,
        10..=34 => 2,
        35..=79 => 3,
        _ => 4,
    };
    let due_on = rng.percent(75).then(|| {
        (created_at + Duration::days(3 + i64::try_from(rng.below(28)).unwrap_or(7))).date_naive()
    });
    Task {
        id: Uuid::now_v7(),
        workspace_id: workspace,
        project_id: project,
        title: (*rng.pick(TASKS)).to_owned(),
        status: status.to_owned(),
        priority,
        due_on,
        assignee_id: rng.percent(85).then(|| *rng.pick(people)),
        created_at,
        updated_at,
    }
}

/// One report of a workspace whose tasks now number `totals` (to do, in
/// progress, done): the numbers it found were a share of those.
fn make_report(workspace: Uuid, totals: [u64; 3], calendar: &Calendar, rng: &mut Rng) -> Report {
    let created_at = calendar.when(rng);
    let recent = (calendar.now - created_at).num_minutes() < 30;
    let status = match rng.below(100) {
        _ if recent => "queued",
        0..=7 => "computed",
        _ => "done",
    };
    let share = 40 + rng.below(61);
    let [todo, doing, done] = totals.map(|count| count * share / 100);
    let finished_at = (status != "queued")
        .then(|| created_at + Duration::seconds(2 + i64::try_from(rng.below(40)).unwrap_or(5)));
    Report {
        id: Uuid::now_v7(),
        workspace_id: workspace,
        kind: "task-summary".to_owned(),
        status: status.to_owned(),
        result: (status != "queued")
            .then(|| json!({ "tasks": { "todo": todo, "doing": doing, "done": done } })),
        attempts: if status == "queued" {
            0
        } else {
            1 + i32::from(rng.percent(8))
        },
        finished_at,
        created_at,
        updated_at: finished_at.unwrap_or(created_at),
    }
}

/// States for the feature flags, written through the flags service:
/// a rollout, an override for a workspace, a kill switch.
async fn seed_flags(ctx: &AppContext, workspaces: &[Workspace]) -> Result<(), BoxError> {
    let flags = ctx.get::<Flags>().await?;
    let (agra, ilaichi) = (workspaces[0].id, workspaces[2].id);

    // The board view reaches half of the workspaces.
    if flags.state::<BoardView>().await?.rollout.is_none() {
        flags
            .rollout::<BoardView>(Rollout::percent(50).of_workspaces())
            .await?;
    }
    // Inline editing: a quarter of users, and Agra for everyone.
    if flags.state::<InlineEditing>().await?.rollout.is_none() {
        flags
            .rollout::<InlineEditing>(Rollout::percent(25).of_users())
            .await?;
    }
    if !flags
        .active::<InlineEditing>(FlagScope::workspace(agra))
        .await?
    {
        flags
            .activate::<InlineEditing>(FlagScope::workspace(agra))
            .await?;
    }
    // Report schedules: on for Agra only.
    if !flags
        .active::<ReportSchedules>(FlagScope::workspace(agra))
        .await?
    {
        flags
            .activate::<ReportSchedules>(FlagScope::workspace(agra))
            .await?;
    }
    // The export is on by default; Ilaichi has it off.
    if flags
        .active::<TaskExport>(FlagScope::workspace(ilaichi))
        .await?
    {
        flags
            .deactivate::<TaskExport>(FlagScope::workspace(ilaichi))
            .await?;
    }
    // The old importer is switched off for everyone.
    if !flags.state::<LegacyImport>().await?.killed {
        flags.kill::<LegacyImport>().await?;
    }
    Ok(())
}

/// Webhook endpoints for each workspace, created through the webhooks
/// service the first time, and an event sent to them: one that takes the
/// event, one that fails, and (in the last workspace) one whose address is
/// gone.
async fn seed_webhooks(
    ctx: &AppContext,
    db: &Db,
    workspaces: &[Workspace],
) -> Result<(), BoxError> {
    let webhooks = ctx.get::<Webhooks>().await?;
    let options = ctx.get::<Config>().await?.typed::<DemoOptions>()?;
    for (index, workspace) in workspaces.iter().enumerate() {
        if !webhooks.endpoints(Some(workspace.id)).await?.is_empty() {
            continue;
        }
        let id = workspace.id;
        let mut endpoints = vec![
            NewEndpoint::new(options.sink_url("alerts"))
                .events(["*"])
                .description("Operations alert sink")
                .workspace(id),
            match index {
                0 => NewEndpoint::new(options.shared_stub_url(500))
                    .events(["task.completed"])
                    .description("CRM sync (answers with errors)")
                    .workspace(id),
                1 => NewEndpoint::new(options.shared_stub_url(200))
                    .events(["project.archived", "report.requested"])
                    .description("Archive notifier")
                    .workspace(id),
                _ => NewEndpoint::new(options.shared_stub_url(410))
                    .events(["task.completed", "project.archived"])
                    .description("Retired reporting service")
                    .workspace(id),
            },
        ];
        if index == 0 {
            endpoints.push(
                NewEndpoint::new(options.shared_stub_url(200))
                    .events(["report.requested"])
                    .description("Finance reports")
                    .header("X-Team", "finance")
                    .workspace(id),
            );
        }
        let created = webhooks.clone();
        in_uow(db, |uow| async move {
            for endpoint in endpoints {
                created.create_endpoint(&uow, endpoint).await?;
            }
            Ok(())
        })
        .await?;
        for event in ["task.completed", "project.archived", "report.requested"] {
            webhooks
                .dispatch_json(
                    event,
                    json!({ "source": "seed", "workspace": workspace.slug }),
                )
                .workspace(id)
                .send()
                .await?;
        }
    }
    Ok(())
}

/// Settings values: global defaults, and overrides for two workspaces.
async fn seed_settings(
    ctx: &AppContext,
    db: &Db,
    workspaces: &[Workspace],
) -> Result<(), BoxError> {
    let settings = ctx.get::<Settings>().await?;
    let (agra, gulati) = (workspaces[0].id, workspaces[1].id);
    if !settings
        .stored::<TaskDefaults>(Scope::global())
        .await?
        .is_empty()
    {
        return Ok(());
    }
    in_uow(db, |uow| async move {
        settings
            .update::<TaskDefaults>(
                &uow,
                Scope::global(),
                json!({ "default_priority": 3, "default_due_days": 14, "archive_after_days": 60 }),
            )
            .await?;
        settings
            .update::<TaskDefaults>(
                &uow,
                Scope::workspace(agra),
                json!({ "default_priority": 2, "require_assignee": true }),
            )
            .await?;
        settings
            .update::<TaskDefaults>(
                &uow,
                Scope::workspace(gulati),
                json!({ "default_due_days": 7 }),
            )
            .await?;
        settings
            .update::<Branding>(
                &uow,
                Scope::global(),
                json!({ "product_name": "Navree System", "support_email": "support@system.test" }),
            )
            .await?;
        settings
            .update::<Branding>(
                &uow,
                Scope::workspace(agra),
                json!({
                    "product_name": "Agra Logistics Tasks",
                    "accent_color": "#ff8a3d",
                    "mail_footer": "Agra Logistics, 14 Dock Road. Sent by the operations team.",
                    "show_beta_badge": true
                }),
            )
            .await?;
        Ok(())
    })
    .await
}

/// The account for `email`, created with [`PASSWORD`] if it does not
/// exist, with its address marked verified.
pub async fn ensure_account(ctx: &AppContext, name: &str, email: &str) -> Result<User, BoxError> {
    ensure_account_at(ctx, name, email, Utc::now()).await
}

/// [`ensure_account`], creating the account as if it signed up at
/// `created_at`: its creation time, and its address verified minutes later.
/// An account that exists is left as it is, except that its address is
/// marked verified when it was not.
pub async fn ensure_account_at(
    ctx: &AppContext,
    name: &str,
    email: &str,
    created_at: DateTime<Utc>,
) -> Result<User, BoxError> {
    let auth = ctx.get::<AuthService>().await?;
    let db = ctx.get::<Db>().await?;
    let (user, created) = match auth.find_by_email(email).await? {
        Some(user) => (user, false),
        None => (auth.create_user(name, email, PASSWORD).await?, true),
    };
    if !created && user.email_verified_at.is_some() {
        return Ok(user);
    }
    let mut record = User::find(&*db, user.id)
        .await?
        .ok_or("the account was just found or created")?;
    record
        .update(&*db, |user| {
            if created {
                user.created_at = created_at;
                user.updated_at = created_at;
            }
            user.email_verified_at = Some(user.created_at + Duration::minutes(5));
        })
        .await?;
    Ok(record.into_inner())
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

/// A small, fixed random sequence (xorshift): the same pattern every run.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number from 0 to `n`, not including `n`.
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    /// Whether a `p` in 100 chance came up.
    fn percent(&mut self, p: u64) -> bool {
        self.below(100) < p
    }

    /// One of `items`.
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let n = u64::try_from(items.len()).unwrap_or(1);
        &items[usize::try_from(self.below(n)).unwrap_or(0)]
    }
}

/// The last [`HISTORY_DAYS`] days, each with a weight: later days count
/// more, weekends less.
struct Calendar {
    now: DateTime<Utc>,
    days: Vec<(DateTime<Utc>, u64)>,
    total: u64,
}

impl Calendar {
    fn new(now: DateTime<Utc>) -> Self {
        let today = now.date_naive();
        let mut days = Vec::new();
        for back in (0..HISTORY_DAYS).rev() {
            let date: NaiveDate = today - Duration::days(back);
            let start = Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap_or_default());
            let recent = u64::try_from(HISTORY_DAYS - back).unwrap_or(1);
            let mut weight = 40 + 3 * recent;
            if matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
                weight = weight * 35 / 100;
            }
            days.push((start, weight));
        }
        let total = days.iter().map(|(_, weight)| weight).sum();
        Self { now, days, total }
    }

    /// The start of the history.
    fn start(&self) -> DateTime<Utc> {
        self.days
            .first()
            .map_or(self.now, |(start, _)| *start + Duration::hours(9))
    }

    /// A time on one of the days, in working hours, never in the future.
    fn when(&self, rng: &mut Rng) -> DateTime<Utc> {
        let mut pick = rng.below(self.total);
        for (start, weight) in &self.days {
            if pick < *weight {
                let seconds = 6 * 3600 + i64::try_from(rng.below(12 * 3600)).unwrap_or(0);
                let guard = 60 + i64::try_from(rng.below(3 * 3600)).unwrap_or(0);
                return (*start + Duration::seconds(seconds))
                    .min(self.now - Duration::seconds(guard));
            }
            pick -= weight;
        }
        self.now
    }

    /// A time from `from` to now.
    fn between(&self, from: DateTime<Utc>, rng: &mut Rng) -> DateTime<Utc> {
        let span = (self.now - from).num_seconds().max(1);
        let offset = rng.below(u64::try_from(span).unwrap_or(1));
        from + Duration::seconds(i64::try_from(offset).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_have_distinct_addresses_and_known_workspaces() {
        let mut seen = std::collections::BTreeSet::new();
        for person in PEOPLE {
            assert!(seen.insert(person.email()), "{} repeats", person.name);
            for (slug, role) in person.memberships {
                assert!(WORKSPACES.iter().any(|(s, _)| s == slug), "{slug}");
                assert!(["admin", "member", "viewer"].contains(role), "{role}");
            }
        }
        assert_eq!(PEOPLE.len(), 40);
    }

    #[test]
    fn project_names_do_not_repeat_inside_a_workspace() {
        for (count, offset) in PROJECT_COUNTS.iter().zip([0, 12, 24]) {
            let names: std::collections::BTreeSet<_> = (0..*count)
                .map(|n| PROJECTS[(n + offset) % PROJECTS.len()].0)
                .collect();
            assert_eq!(names.len(), *count);
        }
    }

    #[test]
    fn times_stay_inside_the_history() {
        let now = Utc::now();
        let calendar = Calendar::new(now);
        let mut rng = Rng::new(7);
        for _ in 0..500 {
            let at = calendar.when(&mut rng);
            assert!(at <= now);
            assert!(at >= now - Duration::days(HISTORY_DAYS + 1));
            let later = calendar.between(at, &mut rng);
            assert!(later >= at && later <= now);
        }
    }
}
