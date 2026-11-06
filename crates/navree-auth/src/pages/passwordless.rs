//! The pages of passwordless sign-in: a link mailed to the address
//! (`{sign-in}/link`), and a code mailed to it (`{sign-in}/code`). They are
//! mounted when `auth.magic_link.enabled` and `auth.otp.enabled` are on.

use std::time::Duration;

use http::Method;
use http::StatusCode;
use http::request::Parts;
use navree_http::extract::Query;
use navree_http::{ApiMeta, AppError, IntoResponse, Response, RouteDef};
use navree_session::Session;
use navree_ui::form::{FormView, Rejected};
use navree_ui::pages::ReturnTo;
use navree_ui::{Alert, AlertTone, Form, Node, Reply, Stack, Submit, Validate};
use serde::Deserialize;

use super::{
    PagesController, Paths, api, field_error, link, page, signed_in_redirect, too_many, wait_text,
};
use crate::{AuthError, ClientMeta, SessionSignIn};

/// How many mails one client may ask for per minute, on these pages.
const MAILS_PER_MINUTE: u32 = 5;

/// The address to mail a sign-in link to.
#[derive(Debug, Deserialize, Validate, Form)]
struct LinkForm {
    /// Email
    #[validate(email, length(max = 255))]
    #[form(widget = email, autofocus, error = "Enter the email address of your account.")]
    email: String,
    #[validate(skip)]
    #[form(widget = hidden)]
    #[serde(default)]
    next: Option<String>,
}

/// The button the mailed link opens: it holds the link's token, so a mail
/// scanner that only fetches the link does not use it up.
#[derive(Debug, Deserialize, Validate, Form)]
struct LinkConfirmForm {
    #[validate(length(min = 1, max = 512))]
    #[form(widget = hidden)]
    token: String,
    /// Keep me signed in on this device
    #[validate(skip)]
    #[serde(default)]
    remember: bool,
    #[validate(skip)]
    #[form(widget = hidden)]
    #[serde(default)]
    next: Option<String>,
}

/// The address to mail a sign-in code to.
#[derive(Debug, Deserialize, Validate, Form)]
struct CodeRequestForm {
    /// Email
    #[validate(email, length(max = 255))]
    #[form(widget = email, autofocus, error = "Enter the email address of your account.")]
    email: String,
    #[validate(skip)]
    #[form(widget = hidden)]
    #[serde(default)]
    next: Option<String>,
}

/// The code from the mail.
#[derive(Debug, Deserialize, Validate, Form)]
struct CodeForm {
    #[validate(email, length(max = 255))]
    #[form(widget = hidden)]
    email: String,
    /// Code from the email
    #[validate(length(min = 1, max = 16))]
    #[form(autofocus, error = "Enter the code from the email.")]
    code: String,
    /// Keep me signed in on this device
    #[validate(skip)]
    #[serde(default)]
    remember: bool,
    #[validate(skip)]
    #[form(widget = hidden)]
    #[serde(default)]
    next: Option<String>,
}

/// `?token=&next=` of the mailed link.
#[derive(Debug, Default, Deserialize)]
struct LinkQuery {
    #[serde(default)]
    token: Option<String>,
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
    next.and_then(|next| ReturnTo::parse_for(next, &paths.sign_in))
        .map(|next| next.as_str().to_owned())
}

fn link_request_page(
    paths: &Paths,
    form: FormView<LinkForm>,
    sent: bool,
    refused: Option<String>,
) -> navree_ui::View {
    let sent = sent.then(|| {
        Alert::new(AlertTone::Success)
            .title("If an account uses that address, we have sent it a link to sign in with.")
    });
    let refused = refused.map(|message| Alert::new(AlertTone::Danger).title(message));
    page(
        paths,
        "Email me a sign-in link",
        "We will email you a link that signs you in, without a password.",
        Stack::gap(4)
            .children((
                sent,
                refused,
                Node::from(form.submit("Send the link")),
                link("Sign in with a password", &paths.sign_in),
            ))
            .into(),
    )
}

fn link_confirm_page(
    paths: &Paths,
    form: FormView<LinkConfirmForm>,
    expired: bool,
    refused: Option<String>,
) -> navree_ui::View {
    let expired = expired.then(|| {
        Alert::new(AlertTone::Danger)
            .title("This link has expired or was used already.")
            .body(navree_ui::Text::new(
                "Ask for a new one and use it within a few minutes.",
            ))
    });
    let refused = refused.map(|message| Alert::new(AlertTone::Danger).title(message));
    page(
        paths,
        "Continue signing in",
        "Press the button to finish signing in with the link from your email.",
        Stack::gap(4)
            .children((
                expired,
                refused,
                Node::from(form.submit("Sign in")),
                link("Ask for a new link", &paths.magic_link),
            ))
            .into(),
    )
}

fn code_request_page(
    paths: &Paths,
    form: FormView<CodeRequestForm>,
    refused: Option<String>,
) -> navree_ui::View {
    let refused = refused.map(|message| Alert::new(AlertTone::Danger).title(message));
    page(
        paths,
        "Email me a sign-in code",
        "We will email you a short code that signs you in, without a password.",
        Stack::gap(4)
            .children((
                refused,
                Node::from(form.submit("Send the code")),
                link("Sign in with a password", &paths.sign_in),
            ))
            .into(),
    )
}

fn code_page(paths: &Paths, form: FormView<CodeForm>, sent: bool) -> navree_ui::View {
    let sent = sent.then(|| {
        Alert::new(AlertTone::Success).title(
            "If an account uses that address, we have sent it a code. It works for a few minutes.",
        )
    });
    page(
        paths,
        "Enter the code",
        "Type the code from the email we sent you.",
        Stack::gap(4)
            .children((
                sent,
                Node::from(form.submit("Sign in")),
                link("Ask for a new code", &paths.code),
            ))
            .into(),
    )
}

/// The empty form that asks for a code, with `email` already typed.
fn code_form(
    paths: &Paths,
    token: String,
    email: &str,
    next: Option<String>,
) -> FormView<CodeForm> {
    FormView::<CodeForm>::new()
        .post(format!("{}/verify", paths.code))
        .csrf(token)
        .of(&CodeForm {
            email: email.to_owned(),
            code: String::new(),
            remember: false,
            next,
        })
}

impl PagesController {
    /// The routes of the mailed link, when `auth.magic_link.enabled`.
    pub(super) fn link_routes(&self) -> Vec<RouteDef> {
        let paths = self.paths.clone();
        let absolute = |path: &str| format!("!{path}");
        let verify_path = format!("{}/verify", paths.magic_link);
        let mut routes = Vec::new();

        // GET link
        let at = paths.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute(&paths.magic_link),
                move |session: Session, Query(query): Query<NextQuery>| {
                    let paths = at.clone();
                    async move {
                        let form = FormView::<LinkForm>::new()
                            .post(paths.magic_link.as_str())
                            .csrf(session.csrf_token())
                            .of(&LinkForm {
                                email: String::new(),
                                next: safe_next(&paths, query.next.as_deref()),
                            });
                        link_request_page(&paths, form, false, None).into_response()
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.magic_link")
            .api(api("The page that mails a sign-in link", &[])),
        );

        // POST link
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&paths.magic_link),
                move |session: Session,
                      meta: ClientMeta,
                      submitted: Result<Submit<LinkForm>, Rejected<LinkForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => {
                                return Ok(rejected.respond(move |form| {
                                    link_request_page(&paths, form.csrf(token), false, None)
                                }));
                            }
                        };
                        let form = || {
                            FormView::<LinkForm>::new()
                                .post(paths.magic_link.as_str())
                                .csrf(token.clone())
                                .of(&LinkForm {
                                    email: String::new(),
                                    next: safe_next(&paths, input.next.as_deref()),
                                })
                        };
                        match service.send_magic_link(&input.email, &meta).await {
                            // The same answer for every address.
                            Ok(()) => Ok::<_, AppError>(
                                link_request_page(&paths, form(), true, None).into_response(),
                            ),
                            Err(AuthError::SignInRefused { message, .. }) => {
                                let mut response =
                                    link_request_page(&paths, form(), false, Some(message))
                                        .into_response();
                                *response.status_mut() = StatusCode::FORBIDDEN;
                                Ok(response)
                            }
                            Err(error) => Err(error.into_app_error()),
                        }
                    }
                },
            )
            .public()
            .page()
            .throttle(MAILS_PER_MINUTE, Duration::from_secs(60))
            .name("auth.pages.magic_link.submit")
            .api(ApiMeta {
                description: Some(
                    "The same answer for every address; a sign-in policy that refuses the account \
                     answers 403."
                        .to_owned(),
                ),
                ..api("Mails a sign-in link", &[403, 422, 429])
            }),
        );

        // GET link/verify: the page the mailed link opens
        let at = paths.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute(&verify_path),
                move |session: Session, Query(query): Query<LinkQuery>| {
                    let paths = at.clone();
                    async move {
                        let form = FormView::<LinkConfirmForm>::new()
                            .post(format!("{}/verify", paths.magic_link))
                            .csrf(session.csrf_token())
                            .of(&LinkConfirmForm {
                                token: query.token.unwrap_or_default(),
                                remember: false,
                                next: safe_next(&paths, query.next.as_deref()),
                            });
                        link_confirm_page(&paths, form, false, None).into_response()
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.magic_link.verify")
            .api(ApiMeta {
                description: Some("One button that signs in with the link's `token`.".to_owned()),
                ..api("The page a mailed sign-in link opens", &[])
            }),
        );

        // POST link/verify
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&verify_path),
                move |parts: Parts,
                      session: Session,
                      meta: ClientMeta,
                      submitted: Result<Submit<LinkConfirmForm>, Rejected<LinkConfirmForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let redraw = |rejected: Rejected<LinkConfirmForm>,
                                      expired: bool,
                                      refused: Option<String>| {
                            let paths = paths.clone();
                            let token = token.clone();
                            rejected.respond(move |form| {
                                link_confirm_page(&paths, form.csrf(token), expired, refused)
                            })
                        };
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => return Ok(redraw(rejected, false, None)),
                        };
                        let outcome = service
                            .login_with_magic_link_session(
                                &session,
                                &input.token,
                                input.remember,
                                &meta,
                            )
                            .await;
                        match outcome {
                            Ok(SessionSignIn::SignedIn(_)) => Ok(signed_in_redirect(
                                &service,
                                &session,
                                &paths,
                                input.next.as_deref(),
                            )),
                            Ok(SessionSignIn::TwoFactorRequired) => Ok(Reply::redirect(
                                paths.with_next(&paths.two_factor, input.next.as_deref()),
                            )
                            .into_response()),
                            Err(AuthError::InvalidLoginToken) => {
                                let mut response = redraw(
                                    Rejected::after_decode(
                                        &input,
                                        navree_ui::form::FieldErrors::new(),
                                        &parts,
                                    ),
                                    true,
                                    None,
                                );
                                *response.status_mut() = StatusCode::BAD_REQUEST;
                                Ok(response)
                            }
                            Err(AuthError::SignInRefused { message, .. }) => {
                                let mut response = redraw(
                                    Rejected::after_decode(
                                        &input,
                                        navree_ui::form::FieldErrors::new(),
                                        &parts,
                                    ),
                                    false,
                                    Some(message),
                                );
                                *response.status_mut() = StatusCode::FORBIDDEN;
                                Ok(response)
                            }
                            Err(error) => Err::<Response, AppError>(error.into_app_error()),
                        }
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.magic_link.verify.submit")
            .api(ApiMeta {
                description: Some(
                    "Goes on to `next`, or to the second factor when the account uses it (303). A \
                     link that is used or expired answers 400."
                        .to_owned(),
                ),
                ..api("Signs in with a mailed link", &[303, 400, 403, 422])
            }),
        );
        routes
    }

    /// The routes of the emailed code, when `auth.otp.enabled`.
    pub(super) fn code_routes(&self) -> Vec<RouteDef> {
        let paths = self.paths.clone();
        let absolute = |path: &str| format!("!{path}");
        let verify_path = format!("{}/verify", paths.code);
        let mut routes = Vec::new();

        // GET code
        let at = paths.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute(&paths.code),
                move |session: Session, Query(query): Query<NextQuery>| {
                    let paths = at.clone();
                    async move {
                        let form = FormView::<CodeRequestForm>::new()
                            .post(paths.code.as_str())
                            .csrf(session.csrf_token())
                            .of(&CodeRequestForm {
                                email: String::new(),
                                next: safe_next(&paths, query.next.as_deref()),
                            });
                        code_request_page(&paths, form, None).into_response()
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.code")
            .api(api("The page that mails a sign-in code", &[])),
        );

        // POST code: mails the code, then asks for it
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&paths.code),
                move |session: Session,
                      meta: ClientMeta,
                      submitted: Result<Submit<CodeRequestForm>, Rejected<CodeRequestForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => {
                                return Ok(rejected.respond(move |form| {
                                    code_request_page(&paths, form.csrf(token), None)
                                }));
                            }
                        };
                        let next = safe_next(&paths, input.next.as_deref());
                        match service.send_login_code(&input.email, &meta).await {
                            // The same answer for every address.
                            Ok(()) => Ok::<_, AppError>(
                                code_page(
                                    &paths,
                                    code_form(&paths, token, &input.email, next),
                                    true,
                                )
                                .into_response(),
                            ),
                            Err(AuthError::SignInRefused { message, .. }) => {
                                let form = FormView::<CodeRequestForm>::new()
                                    .post(paths.code.as_str())
                                    .csrf(token)
                                    .of(&CodeRequestForm {
                                        email: input.email.clone(),
                                        next,
                                    });
                                let mut response =
                                    code_request_page(&paths, form, Some(message)).into_response();
                                *response.status_mut() = StatusCode::FORBIDDEN;
                                Ok(response)
                            }
                            Err(error) => Err(error.into_app_error()),
                        }
                    }
                },
            )
            .public()
            .page()
            .throttle(MAILS_PER_MINUTE, Duration::from_secs(60))
            .name("auth.pages.code.submit")
            .api(ApiMeta {
                description: Some(
                    "Then asks for the code. The same answer for every address; a sign-in policy \
                     that refuses the account answers 403."
                        .to_owned(),
                ),
                ..api("Mails a sign-in code", &[403, 422, 429])
            }),
        );

        // POST code/verify
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&verify_path),
                move |parts: Parts,
                      session: Session,
                      meta: ClientMeta,
                      submitted: Result<Submit<CodeForm>, Rejected<CodeForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let redraw = |rejected: Rejected<CodeForm>| {
                            let paths = paths.clone();
                            let token = token.clone();
                            rejected.respond(move |form| code_page(&paths, form.csrf(token), false))
                        };
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => return Ok(redraw(rejected)),
                        };
                        let outcome = service
                            .login_with_code_session(
                                &session,
                                &input.email,
                                &input.code,
                                input.remember,
                                &meta,
                            )
                            .await;
                        match outcome {
                            Ok(SessionSignIn::SignedIn(_)) => Ok(signed_in_redirect(
                                &service,
                                &session,
                                &paths,
                                input.next.as_deref(),
                            )),
                            Ok(SessionSignIn::TwoFactorRequired) => Ok(Reply::redirect(
                                paths.with_next(&paths.two_factor, input.next.as_deref()),
                            )
                            .into_response()),
                            Err(AuthError::InvalidLoginToken) => {
                                Ok(redraw(Rejected::after_decode(
                                    &input,
                                    field_error(
                                        "code",
                                        "That code didn't work. Check it, or ask for a new one.",
                                    ),
                                    &parts,
                                )))
                            }
                            Err(AuthError::TooManyAttempts { retry_after }) => Ok(too_many(
                                redraw(Rejected::after_decode(
                                    &input,
                                    field_error("code", wait_text(retry_after)),
                                    &parts,
                                )),
                                retry_after,
                            )),
                            Err(AuthError::SignInRefused { message, .. }) => {
                                let mut response = redraw(Rejected::after_decode(
                                    &input,
                                    field_error("code", message),
                                    &parts,
                                ));
                                *response.status_mut() = StatusCode::FORBIDDEN;
                                Ok(response)
                            }
                            Err(error) => Err::<Response, AppError>(error.into_app_error()),
                        }
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.code.verify")
            .api(ApiMeta {
                description: Some(
                    "Goes on to `next`, or to the second factor when the account uses it (303). A \
                     code that didn't work draws the form again (422)."
                        .to_owned(),
                ),
                ..api("Signs in with a mailed code", &[303, 403, 422, 429])
            }),
        );
        routes
    }
}
