//! The `ai` feature through the facade: an agent written with `navree::`
//! paths only (as `navree make:agent` writes it), `#[can]` markers from the
//! prelude, and `AiModule` with the fake.
//!
//! Run with `cargo test -p navree --features ai --test ai`.
#![cfg(feature = "ai")]

use navree::ai::utoipa::{self, ToSchema};
use navree::ai::{Agent, AgentOptions, AgentRun, Ai, FakeReply, agent};
use navree::prelude::*;
use navree::validation::Validate;
use serde::{Deserialize, Serialize};

/// A stand-in subject for `#[can]`.
struct Note;

impl Subject for Note {
    const SUBJECT: &'static str = "Note";
    const PERMISSION_PREFIX: &'static str = "notes";
}

/// The arguments of `lookup`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct Lookup {
    /// What to look up, in the user's words.
    #[validate(length(min = 1, max = 200))]
    pub query: String,
}

/// What `lookup` returns to the model.
#[derive(Debug, Serialize)]
pub struct Found {
    /// The query, echoed.
    pub query: String,
}

/// An agent of the notes module.
#[injectable]
pub struct Assistant;

#[agent]
impl Agent for Assistant {
    const NAME: &'static str = "notes-assistant";

    fn instructions(&self, run: &AgentRun) -> String {
        format!("You help {}.", run.user().name)
    }

    fn options(&self) -> AgentOptions {
        AgentOptions {
            max_steps: 8,
            ..AgentOptions::default()
        }
    }

    /// Looks something up for the user.
    #[tool(read_only)]
    async fn lookup(&self, run: &AgentRun, args: Lookup) -> Result<Found> {
        let _ = run.user_id();
        Ok(Found { query: args.query })
    }

    /// Deletes every note.
    #[tool(needs_approval)]
    #[can(Manage, Note)]
    async fn purge(&self, _run: &AgentRun) -> Result<u64> {
        Ok(0)
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.agent::<Assistant>();
    }
}

#[test]
fn the_agent_describes_its_tools() {
    let tools = <Assistant as Agent>::tool_specs();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "lookup");
    assert_eq!(tools[0].description, "Looks something up for the user.");
    assert!(tools[0].read_only);
    assert_eq!(
        tools[1].access,
        [navree::ai::ToolAccess::can("manage", "Note")]
    );
}

#[test]
fn ai_module_and_agents_wire_with_the_prelude() {
    let report = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("ai.fake", serde_json::json!(true)),
        ))
        .module(AiModule::from_config())
        .module(NotesModule)
        .validate()
        .expect("the graph is valid");
    assert!(report.modules.iter().any(|m| m.name == "AiModule"));
}

#[tokio::test]
async fn the_fake_answers_through_the_facade() {
    let fake = navree::ai::AiFake::new();
    let ai: Ai = fake.ai();
    fake.respond(FakeReply::text("hello"));
    assert_eq!(ai.text("hi").model("fast").await.unwrap().text, "hello");
}
