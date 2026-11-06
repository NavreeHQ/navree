//! The page's tools in an assistant's run: one `show_` tool per component of the
//! catalogue the reader may be offered, and `update_state` when the conversation has a
//! working state. Every call is checked by the catalogue or the state's type before it
//! becomes a stream event; nothing a model sends reaches the page unchecked.

use std::sync::Arc;

use navree_access::PrincipalAccess;
use navree_ui::genui::{Catalog, Checker, LinkPattern, Prop as _, Props};
use serde_json::{Value, json};

use crate::agent::{AgentRun, PageAnswer, PageTool, PageTools};
use crate::stream::StreamEvent;

/// The tool that changes the working state.
pub(crate) const STATE_TOOL: &str = "update_state";

/// A working state's type, as the run needs it: its schema, its description, and its
/// check.
#[derive(Clone)]
pub(crate) struct StateSpec {
    pub(crate) description: String,
    pub(crate) schema: Value,
    pub(crate) check: fn(&Value) -> Result<Value, String>,
}

impl StateSpec {
    /// The spec of state type `T`.
    pub(crate) fn of<T: Props>() -> Self {
        Self {
            description: T::description(),
            schema: T::schema(),
            check: check_state::<T>,
        }
    }
}

/// Checks a state as `T`: what is stored, or what is wrong.
fn check_state<T: Props>(value: &Value) -> Result<Value, String> {
    let links: [LinkPattern; 0] = [];
    let mut checker = Checker::new(&links);
    match T::check(value, "", &mut checker) {
        Some(state) if checker.is_clean() => Ok(state.to_json()),
        _ => Err(checker
            .into_problems()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")),
    }
}

/// The page's tools for one run.
pub(crate) struct AssistantTools {
    pub(crate) catalog: Option<Arc<Catalog<AgentRun>>>,
    pub(crate) state: Option<StateSpec>,
}

impl PageTools for AssistantTools {
    fn tools(&self, access: &PrincipalAccess) -> Vec<PageTool> {
        let mut tools: Vec<PageTool> = self
            .catalog
            .iter()
            .flat_map(|catalog| catalog.all_tools())
            .filter(|tool| {
                tool.can
                    .is_none_or(|(action, subject)| access.ability.can(action, subject, None))
            })
            .map(|tool| PageTool {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
            })
            .collect();
        if let Some(state) = &self.state {
            tools.push(PageTool {
                name: STATE_TOOL.to_owned(),
                description: format!(
                    "Replaces the conversation's working state, which the user sees and edits on \
                     the page: {} Send the whole state.",
                    state.description
                ),
                parameters: state.schema.clone(),
            });
        }
        tools
    }

    fn prompt(&self) -> String {
        let mut prompt = self
            .catalog
            .as_ref()
            .map(|catalog| catalog.prompt())
            .unwrap_or_default();
        if self.state.is_some() {
            if !prompt.is_empty() {
                prompt.push_str("\n\n");
            }
            prompt.push_str(&format!(
                "The conversation has a working state the user sees and edits on the page. Read it \
                 below; change it only with `{STATE_TOOL}`."
            ));
        }
        prompt
    }

    fn answer(&self, tool: &str, arguments: &Value) -> PageAnswer {
        if tool == STATE_TOOL
            && let Some(state) = &self.state
        {
            return match (state.check)(arguments) {
                Ok(value) => PageAnswer::Accepted {
                    result: json!({ "saved": true, "note": "The user sees the new state on the page." }),
                    event: StreamEvent::State(value),
                },
                Err(message) => PageAnswer::Refused { message },
            };
        }
        let Some(catalog) = &self.catalog else {
            return PageAnswer::Refused {
                message: format!("there is no tool `{tool}`"),
            };
        };
        match catalog.check(tool, arguments) {
            Ok(checked) => PageAnswer::Accepted {
                result: json!({
                    "shown": checked.surface,
                    "note": "The user sees it now, drawn from their own data, which you don't see. Don't repeat its numbers."
                }),
                event: StreamEvent::Surface {
                    id: String::new(),
                    surface: checked.surface.to_owned(),
                    props: checked.props,
                },
            },
            Err(refusal) => PageAnswer::Refused {
                message: format!("The arguments were refused: {}", refusal.message()),
            },
        }
    }

    fn most_shown(&self) -> usize {
        self.catalog
            .as_ref()
            .map_or(6, |catalog| catalog.bounds().max_per_answer)
    }

    fn most_tries(&self) -> usize {
        self.catalog
            .as_ref()
            .map_or(2, |catalog| catalog.bounds().max_tries)
    }
}
