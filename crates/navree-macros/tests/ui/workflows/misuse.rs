use navree_macros::{action, condition, trigger, workflow};

pub struct WorkflowCtx;
pub struct AutomationCtx;
pub struct Onboarding;
pub struct Config;
pub struct Event;

/// Not async.
#[workflow]
fn not_async(_ctx: WorkflowCtx, _input: Onboarding) -> Result<(), String> {
    Ok(())
}

/// No `Result`.
#[workflow(name = "Onboarding Flow")]
async fn bad_name_and_output(_ctx: WorkflowCtx, _input: Onboarding) -> u32 {
    1
}

/// One argument.
#[workflow(version = 2, min_version = 3)]
async fn one_argument(_input: Onboarding) -> Result<(), String> {
    Ok(())
}

/// No type.
#[trigger(label = "Deal won")]
fn no_type(_config: &Config, _event: &Event) -> bool {
    true
}

/// A struct without its event.
#[trigger(type = "deal.won")]
pub struct NoEvent;

/// By value.
#[condition(type = "deal.big")]
fn by_value(_config: Config, _ctx: &AutomationCtx) -> bool {
    true
}

/// Not a `Result`.
#[action(type = "mail.send", subject = "owner_id")]
async fn not_a_result(_config: Config, _ctx: AutomationCtx) -> u32 {
    1
}

fn main() {}
