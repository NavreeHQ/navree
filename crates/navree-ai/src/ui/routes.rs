//! The assistant's routes (see the [module's docs](super)).

use std::convert::Infallible;
use std::marker::PhantomData;
use std::sync::Arc;

use http::request::Parts;
use http::{Method, Uri};
use navree_access::CurrentAccess;
use navree_http::extract::FromRequestParts;
use navree_http::response::IntoResponse;
use navree_http::{AppError, Controller, Form, Path, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_ui::{Card, LazyQuery, Reply, Row, Stack, Text, ToastTone, View};
use serde::Deserialize;
use uuid::Uuid;

use super::host::{APPROVAL_ID, Acted, Assistant, COMPOSER_ID};
use crate::agent::Agent;

/// The session's CSRF token, when the app has sessions.
pub(crate) struct PageToken(pub(crate) Option<String>);

impl FromRequestParts for PageToken {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<navree_session::Session>()
                .map(navree_session::Session::csrf_token),
        ))
    }
}

/// The URL a request was sent to.
pub(crate) struct SentTo(pub(crate) Uri);

impl FromRequestParts for SentTo {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(Self(parts.uri.clone()))
    }
}

/// A message the composer posts.
#[derive(Debug, Deserialize)]
struct MessageForm {
    #[serde(default)]
    message: String,
    #[serde(default)]
    context: Option<String>,
}

/// An answer to the call that waits.
#[derive(Debug, Deserialize)]
struct DecisionForm {
    #[serde(default)]
    call_id: String,
    #[serde(default)]
    reason: Option<String>,
}

/// A component's button's post.
#[derive(Debug, Deserialize)]
struct ActForm {
    #[serde(default)]
    confirmed: Option<String>,
}

/// Serves an [`Assistant`]'s routes. Mounted by
/// [`m.assistant::<A>(..)`](super::ModuleBuilderAssistantExt::assistant).
pub struct AssistantController<A> {
    assistant: Arc<Assistant<A>>,
    _agent: PhantomData<fn() -> A>,
}

impl<A: Agent> Injectable for AssistantController<A> {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Assistant<A>>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                assistant: r.get::<Assistant<A>>().await?,
                _agent: PhantomData,
            })
        })
    }
}

/// The composer again, empty, after a message went; or with what was written and why it
/// was refused.
fn composer_reply<A: Agent>(
    assistant: &Assistant<A>,
    page: String,
    conversation: Uuid,
    token: Option<&str>,
    form: &MessageForm,
    error: Option<&AppError>,
) -> Reply {
    let why: Option<String> = error.map(|error| {
        let body = error.body();
        let text = body["detail"]
            .as_str()
            .or_else(|| body["message"].as_str())
            .unwrap_or("The message was not sent.");
        text.to_owned()
    });
    let composer = assistant.composer_node(
        Some(conversation),
        token,
        form.context
            .as_deref()
            .filter(|_| error.is_none_or(|e| e.status.as_u16() != 403)),
        error.map(|_| form.message.as_str()),
        why.as_deref(),
    );
    // A refusal is drawn in the composer: the ops still apply, so the answer is a 200.
    Reply::ops_or_redirect(page).morph(COMPOSER_ID, composer)
}

impl<A: Agent> Controller for AssistantController<A> {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let base = self.assistant.path().to_owned();
        let at = |path: &str| format!("!{base}{path}");
        let mut routes = Vec::new();

        let assistant = self.assistant.clone();
        routes.push(
            RouteDef::on(
                Method::POST,
                at(""),
                move |CurrentAccess(access): CurrentAccess, Form(form): Form<MessageForm>| {
                    let assistant = assistant.clone();
                    async move {
                        let id = assistant
                            .start(access, &form.message, form.context.as_deref())
                            .await?;
                        Ok::<_, AppError>(Reply::redirect(assistant.page_path(id)))
                    }
                },
            )
            .authenticated()
            .page()
            .name("assistant.start"),
        );

        let assistant = self.assistant.clone();
        routes.push(
            RouteDef::on(
                Method::POST,
                at("/{conversation}/messages"),
                move |CurrentAccess(access): CurrentAccess,
                      PageToken(token): PageToken,
                      Path(conversation): Path<Uuid>,
                      Form(form): Form<MessageForm>| {
                    let assistant = assistant.clone();
                    async move {
                        let page = assistant.page_path(conversation);
                        let sent = assistant
                            .send(access, conversation, &form.message, form.context.as_deref())
                            .await;
                        match sent {
                            Ok(()) => Ok(composer_reply(
                                &assistant,
                                page,
                                conversation,
                                token.as_deref(),
                                &form,
                                None,
                            )),
                            // The reader's own conversation, refused for what was sent: the
                            // composer says why and keeps what they wrote.
                            Err(error) if matches!(error.status.as_u16(), 400 | 403 | 409) => {
                                Ok(composer_reply(
                                    &assistant,
                                    page,
                                    conversation,
                                    token.as_deref(),
                                    &form,
                                    Some(&error),
                                ))
                            }
                            Err(error) => Err::<Reply, AppError>(error),
                        }
                    }
                },
            )
            .authenticated()
            .page()
            .name("assistant.send"),
        );

        let assistant = self.assistant.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                at("/{conversation}/items"),
                move |CurrentAccess(access): CurrentAccess,
                      PageToken(token): PageToken,
                      Path(conversation): Path<Uuid>,
                      ask: LazyQuery| {
                    let assistant = assistant.clone();
                    async move {
                        assistant
                            .items(access, conversation, ask.ask(), token.as_deref())
                            .await
                            .map(IntoResponse::into_response)
                    }
                },
            )
            .authenticated()
            .page()
            .name("assistant.items"),
        );

        let assistant = self.assistant.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                at("/{conversation}/approval"),
                move |CurrentAccess(access): CurrentAccess,
                      PageToken(token): PageToken,
                      Path(conversation): Path<Uuid>| {
                    let assistant = assistant.clone();
                    async move {
                        let region = assistant
                            .approval(access, conversation, token.as_deref())
                            .await?;
                        let mut response = View::new("Approval").body(region).into_response();
                        response.headers_mut().insert(
                            http::header::CACHE_CONTROL,
                            http::HeaderValue::from_static("no-store"),
                        );
                        Ok::<_, AppError>(response)
                    }
                },
            )
            .authenticated()
            .page()
            .name("assistant.approval"),
        );

        let assistant = self.assistant.clone();
        routes.push(
            RouteDef::on(
                Method::POST,
                at("/{conversation}/stop"),
                move |CurrentAccess(access): CurrentAccess, Path(conversation): Path<Uuid>| {
                    let assistant = assistant.clone();
                    async move {
                        let user = access.user.ok_or_else(AppError::unauthenticated)?;
                        assistant.stop(user, conversation).await?;
                        Ok::<_, AppError>(Reply::ops_or_redirect(assistant.page_path(conversation)))
                    }
                },
            )
            .authenticated()
            .page()
            .name("assistant.stop"),
        );

        for approve in [true, false] {
            let assistant = self.assistant.clone();
            routes.push(
                RouteDef::on(
                    Method::POST,
                    at(if approve {
                        "/{conversation}/approve"
                    } else {
                        "/{conversation}/reject"
                    }),
                    move |CurrentAccess(access): CurrentAccess,
                          PageToken(token): PageToken,
                          Path(conversation): Path<Uuid>,
                          Form(form): Form<DecisionForm>| {
                        let assistant = assistant.clone();
                        async move {
                            if approve {
                                assistant
                                    .approve(access.clone(), conversation, &form.call_id)
                                    .await?;
                            } else {
                                assistant
                                    .reject(
                                        access.clone(),
                                        conversation,
                                        &form.call_id,
                                        form.reason.as_deref(),
                                    )
                                    .await?;
                            }
                            let region = assistant
                                .approval(access, conversation, token.as_deref())
                                .await?;
                            Ok::<_, AppError>(
                                Reply::ops_or_redirect(assistant.page_path(conversation))
                                    .morph(APPROVAL_ID, region),
                            )
                        }
                    },
                )
                .authenticated()
                .page()
                .name(if approve {
                    "assistant.approve"
                } else {
                    "assistant.reject"
                }),
            );
        }

        let assistant = self.assistant.clone();
        routes.push(
            RouteDef::on(
                Method::POST,
                at("/act"),
                move |CurrentAccess(access): CurrentAccess,
                      PageToken(token): PageToken,
                      SentTo(uri): SentTo,
                      Form(form): Form<ActForm>| {
                    let assistant = assistant.clone();
                    async move {
                        let confirmed = form.confirmed.as_deref() == Some("1");
                        let (conversation, acted) = assistant.act(access, &uri, confirmed).await?;
                        let page = assistant.page_path(conversation);
                        Ok::<_, AppError>(match acted {
                            Acted::Done(message) => {
                                Reply::ops_or_redirect(page).toast(ToastTone::Success, message)
                            }
                            Acted::Ask { label } => Reply::view(confirm_page(
                                &label,
                                &uri,
                                &page,
                                token.as_deref(),
                            )),
                        })
                    }
                },
            )
            .authenticated()
            .page()
            .name("assistant.act"),
        );

        routes
    }
}

/// The question a destructive button asks when the page couldn't ask it in its own
/// dialog: the same signed post, answered.
fn confirm_page(label: &str, uri: &Uri, back: &str, token: Option<&str>) -> View {
    let action = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_owned(), |path| path.as_str().to_owned());
    let form = navree_ui::el("form")
        .attr("method", "post")
        .attr("action", action)
        .child(token.map(|token| navree_ui::form::csrf_field(token.to_owned())))
        .child(
            navree_ui::el("input")
                .attr("type", "hidden")
                .attr("name", "confirmed")
                .attr("value", "1"),
        )
        .child(navree_ui::Button::destructive(label.to_owned()).submit());
    View::new("Are you sure?").body(
        Card::titled("Are you sure?").body(Stack::gap(3).children((
            Text::new(format!("{label}: this can't be taken back.")),
            Row::gap(2).children((
                form,
                navree_ui::Button::outline("Cancel").href(back.to_owned()),
            )),
        ))),
    )
}
