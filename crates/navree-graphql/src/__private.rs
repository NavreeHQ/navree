//! What `#[resolver]` expansions call. Not a public API: names and
//! signatures change without notice.

use std::borrow::Cow;
use std::future::Future;
use std::sync::Arc;

use async_graphql::{Context, Error, InputType, InputValueError, InputValueResult, Value};
use garde::Validate;
use navree_db::Uow;
use navree_http::AppError;

pub use crate::root::{ErasedObject, ErasedSubscription};
pub use navree_access::{ActionMarker, Subject};

use crate::context::operation;
use crate::operation::Operation;

/// What kind of root field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// A `#[query]`.
    Query,
    /// A `#[mutation]`.
    Mutation,
    /// A `#[subscription]`.
    Subscription,
}

impl FieldKind {
    /// `query`, `mutation` or `subscription`.
    pub fn as_str(self) -> &'static str {
        match self {
            FieldKind::Query => "query",
            FieldKind::Mutation => "mutation",
            FieldKind::Subscription => "subscription",
        }
    }
}

/// A field's access declaration.
#[derive(Clone, Copy, Debug)]
pub struct FieldAccess {
    /// `#[public]`.
    pub public: bool,
    /// `#[authenticated]`.
    pub authenticated: bool,
    /// `#[roles(..)]`.
    pub roles: &'static [&'static str],
    /// `#[permissions(..)]`.
    pub permissions: &'static [&'static str],
    /// `#[can(..)]`.
    pub can: Option<CanAccess>,
}

impl FieldAccess {
    /// Whether the route authorizer checks it (roles, permissions or an
    /// ability).
    pub fn needs_authorizer(&self) -> bool {
        !self.roles.is_empty() || !self.permissions.is_empty() || self.can.is_some()
    }
}

/// `#[can(Action, Subject, from = "id")]`.
#[derive(Clone, Copy, Debug)]
pub struct CanAccess {
    /// The action, e.g. `read`.
    pub action: &'static str,
    /// The subject, e.g. `Deal`.
    pub subject: &'static str,
    /// The argument holding the record id.
    pub from: Option<&'static str>,
}

/// One field of a resolver, for the boot checks.
#[derive(Clone, Copy, Debug)]
pub struct FieldSpec {
    /// The method name.
    pub name: &'static str,
    /// Query, mutation or subscription.
    pub kind: FieldKind,
    /// Its access declaration.
    pub access: FieldAccess,
}

/// The objects a resolver contributes to the roots.
pub struct ResolverParts {
    /// Its `#[query]` fields.
    pub query: Option<Arc<dyn ErasedObject>>,
    /// Its `#[mutation]` fields.
    pub mutation: Option<Arc<dyn ErasedObject>>,
    /// Its `#[subscription]` fields.
    pub subscription: Option<Arc<dyn ErasedSubscription>>,
    /// Every field with its access declaration.
    pub fields: Vec<FieldSpec>,
}

/// The GraphQL type name of a resolver's part: unique per resolver type
/// (the part never shows in the schema; its fields move to the root).
pub fn part_type_name<R: ?Sized>(kind: &str) -> Cow<'static, str> {
    let rust: String = std::any::type_name::<R>()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    Cow::Owned(format!("__Navree_{rust}_{kind}"))
}

/// The text of an argument, for `can(.., from = "x")`.
pub fn arg_text<T: InputType>(value: &T) -> String {
    match value.to_value() {
        Value::String(text) => text,
        Value::Enum(name) => name.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

tokio::task_local! {
    /// The state of the field whose method is running.
    pub(crate) static FIELD: FieldScope;
}

/// What a running field method can reach besides the context.
#[derive(Clone, Default)]
pub(crate) struct FieldScope {
    /// The request extensions after the field's access check (the record of
    /// `can(.., from = ..)`).
    pub(crate) authorized: Option<Arc<http::Extensions>>,
    /// The mutation's own unit of work.
    pub(crate) uow: Option<Uow>,
}

/// One resolving root field: checked, then run.
pub struct Field<'a> {
    op: &'a Arc<Operation>,
    kind: FieldKind,
    authorized: Option<http::Extensions>,
    uow: Option<Uow>,
}

impl<'a> Field<'a> {
    /// Checks `access` for the field `ctx` resolves (`UNAUTHENTICATED` for
    /// guests on a non-public field, the route authorizer's error
    /// otherwise). A mutation gets its own unit of work, which the check
    /// uses to load the record.
    pub async fn begin(
        ctx: &Context<'a>,
        kind: FieldKind,
        access: &FieldAccess,
        args: &[(&str, String)],
    ) -> Result<Field<'a>, Error> {
        let op = operation(ctx)?;
        if !access.public && op.principal.is_none() {
            return Err(AppError::unauthenticated().into());
        }
        let uow = match kind {
            FieldKind::Mutation => {
                op.release_reads().await;
                op.new_uow().await
            }
            FieldKind::Query | FieldKind::Subscription => None,
        };
        let authorized = if access.needs_authorizer() {
            let path = ctx
                .path_node
                .as_ref()
                .map_or_else(String::new, ToString::to_string);
            let check_uow = uow.clone().or_else(|| op.reads());
            match op.authorize(path, access, args, check_uow.as_ref()).await {
                Ok(extensions) => Some(extensions),
                Err(error) => {
                    if let Some(uow) = &uow
                        && let Err(rollback) = uow.rollback().await
                    {
                        tracing::warn!(target: "navree::graphql", error = %rollback, "rolling back a refused mutation failed");
                    }
                    return Err(error.into());
                }
            }
        } else {
            None
        };
        Ok(Field {
            op,
            kind,
            authorized,
            uow,
        })
    }

    /// Runs the field's method (`fut`, which also validates its `Valid`
    /// arguments) with its unit of work as `Uow::current`. A mutation's
    /// unit of work commits when `fut` returns `Ok` and rolls back
    /// otherwise; a subscription ends the reads its setup began.
    pub async fn run<T>(self, fut: impl Future<Output = Result<T, Error>>) -> Result<T, Error> {
        let scope = FieldScope {
            authorized: self.authorized.map(Arc::new),
            uow: self.uow.clone(),
        };
        let current = self.uow.clone().or_else(|| self.op.reads());
        let fut = FIELD.scope(scope, fut);
        let result = match &current {
            Some(uow) => uow.scope(fut).await,
            None => fut.await,
        };
        match (self.kind, &self.uow) {
            (FieldKind::Mutation, Some(uow)) => match &result {
                Ok(_) => {
                    if let Err(error) = uow.commit().await {
                        return Err(AppError::internal(error).into());
                    }
                }
                Err(_) => {
                    if let Err(error) = uow.rollback().await {
                        tracing::warn!(target: "navree::graphql", %error, "rolling back a failed mutation failed");
                    }
                }
            },
            (FieldKind::Subscription, _) => self.op.release_reads().await,
            _ => {}
        }
        result
    }
}

/// A `Valid<T>` argument as async-graphql parses it: exactly `T` in the
/// schema, with the raw input kept for the validation messages.
pub struct ValidArg<T> {
    value: T,
    input: Option<serde_json::Value>,
}

impl<T: InputType> InputType for ValidArg<T> {
    type RawValueType = T::RawValueType;

    fn type_name() -> Cow<'static, str> {
        T::type_name()
    }

    fn qualified_type_name() -> String {
        T::qualified_type_name()
    }

    fn create_type_info(registry: &mut async_graphql::registry::Registry) -> String {
        T::create_type_info(registry)
    }

    fn parse(value: Option<Value>) -> InputValueResult<Self> {
        let input = value
            .clone()
            .and_then(|value| value.into_json().ok())
            .map(snake_case_keys);
        T::parse(value)
            .map(|value| ValidArg { value, input })
            .map_err(InputValueError::propagate)
    }

    fn to_value(&self) -> Value {
        self.value.to_value()
    }

    fn as_raw_value(&self) -> Option<&Self::RawValueType> {
        self.value.as_raw_value()
    }
}

/// Runs `T`'s garde rules on the argument `name`: the value, or a
/// `BAD_REQUEST` error with every message and the field errors under
/// `name.<field>` (messages from the app's validation messages).
pub fn validate<T: Validate<Context = ()>>(
    ctx: &Context<'_>,
    name: &str,
    arg: ValidArg<T>,
) -> Result<T, Error> {
    let op = operation(ctx)?;
    match navree_validation::validate(&arg.value, arg.input.as_ref(), &op.shared.messages, name) {
        Ok(()) => Ok(arg.value),
        Err(errors) => {
            let prefixed = errors
                .into_map()
                .into_iter()
                .map(|(field, messages)| {
                    let key = if field == name {
                        field
                    } else {
                        format!("{name}.{field}")
                    };
                    (key, messages)
                })
                .collect();
            Err(AppError::validation(prefixed).into())
        }
    }
}

/// `lostReason` → `lost_reason` in every object key (GraphQL input names
/// are camelCase; validation paths are the Rust field names).
fn snake_case_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(key, value)| (snake_case(&key), snake_case_keys(value)))
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(snake_case_keys).collect())
        }
        other => other,
    }
}

fn snake_case(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_names_are_valid_graphql_names() {
        let name = part_type_name::<Vec<u8>>("Query");
        assert!(name.starts_with("__Navree_"), "{name}");
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        assert!(name.ends_with("_Query"));
    }

    #[test]
    fn keys_turn_snake_case() {
        let value = serde_json::json!({ "lostReason": "x", "items": [{ "unitPrice": 1 }] });
        assert_eq!(
            snake_case_keys(value),
            serde_json::json!({ "lost_reason": "x", "items": [{ "unit_price": 1 }] })
        );
    }

    #[test]
    fn arguments_turn_into_text() {
        assert_eq!(arg_text(&String::from("abc")), "abc");
        assert_eq!(arg_text(&42_i32), "42");
        let id = uuid::Uuid::now_v7();
        assert_eq!(arg_text(&id), id.to_string());
    }
}
