//! The pages a signed-in browser reaches: setting up two-factor
//! authentication (`/account/two-factor`) and going back to one's own
//! sign-in after impersonating a user (`POST /impersonate/stop`).
//!
//! A user whom a two-factor policy requires to set it up reaches nothing
//! else: after signing in, the pages send them to the setup page.

use http::Method;
use navree_http::extract::Query;
use navree_http::{ApiMeta, AppError, CurrentPrincipal, IntoResponse, RouteDef};
use navree_session::Session;
use navree_ui::form::{FormView, Rejected, csrf_field};
use navree_ui::html::SafeHtml;
use navree_ui::{Alert, AlertTone, Button, Form, Node, Reply, Stack, Submit, Text, Validate, el};
use serde::Deserialize;

use super::{
    Label, Page, PagesController, Paths, api, finishes_hand_over, link, onward, page, plain_link,
};
use crate::session_auth::{
    SIGN_OUT_PATH, STOP_IMPERSONATING_PATH, TWO_FACTOR_CONFIRM_PATH, TWO_FACTOR_SETUP_PATH,
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

/// The Sign out button of the set-up page, when the app asks for it
/// ([`AuthPages::setup_sign_out`](super::AuthPages::setup_sign_out)): a form that posts to
/// the sign-out route with the session's `token`, as the shell's user menu does. A user who
/// must set two-factor authentication up reaches that route and the set-up routes only.
fn sign_out(paths: &Paths, token: &str) -> Option<Node> {
    let dressing = &paths.dressing;
    dressing.setup_sign_out.then(|| {
        let button = Button::new("Sign out").submit();
        let button = match dressing.links {
            Some(variant) => button.variant(variant).block(),
            None => button,
        };
        el("form")
            .attr("method", "post")
            .attr("action", SIGN_OUT_PATH)
            .flag("data-ui-form", true)
            .children((csrf_field(token.to_owned()), button))
            .into()
    })
}

/// The page that says where two-factor authentication stands. `token` is the session's,
/// for the Sign out button.
fn status_page(
    paths: &Paths,
    enabled: bool,
    form: FormView<SetupStartForm>,
    token: &str,
) -> navree_ui::View {
    if enabled {
        return page(
            paths,
            Page::TwoFactorOn,
            Stack::gap(4)
                .children((
                    Alert::new(AlertTone::Success).title("Two-factor authentication is on."),
                    link(paths, "Continue", &paths.home),
                ))
                .into(),
        );
    }
    page(
        paths,
        Page::TwoFactorSetup,
        Stack::gap(4)
            .children((
                Node::from(form.one_column().submit("Set it up")),
                link(paths, "Not now", &paths.home),
                sign_out(paths, token),
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

/// The page with the secret to scan or type, and the form for the first code. `token` is
/// the session's, for the Sign out button.
fn setup_page(
    paths: &Paths,
    setup: &TwoFactorSetup,
    form: FormView<SetupConfirmForm>,
    mistake: bool,
    token: &str,
) -> navree_ui::View {
    let mistake = mistake.then(|| {
        Alert::new(AlertTone::Danger)
            .title("That code didn't work.")
            .body(Text::new(
                "We made a new key. Scan it again, then type the code your app shows now.",
            ))
    });
    let form = paths.labelled(form, &[("code", Label::AuthenticatorCode)]);
    page(
        paths,
        Page::TwoFactorSecret,
        Stack::gap(4)
            .children((
                mistake,
                el("div")
                    .attr("role", "img")
                    .attr("aria-label", "QR code for your authenticator app")
                    .child(SafeHtml::trusted(setup.qr_svg.clone())),
                Text::muted("Or type this key into the app:"),
                Text::code(setup.secret.clone()),
                Node::from(form.one_column().submit("Turn it on")),
                sign_out(paths, token),
            ))
            .into(),
    )
}

/// The recovery codes, shown once. The way on is a link the runtime follows
/// in place, unless it leads to the finish of an app's sign-in hand-over,
/// which only the browser itself opens.
fn codes_page(paths: &Paths, codes: &[String], next: &str) -> navree_ui::View {
    const CONTINUE: &str = "I have saved them, continue";
    let on = if finishes_hand_over(next) {
        plain_link(paths, CONTINUE, next, None)
    } else {
        link(paths, CONTINUE, next)
    };
    page(
        paths,
        Page::RecoveryCodes,
        Stack::gap(4)
            .children((
                Alert::new(AlertTone::Success).title("Two-factor authentication is on."),
                el("ul").children(
                    codes
                        .iter()
                        .map(|code| el("li").child(Text::code(code.clone())))
                        .collect::<Vec<_>>(),
                ),
                on,
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
                        let token = session.csrf_token();
                        let form = FormView::<SetupStartForm>::new()
                            .post(TWO_FACTOR_SETUP_PATH)
                            .csrf(token.clone())
                            .of(&SetupStartForm {
                                next: safe_next(&paths, query.next.as_deref()),
                            });
                        Ok::<_, AppError>(
                            status_page(&paths, enabled, form, &token).into_response(),
                        )
                    }
                },
            )
            .authenticated()
            .page()
            .guard(not_while_impersonating())
            .name("auth.pages.two_factor_setup")
            .api(ApiMeta {
                description: Some(
                    "Whether two-factor authentication is on, with a button to set it up. Not \
                     while impersonating (403)."
                        .to_owned(),
                ),
                ..api("The two-factor setup page", &[403])
            }),
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
                                    status_page(&paths, false, form.csrf(token.clone()), &token)
                                }));
                            }
                        };
                        let next = safe_next(&paths, input.next.as_deref());
                        match service.enable_two_factor(me.user_id, &meta).await {
                            Ok(setup) => {
                                let form = FormView::<SetupConfirmForm>::new()
                                    .post(TWO_FACTOR_CONFIRM_PATH)
                                    .csrf(token.clone())
                                    .of(&SetupConfirmForm {
                                        code: String::new(),
                                        next,
                                    });
                                Ok::<_, AppError>(
                                    setup_page(&paths, &setup, form, false, &token)
                                        .into_response(),
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
            .name("auth.pages.two_factor_setup.start")
            .api(ApiMeta {
                description: Some(
                    "Draws a new secret as a picture and a key, with the form for the first code."
                        .to_owned(),
                ),
                ..api("Starts setting two-factor authentication up", &[303, 403, 422])
            }),
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
                                    setup_page(&paths, &setup, form.csrf(token.clone()), false, &token)
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
                                    .csrf(token.clone())
                                    .of(&SetupConfirmForm {
                                        code: String::new(),
                                        next,
                                    });
                                let mut response =
                                    setup_page(&paths, &setup, form, true, &token).into_response();
                                *response.status_mut() = http::StatusCode::UNPROCESSABLE_ENTITY;
                                Ok(response)
                            }
                            Err(AuthError::TwoFactorNotEnabled) => Ok(Reply::redirect(
                                paths.with_next(TWO_FACTOR_SETUP_PATH, next.as_deref()),
                            )
                            .into_response()),
                            Err(AuthError::TwoFactorAlreadyEnabled) => {
                                Ok(onward(paths.after(next.as_deref())).into_response())
                            }
                            Err(error) => Err(error.into_app_error()),
                        }
                    }
                },
            )
            .authenticated()
            .page()
            .guard(not_while_impersonating())
            .name("auth.pages.two_factor_setup.confirm")
            .api(ApiMeta {
                description: Some(
                    "Shows the recovery codes once. A wrong code starts the setup again with a \
                     new key (422)."
                        .to_owned(),
                ),
                ..api("Turns two-factor authentication on with the first code", &[303, 403, 422])
            }),
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
            .name("auth.pages.impersonation.stop")
            .api(ApiMeta {
                description: Some(
                    "The administrator is signed in as themselves again, then goes to the home \
                     page (303). A browser that is not impersonating answers 400."
                        .to_owned(),
                ),
                ..api("Stops impersonating a user", &[303, 400, 403])
            }),
        );
        routes
    }
}
