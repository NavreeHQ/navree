//! What the `#[mcp_server]` expansion names (`navree::mcp::__private::…`).
//! Not a public API: it changes with the macro.

use std::str::FromStr;

use navree_access::Authorized;
use sea_orm::EntityTrait;
use serde::Serialize;

pub use navree_access::{ActionMarker, Subject};
pub use navree_http::AppError;

pub use crate::item::{Call, McpItem, McpOutput, McpServer, PromptDef, ResourceDef, ToolDef};

use crate::content::{PromptMessage, ResourceContents};

/// A serializable result (tools, resources, prompts returning a value).
pub fn json_output<T, E>(result: Result<T, E>) -> Result<McpOutput, AppError>
where
    T: Serialize,
    E: Into<AppError>,
{
    let value = result.map_err(Into::into)?;
    serde_json::to_value(value)
        .map(McpOutput::Json)
        .map_err(AppError::internal)
}

/// A resource returning [`ResourceContents`] (one or many).
pub fn contents_output<C, E>(result: Result<C, E>) -> Result<McpOutput, AppError>
where
    C: IntoContents,
    E: Into<AppError>,
{
    Ok(McpOutput::Contents(result.map_err(Into::into)?.into_contents()))
}

/// A prompt's messages.
pub fn prompt_output<M, E>(result: Result<M, E>) -> Result<McpOutput, AppError>
where
    M: IntoMessages,
    E: Into<AppError>,
{
    Ok(McpOutput::Messages(result.map_err(Into::into)?.into_messages()))
}

/// What a resource method may return besides a serializable value.
pub trait IntoContents {
    /// The contents.
    fn into_contents(self) -> Vec<ResourceContents>;
}

impl IntoContents for ResourceContents {
    fn into_contents(self) -> Vec<ResourceContents> {
        vec![self]
    }
}

impl IntoContents for Vec<ResourceContents> {
    fn into_contents(self) -> Vec<ResourceContents> {
        self
    }
}

/// What a prompt method may return.
pub trait IntoMessages {
    /// The messages.
    fn into_messages(self) -> Vec<PromptMessage>;
}

impl IntoMessages for Vec<PromptMessage> {
    fn into_messages(self) -> Vec<PromptMessage> {
        self
    }
}

impl IntoMessages for PromptMessage {
    fn into_messages(self) -> Vec<PromptMessage> {
        vec![self]
    }
}

impl IntoMessages for String {
    fn into_messages(self) -> Vec<PromptMessage> {
        vec![PromptMessage::user(self)]
    }
}

impl IntoMessages for &'static str {
    fn into_messages(self) -> Vec<PromptMessage> {
        vec![PromptMessage::user(self)]
    }
}

/// A parameter filled with the record of a `can(.., from = ..)` check.
pub trait FromRecord: Sized {
    /// The record of `call`.
    fn from_record(call: &Call) -> Result<Self, AppError>;
}

impl<E, A> FromRecord for Authorized<E, A>
where
    E: EntityTrait + Subject,
    E::Model: Sync,
    A: ActionMarker,
{
    fn from_record(call: &Call) -> Result<Self, AppError> {
        call.record::<E, A>()
    }
}

/// The URI variable `name` of `call` as a `T`.
pub fn param<T: FromStr>(call: &Call, name: &str) -> Result<T, AppError> {
    call.param(name)
}
