//! Every public item `docs/packages/directory-sync` names (types, fields,
//! methods, configuration keys, events, tables, routes, commands, codes and
//! fake helpers), so a rename fails the build or this test. The behavior is
//! exercised in `tests/directory`; the page's examples run in the facade's
//! `tests/directory_docs.rs`.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use navree_access::{AbilityCtx, AccessModule, AccessOptions, Rules};
use navree_config::{Config, ConfigSources};
use navree_contracts::Secret;
use navree_db::DatabaseModule;
use navree_directory::{
    BUILT_IN_SOURCES, ChangeAction, ChangeStatus, ChangeSubject, ConnectionStatus,
    ConnectionUpdate, ContactValue, CreatedConnection, DIRECTORY_MIGRATIONS, DIRECTORY_SCOPE_SET,
    Directory, DirectoryConnection, DirectoryController, DirectoryError, DirectoryFake,
    DirectoryGroup, DirectoryModule, DirectoryOptions, DirectoryOwner, DirectoryScopes,
    DirectorySink, DirectorySource, DirectorySyncFinished, DirectoryUser, DirectoryUserCreated,
    DirectoryUserDeactivated, DirectoryUserUpdated, FakePull, GOOGLE, GroupChange, LDAP,
    LdapPolicy, Level, MANAGE_PERMISSION, MICROSOFT, MemberChange, ModuleBuilderDirectoryExt,
    NewConnection, OKTA, Owner, PullRequest, Pulled, RunStatus, RunSummary, RunTrigger, SCIM,
    SCIM_TOKEN_PREFIX, ScimController, ScimOptions, SinkContext, SyncChange, SyncDirectory,
    SyncOptions, SyncRun, SyncSettings, SyncedGroup, SyncedUser, UserChange, models,
};
use navree_events::Event;
use navree_integrations::ScopeSet;
use navree_orm::Model as _;
use navree_testing::TestApp;
use serde_json::{Value, json};

#[path = "directory/servers.rs"]
mod servers;
use uuid::Uuid;

#[test]
fn the_configuration_keys_have_their_documented_defaults() {
    let DirectoryOptions {
        routes,
        level,
        scim,
        sync,
        ldap,
    } = DirectoryOptions::default();
    assert!(routes);
    assert_eq!(level, Level::Workspace);
    let LdapPolicy {
        block_private,
        allow_hosts,
    } = ldap;
    assert!(block_private);
    assert!(allow_hosts.is_empty());
    let ScimOptions {
        enabled,
        path,
        base_url,
        max_results,
    } = scim;
    assert!(enabled);
    assert_eq!(path, "/scim/v2");
    assert_eq!(base_url, None);
    assert_eq!(max_results, 200);
    let SyncSettings {
        default_interval_mins,
        chunk_size,
        max_deactivation_percent,
        min_deactivations,
        max_recorded_changes,
        keep_runs_days,
        stale_after_mins,
    } = sync;
    assert_eq!(default_interval_mins, 60);
    assert_eq!(chunk_size, 100);
    assert_eq!(max_deactivation_percent, 20);
    assert_eq!(min_deactivations, 10);
    assert_eq!(max_recorded_changes, 5000);
    assert_eq!(keep_runs_days, 90);
    assert_eq!(stale_after_mins, 120);

    // The documented TOML, and the environment variable form.
    let config = Config::load(
        &ConfigSources::new()
            .set(
                "directory",
                json!({
                    "routes": true,
                    "level": "workspace",
                    "scim": {
                        "enabled": true,
                        "path": "/scim/v2",
                        "base_url": "https://app.acme.test/scim/v2",
                        "max_results": 200
                    },
                    "sync": {
                        "default_interval_mins": 60,
                        "chunk_size": 100,
                        "min_deactivations": 10,
                        "max_recorded_changes": 5000,
                        "keep_runs_days": 90,
                        "stale_after_mins": 120
                    },
                    "ldap": { "block_private": true }
                }),
            )
            .env_vars(
                "APP_",
                [
                    ("APP_DIRECTORY__SYNC__MAX_DEACTIVATION_PERCENT", "10"),
                    (
                        "APP_DIRECTORY__LDAP__ALLOW_HOSTS",
                        "dc1.corp.internal,10.0.0.5",
                    ),
                ],
            ),
    )
    .unwrap();
    let options: DirectoryOptions = config.typed().unwrap();
    assert_eq!(options.sync.max_deactivation_percent, 10);
    assert_eq!(
        options.ldap.allow_hosts,
        vec!["dc1.corp.internal", "10.0.0.5"]
    );
    assert_eq!(
        options.scim.base_url.as_deref(),
        Some("https://app.acme.test/scim/v2")
    );
}

#[test]
fn the_normalized_directory_has_the_documented_fields() {
    let DirectoryUser {
        external_id,
        user_name,
        active,
        given_name,
        family_name,
        display_name,
        emails,
        phone_numbers,
        title,
        department,
        organization,
        employee_number,
        manager_id,
        locale,
        time_zone,
        attributes,
    } = DirectoryUser::new("00u1", "anu@acme.test")
        .name("Anu", "Lakhotia")
        .display_name("Anu Lakhotia")
        .email("anu@acme.test")
        .phone("mobile", "+44 7700 900123")
        .title("Engineer")
        .department("Engineering")
        .employee_number("1815")
        .manager("00u0")
        .attribute("costCenter", "4130");
    let _: (String, String, bool) = (external_id, user_name, active);
    let _: [Option<String>; 10] = [
        given_name,
        family_name,
        display_name,
        title,
        department,
        organization,
        employee_number,
        manager_id,
        locale,
        time_zone,
    ];
    let _: (Vec<ContactValue>, Vec<ContactValue>) = (emails, phone_numbers);
    let _: BTreeMap<String, Value> = attributes;
    let ContactValue {
        value,
        kind,
        primary,
    } = ContactValue::new("anu@acme.test", Some("work"), true);
    let _: (String, Option<String>, bool) = (value, kind, primary);

    let user = DirectoryUser::new("u", "u").inactive();
    let _: (Option<&str>, Option<&str>, String, String, Vec<String>) = (
        user.primary_email(),
        user.primary_phone(),
        user.full_name(),
        user.fingerprint(),
        user.changed_fields(&user),
    );

    let DirectoryGroup {
        external_id,
        display_name,
        description,
        email,
        members,
        attributes,
    } = DirectoryGroup::new("g1", "Engineering")
        .description("Builders")
        .email("eng@acme.test")
        .members(["00u1"]);
    let _: (String, String, Option<String>, Option<String>, Vec<String>) =
        (external_id, display_name, description, email, members);
    let _: BTreeMap<String, Value> = attributes;

    let Pulled {
        users,
        groups,
        complete,
        removed_users,
        removed_groups,
        member_changes,
        cursor,
    } = Pulled::delta(Vec::new(), Vec::new())
        .removed_user("u")
        .removed_group("g")
        .member_added("g", "u")
        .member_removed("g", "u")
        .cursor("c");
    let _: (Vec<DirectoryUser>, Vec<DirectoryGroup>, bool) = (users, groups, complete);
    let _: (Vec<String>, Vec<String>, Option<String>) = (removed_users, removed_groups, cursor);
    let MemberChange { group, user, added } = member_changes[0].clone();
    let _: (String, String, bool) = (group, user, added);
    assert!(Pulled::snapshot(Vec::new(), Vec::new()).complete);
}

#[test]
fn the_service_values_have_their_documented_fields() {
    let now: DateTime<Utc> = Utc::now();
    let connection = DirectoryConnection {
        id: Uuid::nil(),
        owner_type: navree_integrations::OwnerType::Workspace,
        owner_id: Some(Uuid::nil()),
        name: "Agra".into(),
        source: SCIM.into(),
        settings: json!({}),
        has_secret: false,
        status: ConnectionStatus::Active,
        scim_token_hint: Some("nvscim_3fQx".into()),
        sync_interval_mins: 0,
        last_run_id: None,
        last_synced_at: None,
        next_sync_at: None,
        created_by: None,
        created_at: now,
        updated_at: now,
    };
    assert_eq!(connection.owner(), Owner::Workspace(Uuid::nil()));
    assert!(connection.is_scim());
    assert_eq!(connection.setting("url"), None);
    let wire = serde_json::to_value(&connection).unwrap();
    for key in [
        "id",
        "owner_type",
        "owner_id",
        "name",
        "source",
        "settings",
        "has_secret",
        "status",
        "scim_token_hint",
        "sync_interval_mins",
        "last_run_id",
        "last_synced_at",
        "next_sync_at",
        "created_by",
        "created_at",
        "updated_at",
    ] {
        assert!(wire.get(key).is_some(), "{key}");
    }
    let created = CreatedConnection {
        connection,
        scim_token: Some(Secret::new("nvscim_x".into())),
        scim_url: Some("/scim/v2".into()),
    };
    assert_eq!(
        serde_json::to_value(&created).unwrap()["scim_token"],
        json!("nvscim_x")
    );

    let RunSummary {
        users_seen,
        groups_seen,
        users_created,
        users_updated,
        users_reactivated,
        users_deactivated,
        users_unchanged,
        groups_created,
        groups_updated,
        groups_deleted,
        groups_unchanged,
        memberships_added,
        memberships_removed,
        skipped,
        unknown_members,
        failed,
    } = RunSummary::default();
    let _: [u64; 16] = [
        users_seen,
        groups_seen,
        users_created,
        users_updated,
        users_reactivated,
        users_deactivated,
        users_unchanged,
        groups_created,
        groups_updated,
        groups_deleted,
        groups_unchanged,
        memberships_added,
        memberships_removed,
        skipped,
        unknown_members,
        failed,
    ];
    let SyncRun {
        id: _,
        connection_id: _,
        status: _,
        trigger: _,
        dry_run: _,
        force: _,
        complete: _,
        summary: _,
        done: _,
        total: _,
        note: _,
        error: _,
        operation_id: _,
        created_by: _,
        created_at: _,
        started_at: _,
        finished_at: _,
    } = sample_run(now);
    let SyncChange {
        id: _,
        run_id: _,
        subject,
        action,
        external_id: _,
        label: _,
        fields: _,
        members_added: _,
        members_removed: _,
        status,
        error: _,
    } = SyncChange {
        id: Uuid::nil(),
        run_id: Uuid::nil(),
        subject: ChangeSubject::User,
        action: ChangeAction::Create,
        external_id: "u".into(),
        label: "u".into(),
        fields: Vec::new(),
        members_added: 0,
        members_removed: 0,
        status: ChangeStatus::Planned,
        error: None,
    };
    let _ = (subject, action, status);
    let SyncOptions {
        dry_run,
        force,
        full,
        trigger,
    } = SyncOptions::dry_run();
    assert!(dry_run && !force && !full);
    assert_eq!(trigger, RunTrigger::Manual);
    for (status, name) in [
        (RunStatus::Queued, "queued"),
        (RunStatus::Running, "running"),
        (RunStatus::Succeeded, "succeeded"),
        (RunStatus::Partial, "partial"),
        (RunStatus::Held, "held"),
        (RunStatus::Failed, "failed"),
        (RunStatus::Cancelled, "cancelled"),
    ] {
        assert_eq!(status.as_str(), name);
    }
    for (action, name) in [
        (ChangeAction::Create, "create"),
        (ChangeAction::Update, "update"),
        (ChangeAction::Reactivate, "reactivate"),
        (ChangeAction::Deactivate, "deactivate"),
        (ChangeAction::Delete, "delete"),
    ] {
        assert_eq!(action.as_str(), name);
    }
    fn synced_user(u: &SyncedUser) {
        let _: (Uuid, &DirectoryUser, &Option<String>) = (u.id, &u.user, &u.local_id);
        let _: Option<DateTime<Utc>> = u.deactivated_at;
    }
    fn synced_group(g: &SyncedGroup) {
        let _: (Uuid, &DirectoryGroup, &Option<String>) = (g.id, &g.group, &g.local_id);
    }
    fn user_change(c: &UserChange) {
        let _: (ChangeAction, Uuid, bool) = (c.action, c.directory_user_id, c.deleted);
        let _: (&Option<DirectoryUser>, &Vec<String>) = (&c.previous, &c.changed);
    }
    fn group_change(c: &GroupChange) {
        let _: (Uuid, &Option<String>) = (c.directory_group_id, &c.local_id);
        let _: (&Vec<String>, &Vec<String>) = (&c.added, &c.removed);
    }
    fn pull_request(r: &PullRequest) {
        let _: &DirectoryConnection = &r.connection;
        let _: (&Option<String>, &Option<Secret<String>>) = (&r.cursor, &r.secret);
    }
    fn fake_pull(p: &FakePull) {
        let _: (Uuid, &String, &Option<String>) = (p.connection_id, &p.source, &p.cursor);
    }
    fn sink_context(c: &SinkContext) {
        let _: (&DirectoryConnection, Owner, Option<Uuid>) =
            (c.connection(), c.owner(), c.run_id());
    }
    let _ = (
        synced_user as fn(&SyncedUser),
        synced_group as fn(&SyncedGroup),
        user_change as fn(&UserChange),
        group_change as fn(&GroupChange),
        pull_request as fn(&PullRequest),
        fake_pull as fn(&FakePull),
        sink_context as fn(&SinkContext),
    );
    let update = ConnectionUpdate {
        name: Some("n".into()),
        settings: Some(json!({})),
        secret: Some(String::new()),
        sync_interval_mins: Some(0),
        status: Some(ConnectionStatus::Paused),
    };
    assert!(format!("{update:?}").contains("***"));
}

fn sample_run(now: DateTime<Utc>) -> SyncRun {
    SyncRun {
        id: Uuid::nil(),
        connection_id: Uuid::nil(),
        status: RunStatus::Held,
        trigger: RunTrigger::Schedule,
        dry_run: false,
        force: false,
        complete: Some(true),
        summary: RunSummary::default(),
        done: 0,
        total: 0,
        note: None,
        error: None,
        operation_id: None,
        created_by: None,
        created_at: now,
        started_at: None,
        finished_at: None,
    }
}

#[test]
fn events_constants_tables_and_codes_have_their_documented_names() {
    assert_eq!(DirectoryUserCreated::NAME, "directory.user_created");
    assert_eq!(DirectoryUserUpdated::NAME, "directory.user_updated");
    assert_eq!(DirectoryUserDeactivated::NAME, "directory.user_deactivated");
    assert_eq!(DirectorySyncFinished::NAME, "directory.sync_finished");
    assert_eq!(
        <SyncDirectory as navree_contracts::Job>::NAME,
        "directory.sync"
    );
    assert_eq!(MANAGE_PERMISSION, "directory.manage");
    assert_eq!(DIRECTORY_MIGRATIONS, "directory");
    assert_eq!(DIRECTORY_SCOPE_SET, "directory");
    assert_eq!(DirectoryScopes::NAME, "directory");
    assert_eq!(SCIM_TOKEN_PREFIX, "nvscim_");
    assert_eq!(BUILT_IN_SOURCES, [SCIM, MICROSOFT, GOOGLE, OKTA, LDAP]);
    assert_eq!(
        DirectoryScopes::scopes("okta"),
        ["okta.users.read", "okta.groups.read"]
    );
    for (table, name) in [
        (models::Connection::meta().table(), "directory_connections"),
        (models::User::meta().table(), "directory_users"),
        (models::Group::meta().table(), "directory_groups"),
        (models::Membership::meta().table(), "directory_memberships"),
        (models::Run::meta().table(), "directory_runs"),
        (models::RunChange::meta().table(), "directory_run_changes"),
    ] {
        assert_eq!(table, name);
    }
    for (error, code) in [
        (
            DirectoryError::UnknownSource("x".into()),
            "unknown_directory_source",
        ),
        (
            DirectoryError::InvalidSettings("x".into()),
            "invalid_directory_settings",
        ),
        (DirectoryError::PushOnly, "directory_push_only"),
        (DirectoryError::NotScim, "directory_not_scim"),
        (
            DirectoryError::ConnectionNotFound,
            "directory_connection_not_found",
        ),
        (DirectoryError::RunNotFound, "directory_run_not_found"),
        (DirectoryError::Paused, "directory_connection_paused"),
        (DirectoryError::SyncRunning, "directory_sync_running"),
        (DirectoryError::RunNotHeld, "directory_run_not_held"),
        (
            DirectoryError::source_failed("LDAP", "x"),
            "directory_source_failed",
        ),
    ] {
        assert_eq!(error.to_app_error().unwrap().code, code);
    }
}

#[test]
fn the_traits_and_extensions_have_their_documented_names() {
    fn _sink<S: DirectorySink>() {}
    fn _source<S: DirectorySource>() {}
    fn _ext(m: &mut navree_kernel::ModuleBuilder) {
        let _ = m.directory_source("hr", NoSource);
    }
    struct NoSource;
    impl DirectorySource for NoSource {
        fn pull<'a>(
            &'a self,
            _request: &'a PullRequest,
        ) -> navree_kernel::BoxFuture<'a, Result<Pulled, DirectoryError>> {
            Box::pin(async { Ok(Pulled::snapshot(Vec::new(), Vec::new())) })
        }
    }
    let _ = NoSource;
    let _ = DirectoryModule::for_root(DirectoryOptions::default());
    let _ = DirectoryModule::from_config();
    let _: fn(&DirectoryOwner) -> Owner = |owner| owner.0;
    let _ = std::any::type_name::<DirectoryController>();
    let _ = std::any::type_name::<ScimController>();
    let (directory, fake) = Directory::fake();
    assert!(directory.is_fake());
    let _: &DirectoryFake = fake
        .user(DirectoryUser::new("u", "u"))
        .users(Vec::new())
        .remove_user("u")
        .group(DirectoryGroup::new("g", "g"))
        .groups(Vec::new())
        .remove_group("g")
        .next_pull(Pulled::snapshot(Vec::new(), Vec::new()))
        .fail_next_pull("down");
    assert!(fake.snapshot().users.is_empty());
    assert!(fake.pulls().is_empty());
}

/// The service's documented methods, as signatures (exercised in
/// `tests/directory`).
#[allow(dead_code)]
async fn service_methods(
    directory: &Directory,
    owner: Owner,
    id: Uuid,
) -> Result<(), DirectoryError> {
    let _: Level = directory.level()?;
    let _: Vec<String> = directory.sources()?;
    let _: String = directory.scim_url()?;
    let _: CreatedConnection = directory
        .create_connection(
            owner,
            NewConnection::new("n", SCIM)
                .settings(json!({}))
                .secret("s")
                .interval(0)
                .created_by(id),
        )
        .await?;
    let _: DirectoryConnection = directory
        .update_connection(id, ConnectionUpdate::default())
        .await?;
    directory.delete_connection(id).await?;
    let _: u64 = directory.forget(owner).await?;
    let _: DirectoryConnection = directory.connection(id).await?;
    let _: DirectoryConnection = directory.connection_for(owner, id).await?;
    let _: Vec<DirectoryConnection> = directory.connections(owner).await?;
    let _: Vec<DirectoryConnection> = directory.all_connections().await?;
    let _: Secret<String> = directory.rotate_scim_token(id).await?;
    let _: SyncRun = directory.sync(id, SyncOptions::default()).await?;
    let _: SyncRun = directory.sync_now(id, SyncOptions::default()).await?;
    let _: Vec<SyncRun> = directory.sync_due().await?;
    let _: SyncRun = directory.approve(id).await?;
    let _: SyncRun = directory.run(id).await?;
    let _: Vec<SyncRun> = directory.runs(id, 20).await?;
    let _: Vec<SyncChange> = directory.changes(id, 0, 100).await?;
    let _: u64 = directory.prune_runs().await?;
    let _: Vec<SyncedUser> = directory.users(id).await?;
    let _: Option<SyncedUser> = directory.user(id, "u").await?;
    let _: Vec<SyncedGroup> = directory.groups(id).await?;
    let _: Vec<SyncedGroup> = directory.groups_of(id).await?;
    Ok(())
}

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
}

#[tokio::test]
async fn the_documented_routes_and_commands_are_registered() {
    let app = servers::database(TestApp::builder())
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
        .module(DirectoryModule::from_config())
        .build()
        .await;
    let routes: Vec<(String, String)> = navree_http::route_table(app.ctx())
        .await
        .unwrap()
        .into_iter()
        .filter(|route| route.full_path.contains("directory") || route.full_path.contains("scim"))
        .map(|route| (route.method, route.full_path))
        .collect();
    for (method, path) in [
        ("GET", "/api/directory/sources"),
        ("GET", "/api/directory/connections"),
        ("POST", "/api/directory/connections"),
        ("GET", "/api/directory/connections/{id}"),
        ("PATCH", "/api/directory/connections/{id}"),
        ("DELETE", "/api/directory/connections/{id}"),
        ("POST", "/api/directory/connections/{id}/token"),
        ("POST", "/api/directory/connections/{id}/sync"),
        ("GET", "/api/directory/connections/{id}/runs"),
        ("GET", "/api/directory/connections/{id}/users"),
        ("GET", "/api/directory/connections/{id}/groups"),
        ("GET", "/api/directory/runs/{id}"),
        ("GET", "/api/directory/runs/{id}/changes"),
        ("POST", "/api/directory/runs/{id}/approve"),
        ("GET", "/scim/v2/ServiceProviderConfig"),
        ("GET", "/scim/v2/ResourceTypes"),
        ("GET", "/scim/v2/ResourceTypes/{id}"),
        ("GET", "/scim/v2/Schemas"),
        ("GET", "/scim/v2/Schemas/{id}"),
        ("GET", "/scim/v2/Users"),
        ("POST", "/scim/v2/Users"),
        ("GET", "/scim/v2/Users/{id}"),
        ("PUT", "/scim/v2/Users/{id}"),
        ("PATCH", "/scim/v2/Users/{id}"),
        ("DELETE", "/scim/v2/Users/{id}"),
        ("GET", "/scim/v2/Groups"),
        ("POST", "/scim/v2/Groups"),
        ("GET", "/scim/v2/Groups/{id}"),
        ("PUT", "/scim/v2/Groups/{id}"),
        ("PATCH", "/scim/v2/Groups/{id}"),
        ("DELETE", "/scim/v2/Groups/{id}"),
    ] {
        assert!(
            routes.contains(&(method.to_owned(), path.to_owned())),
            "{method} {path} in {routes:?}"
        );
    }
    let commands: Vec<&str> = app.ctx().commands().iter().map(|c| c.name).collect();
    for command in [
        "directory:sync",
        "directory:runs",
        "directory:connections",
        "directory:token",
        "directory:prune",
    ] {
        assert!(commands.contains(&command), "{command}");
    }
}
