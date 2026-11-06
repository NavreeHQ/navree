//! The pages a signed-in browser reaches: setting up two-factor
//! authentication (`/account/two-factor`) and going back to one's own
//! sign-in after impersonating a user (`POST /impersonate/stop`).
//!
//! A user whom a two-factor policy requires to set it up reaches nothing
//! else: after signing in, the pages send them to the setup page.

use http::Method;
use navree_http::extract::Query;
use navree_http::{AppError, CurrentPrincipal, IntoResponse, RouteDef};
use navree_session::Session;
use navree_ui::form::{FormView, Rejected};
use navree_ui::html::SafeHtml;
use navree_ui::{Alert, AlertTone, Form, Node, Reply, Stack, Submit, Text, Validate, el};
use serde::Deserialize;

use super::{PagesController, Paths, link, page};
use crate::session_auth::{
    STOP_IMPERSONATING_PATH, TWO_FACTOR_CONFIRM_PATH, TWO_FACTOR_SETUP_PATH,
};
use crate::{
    AuthError, AuthService, ClientMeta, TwoFactorSetup, not_while_impersonating, two_factor_enabled,
};

/// The button that starts the setup.
#[derive(Debug, Deserialize, Validate, Form)]
struct SetupStartForm {
    #[validate(skip)]
    #[form(widget = hidden)]
    #[serde(default)]
    next: Option<String>,
}

/// The first code from the authenticator app.
#[derive(Debug, Deserialize, Validate, Form)]
struct SetupConfirmForm {
    /// Code from your authenticator app
    #[validate(length(min = 1, max = 16))]
    #[form(autofocus, error = "Enter the code your authenticator app shows now.")]
    code: String,
    #[validate(skip)]
    #[form(widget = hidden)]
    #[serde(default)]
    next: Option<String>,
}

/// `?next=`.
#[derive(Debug, Default, Deserialize)]
struct NextQuery {
    #[serde(default)]
    next: Option<String>,
}

/// `next` when it is a page of this site other than the sign-in pages.
fn safe_next(paths: &Paths, next: Option<&str>) -> Option<String> {
    next.and_then(|next| navree_ui::pages::ReturnTo::parse_for(next, &paths.sign_in))
        .map(|next| next.as_str().to_owned())
}

/// The page that says where two-factor authentication stands.
fn status_page(paths: &Paths, enabled: bool, form: FormView<SetupStartForm>) -> navree_ui::View {
    if enabled {
        return page(
            paths,
            "Two-factor authentication",
            "It is on: signing in asks for a code from your authenticator app.",
            Stack::gap(4)
                .children((
                    Alert::new(AlertTone::Success).title("Two-factor authentication is on."),
                    link("Continue", &paths.home),
                ))
                .into(),
        );
    }
    page(
        paths,
        "Set up two-factor authentication",
        "Signing in will ask for a code from an authenticator app on your phone, as well as your password.",
        Stack::gap(4)
            .children((
                Node::from(form.submit("Set it up")),
                link("Not now", &paths.home),
            ))
            .into(),
    )
}

/// A new key for a setup that starts again after a refused code. The page that shows it
/// answers 422, and the request's own unit of work rolls back with a 4xx answer, so the
/// key is written in a unit of work of its own: the code typed from it next is checked
/// against it.
async fn new_key(
    service: &AuthService,
    user: uuid::Uuid,
    meta: &ClientMeta,
) -> Result<TwoFactorSetup, AppError> {
    service
        .own(Some(user), None, |_| service.enable_two_factor(user, meta))
        .await
        .map_err(AuthError::into_app_error)
}

/// The page with the secret to scan or type, and the form for the first code.
fn setup_page(
    paths: &Paths,
    setup: &TwoFactorSetup,
    form: FormView<SetupConfirmForm>,
    mistake: bool,
) -> navree_ui::View {
    let mistake = mistake.then(|| {
        Alert::new(AlertTone::Danger)
            .title("That code didn't work.")
            .body(Text::new(
                "We made a new key. Scan it again, then type the code your app shows now.",
            ))
    });
    page(
        paths,
        "Set up two-factor authentication",
        "Scan the picture with an authenticator app, or type the key into it, then enter the code it shows.",
        Stack::gap(4)
            .children((
                mistake,
                el("div")
                    .attr("role", "img")
                    .attr("aria-label", "QR code for your authenticator app")
                    .child(SafeHtml::trusted(setup.qr_svg.clone())),
                Text::muted("Or type this key into the app:"),
                Text::code(setup.secret.clone()),
                Node::from(form.submit("Turn it on")),
            ))
            .into(),
    )
}

/// The recovery codes, shown once.
fn codes_page(paths: &Paths, codes: &[String], next: &str) -> navree_ui::View {
    page(
        paths,
        "Save your recovery codes",
        "Each code works once, if you lose your phone. They are shown only now.",
        Stack::gap(4)
            .children((
                Alert::new(AlertTone::Success).title("Two-factor authentication is on."),
                el("ul").children(
                    codes
                        .iter()
                        .map(|code| el("li").child(Text::code(code.clone())))
                        .collect::<Vec<_>>(),
                ),
                link("I have saved them, continue", next),
            ))
            .into(),
    )
}

impl PagesController {
    /// The setup page of two-factor authentication, and the way back from
    /// impersonating.
    pub(super) fn account_routes(&self) -> Vec<RouteDef> {
        let paths = self.paths.clone();
        let absolute = |path: &str| format!("!{path}");
        let mut routes = Vec::new();

        // GET two-factor setup
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute(TWO_FACTOR_SETUP_PATH),
                move |CurrentPrincipal(me): CurrentPrincipal,
                      session: Session,
                      Query(query): Query<NextQuery>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let user = service
                            .find_user(me.user_id)
                            .await
                            .map_err(AuthError::into_app_error)?;
                        let enabled = user.as_ref().is_some_and(two_factor_enabled);
                        let form = FormView::<SetupStartForm>::new()
                            .post(TWO_FACTOR_SETUP_PATH)
                            .csrf(session.csrf_token())
                            .of(&SetupStartForm {
                                next: safe_next(&paths, query.next.as_deref()),
                            });
                        Ok::<_, AppError>(status_page(&paths, enabled, form).into_response())
                    }
                },
            )
            .authenticated()
            .page()
            .guard(not_while_impersonating())
            .name("auth.pages.two_factor_setup"),
        );

        // POST two-factor setup: a new secret
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(TWO_FACTOR_SETUP_PATH),
                move |CurrentPrincipal(me): CurrentPrincipal,
                      session: Session,
                      meta: ClientMeta,
                      submitted: Result<Submit<SetupStartForm>, Rejected<SetupStartForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => {
                                return Ok(rejected.respond(move |form| {
                                    status_page(&paths, false, form.csrf(token))
                                }));
                            }
                        };
                        let next = safe_next(&paths, input.next.as_deref());
                        match service.enable_two_factor(me.user_id, &meta).await {
                            Ok(setup) => {
                                let form = FormView::<SetupConfirmForm>::new()
                                    .post(TWO_FACTOR_CONFIRM_PATH)
                                    .csrf(token)
                                    .of(&SetupConfirmForm {
                                        code: String::new(),
                                        next,
                                    });
                                Ok::<_, AppError>(
                                    setup_page(&paths, &setup, form, false).into_response(),
                                )
                            }
                            // Already on: the page says so.
                            Err(AuthError::TwoFactorAlreadyEnabled) => Ok(Reply::redirect(
                                paths.with_next(TWO_FACTOR_SETUP_PATH, next.as_deref()),
                            )
                            .into_response()),
                            Err(error) => Err(error.into_app_error()),
                        }
                    }
                },
            )
            .authenticated()
            .page()
            .guard(not_while_impersonating())
            .name("auth.pages.two_factor_setup.start"),
        );

        // POST two-factor setup: the first code
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(TWO_FACTOR_CONFIRM_PATH),
                move |CurrentPrincipal(me): CurrentPrincipal,
                      session: Session,
                      meta: ClientMeta,
                      submitted: Result<Submit<SetupConfirmForm>, Rejected<SetupConfirmForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => {
                                // The secret is not on this page any more: a
                                // new one starts the setup again.
                                let setup = new_key(&service, me.user_id, &meta).await?;
                                return Ok(rejected.respond(move |form| {
                                    setup_page(&paths, &setup, form.csrf(token), false)
                                }));
                            }
                        };
                        let next = safe_next(&paths, input.next.as_deref());
                        match service
                            .confirm_two_factor(me.user_id, input.code.trim(), &meta)
                            .await
                        {
                            Ok(codes) => {
                                let target = paths.after(next.as_deref());
                                Ok::<_, AppError>(
                                    codes_page(&paths, &codes, target.as_str()).into_response(),
                                )
                            }
                            // A wrong code: the setup begins again with a new key.
                            Err(AuthError::Invalid { .. }) => {
                                let setup = new_key(&service, me.user_id, &meta).await?;
                                let form = FormView::<SetupConfirmForm>::new()
                                    .post(TWO_FACTOR_CONFIRM_PATH)
                                    .csrf(token)
                                    .of(&SetupConfirmForm {
                                        code: String::new(),
                                        next,
                                    });
                                let mut response =
                                    setup_page(&paths, &setup, form, true).into_response();
                                *response.status_mut() = http::StatusCode::UNPROCESSABLE_ENTITY;
                                Ok(response)
                            }
                            Err(AuthError::TwoFactorNotEnabled) => Ok(Reply::redirect(
                                paths.with_next(TWO_FACTOR_SETUP_PATH, next.as_deref()),
                            )
                            .into_response()),
                            Err(AuthError::TwoFactorAlreadyEnabled) => {
                                Ok(Reply::redirect(paths.after(next.as_deref())).into_response())
                            }
                            Err(error) => Err(error.into_app_error()),
                        }
                    }
                },
            )
            .authenticated()
            .page()
            .guard(not_while_impersonating())
            .name("auth.pages.two_factor_setup.confirm"),
        );

        // POST impersonate/stop
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(STOP_IMPERSONATING_PATH),
                move |session: Session, meta: ClientMeta| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        service
                            .stop_impersonating_session(&session, &meta)
                            .await
                            .map_err(AuthError::into_app_error)?;
                        Ok::<_, AppError>(
                            Reply::redirect(navree_ui::LocalHref::from(paths.home.as_str()))
                                .into_response(),
                        )
                    }
                },
            )
            .authenticated()
            .page()
            .name("auth.pages.impersonation.stop"),
        );
        routes
    }
}
