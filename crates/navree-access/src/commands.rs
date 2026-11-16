//! The commands `access:role`, `access:grant` and `access:explain`.

use navree_contracts::permission_catalog;
use navree_db::Db;
use navree_kernel::{AppContext, BoxError, CommandSpec};
use uuid::Uuid;

use crate::error::AccessError;
use crate::factory::AbilityFactory;
use crate::service::AccessService;

/// Every command of the access module.
pub(crate) fn commands() -> [CommandSpec; 3] {
    [
        CommandSpec::new(
            "access:role",
            "Create a role: access:role <name> [description]",
            role_command,
        ),
        CommandSpec::new(
            "access:grant",
            "Give a role a permission: access:grant <role> <permission>",
            grant_command,
        ),
        CommandSpec::new(
            "access:explain",
            "Show the rule deciding a check: access:explain <user> <action> <subject> [id]",
            explain_command,
        ),
    ]
}

async fn role_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let [name, rest @ ..] = args.as_slice() else {
        return Err("usage: access:role <name> [description]".into());
    };
    let description = (!rest.is_empty()).then(|| rest.join(" "));
    let (service, db) = (ctx.get::<AccessService>().await?, ctx.get::<Db>().await?);
    let uow = db.uow(None, None).await;
    match service
        .create_role(&uow, name, description.as_deref())
        .await
    {
        Ok(_) => {
            uow.commit().await?;
            println!("Role `{name}` created.");
            Ok(())
        }
        Err(AccessError::RoleExists(_)) => {
            uow.rollback().await?;
            println!("Role `{name}` already exists.");
            Ok(())
        }
        Err(error) => {
            uow.rollback().await?;
            Err(error.into())
        }
    }
}

async fn grant_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let [role, permission] = args.as_slice() else {
        return Err("usage: access:grant <role> <permission>".into());
    };
    let (service, db) = (ctx.get::<AccessService>().await?, ctx.get::<Db>().await?);
    let uow = db.uow(None, None).await;
    match service.grant(&uow, role, permission).await {
        Ok(granted) => {
            uow.commit().await?;
            if granted {
                println!("Role `{role}` can now `{permission}`.");
            } else {
                println!("Role `{role}` already has `{permission}`.");
            }
            Ok(())
        }
        Err(error) => {
            uow.rollback().await?;
            Err(error.into())
        }
    }
}

async fn explain_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    println!("{}", explain_text(&ctx, &args).await?);
    Ok(())
}

/// What `access:explain` prints: the explanation, then one line for each
/// permission a module describes with `m.permissions` (in catalog order)
/// that stands for the explained action on the explained subject, with its
/// description. With no such permission, the explanation alone.
pub(crate) async fn explain_text(ctx: &AppContext, args: &[String]) -> Result<String, BoxError> {
    let usage = "usage: access:explain <user> <action> <subject> [id]";
    let (user, action, subject, id) = match args {
        [user, action, subject] => (user, action, subject, None),
        [user, action, subject, id] => (user, action, subject, Some(id.as_str())),
        _ => return Err(usage.into()),
    };
    let user = Uuid::parse_str(user).map_err(|_| format!("`{user}` is not a user id (UUID)"))?;
    let service = ctx.get::<AccessService>().await?;
    let explanation = service.explain(user, action, subject, id).await?;
    let mut text = explanation.to_string();
    let factory = ctx.get::<AbilityFactory>().await?;
    let explained = (explanation.action.as_str(), explanation.subject.as_str());
    for (_, permission) in permission_catalog(ctx) {
        if factory.permission_target(permission.name) == explained {
            text.push_str(&format!(
                "\nPermission `{}`: {}",
                permission.name, permission.description
            ));
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use navree_contracts::{ModuleBuilderPermissionsExt, Permission};
    use navree_kernel::{Module, ModuleBuilder};
    use navree_testing::TestApp;

    use super::*;
    use crate::AccessModule;

    /// A module that describes `announcements.publish`.
    struct Announcements;

    impl Module for Announcements {
        fn register(&self, m: &mut ModuleBuilder) {
            m.permissions(&[Permission::new(
                "announcements.publish",
                "Publish an announcement to the workspace",
            )]);
        }
    }

    #[tokio::test]
    async fn access_explain_prints_a_permissions_description() {
        let app = TestApp::builder()
            .memory_database()
            .module(navree_db::DatabaseModule)
            .module(AccessModule)
            .module(Announcements)
            .build()
            .await;
        let ctx = app.ctx();
        let user = Uuid::now_v7();
        let args = |action: &str| [user.to_string(), action.into(), "announcements".into()];

        let text = explain_text(ctx, &args("publish")).await.unwrap();
        assert_eq!(
            text,
            format!(
                "User {user} may not `publish` announcements\n\
                 No rule matches: denied by default.\n\
                 Roles: (none)\n\
                 Permission `announcements.publish`: Publish an announcement to the workspace"
            )
        );

        // No described permission stands for `read`: the output of before.
        let text = explain_text(ctx, &args("read")).await.unwrap();
        let service = app.get::<AccessService>().await;
        let explanation = service
            .explain(user, "read", "announcements", None)
            .await
            .unwrap();
        assert_eq!(text, explanation.to_string());
        assert_eq!(
            text,
            format!(
                "User {user} may not `read` announcements\n\
                 No rule matches: denied by default.\n\
                 Roles: (none)"
            )
        );
    }
}
