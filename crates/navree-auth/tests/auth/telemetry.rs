//! Sign-ins reported as telemetry: the method, the outcome and a reason, with
//! the user only when the sign-in worked and never the email address.

use navree_auth::{AuthService, ClientMeta, SignInMethod};
use navree_telemetry::{
    Outcome, SignInMethod as TelemetrySignInMethod, Telemetry, TelemetryContext, TelemetryRecorder,
};

use crate::support::{self, PASSWORD, login};

#[tokio::test]
async fn password_sign_ins_are_reported_without_the_email() {
    let recorder = TelemetryRecorder::new();
    let app = support::builder().module(recorder.module()).build().await;
    let user = support::create_user(&app, "anu@example.com").await;
    let client = app.client();

    login(&client, "anu@example.com", "wrong password")
        .await
        .assert_status(401);
    login(&client, "anu@example.com", PASSWORD)
        .await
        .assert_status(200);

    let events = recorder.sign_ins();
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0].method, TelemetrySignInMethod::Password);
    assert_eq!(events[0].outcome, Outcome::Failure);
    assert_eq!(events[0].reason.as_deref(), Some("invalid_credentials"));
    assert_eq!(events[0].user_id, None, "a refusal names nobody");
    assert_eq!(events[1].method, TelemetrySignInMethod::Password);
    assert_eq!(events[1].outcome, Outcome::Success);
    assert_eq!(events[1].user_id, Some(user.id.to_string()));
    let text = serde_json::to_string(&events).unwrap();
    assert!(!text.contains("anu@example.com"), "{text}");
    assert!(!text.contains(PASSWORD), "{text}");
}

#[tokio::test]
async fn a_sign_in_that_a_social_flow_ends_with_names_its_provider_and_user() {
    let recorder = TelemetryRecorder::new();
    let app = support::builder().module(recorder.module()).build().await;
    let user = support::create_user(&app, "anu@example.com").await;
    let auth = app.get::<AuthService>().await;
    let telemetry = Telemetry::from_app(app.ctx()).await;

    TelemetryContext::new(telemetry)
        .scope(async {
            auth.sign_in(
                &user,
                SignInMethod::social("github"),
                &ClientMeta::default(),
            )
            .await
            .unwrap();
        })
        .await;

    let events = recorder.sign_ins();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].method, TelemetrySignInMethod::Social);
    assert_eq!(events[0].provider.as_deref(), Some("github"));
    assert_eq!(events[0].user_id, Some(user.id.to_string()));
    assert_eq!(events[0].outcome, Outcome::Success);
}
