//! `cargo loco start`, without the CLI front end.
//!
//! The same steps `loco_rs::cli::main` takes for `start` in server-only mode:
//! load `config/<LOCO_ENV>.yaml`, start Loco's logger, boot the app (with
//! `auto_migrate: false` there is nothing to converge) and serve on
//! `server.binding:server.port`. The one difference: `cli::main` builds an
//! `AppContext` to read the config and then `create_app` builds a second one,
//! so the CLI process keeps an extra database pool open beside the serving
//! one. Booting directly gives one pool of `database.max_connections`.
use bench_loco::app::App;
use loco_rs::{
    app::Hooks,
    boot::{self, ServeParams, StartMode},
    environment::{self, Environment},
    logger,
};

#[tokio::main]
async fn main() -> loco_rs::Result<()> {
    let environment: Environment = environment::resolve_from_env().into();
    let config = App::load_config(&environment).await?;
    logger::init::<App>(&config.logger)?;

    let serve_params = ServeParams {
        port: config.server.port,
        binding: config.server.binding.clone(),
    };
    let boot_result = App::boot(StartMode::ServerOnly, &environment, config).await?;
    boot::start::<App>(boot_result, serve_params, false).await
}
