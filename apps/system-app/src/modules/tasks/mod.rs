//! Tasks: a workspace-scoped navree-orm model (`#[model(workspace)]`).
//! Every query and write is filtered by the request's workspace; routes
//! bind records by id (`#[can(.., from = "id")]`), so another workspace's
//! task is a 404.

mod controller;
mod migrations;
mod model;

use navree::prelude::*;

pub use controller::TasksController;
pub use model::{Task, TaskStatus};

/// Tasks, scoped to the request's workspace.
#[module(
    imports = [AccessModule::for_feature::<Task>()],
    controllers = [TasksController],
    register = Self::wire,
)]
pub struct TasksModule;

impl TasksModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations::migrations());
        m.model::<Task>();
    }
}
