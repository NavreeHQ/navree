//! The commands `access:role`, `access:grant` and `access:explain`.

use navree_db::Db;
use navree_kernel::{AppContext, BoxError, CommandSpec};
use uuid::Uuid;

use crate::error::AccessError;
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
    let usage = "usage: access:explain <user> <action> <subject> [id]";
    let (user, action, subject, id) = match args.as_slice() {
        [user, action, subject] => (user, action, subject, None),
        [user, action, subject, id] => (user, action, subject, Some(id.as_str())),
        _ => return Err(usage.into()),
    };
    let user = Uuid::parse_str(user).map_err(|_| format!("`{user}` is not a user id (UUID)"))?;
    let service = ctx.get::<AccessService>().await?;
    let explanation = service.explain(user, action, subject, id).await?;
    println!("{explanation}");
    Ok(())
}
