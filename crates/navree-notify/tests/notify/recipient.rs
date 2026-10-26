//! A `dyn RecipientScope` runs the rendering of a notification, and the
//! rendering of its mail at delivery, in the recipient's context (what
//! navree-i18n uses to render in the recipient's locale).

use std::sync::Arc;

use navree_contracts::{BoxFuture, Dispatcher};
use navree_kernel::{Module, ModuleBuilder};
use navree_mail::TemplateMail;
use navree_notify::{
    Channel, Notifiable, Notification, Notifier, NotifyOptions, RecipientScope,
};
use navree_view::ModuleBuilderViewExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::support::{self, RecordingDispatcher, TestUser};

tokio::task_local! {
    static RECIPIENT: String;
}

/// The recipient the current task works for, or `none`.
fn recipient() -> String {
    RECIPIENT
        .try_with(Clone::clone)
        .unwrap_or_else(|_| "none".to_owned())
}

/// Runs the work with `RECIPIENT` set to `type:id`.
struct TagRecipient;

impl RecipientScope for TagRecipient {
    fn scope<'a>(
        &'a self,
        notifiable_type: &'a str,
        notifiable_id: &'a str,
        work: BoxFuture<'a, ()>,
    ) -> BoxFuture<'a, ()> {
        Box::pin(RECIPIENT.scope(format!("{notifiable_type}:{notifiable_id}"), work))
    }
}

/// A notification that records whom it was rendered for.
#[derive(Debug, Serialize, Deserialize)]
struct Greeting;

impl Notification for Greeting {
    const TYPE: &'static str = "greeting";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Mail]
    }

    fn to_mail(&self, _notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        Some(TemplateMail {
            subject: format!("for {}", recipient()),
            template: "mail/greeting".to_owned(),
            context: json!({}),
            ..TemplateMail::default()
        })
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        Some(json!({ "for": recipient() }))
    }
}

/// The greeting's template, which prints the recipient the mail is
/// rendered for.
struct GreetingTemplates;

impl Module for GreetingTemplates {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("mail/greeting.txt", "body for {{ recipient() }}")])
            .view_environment(|env| env.add_function("recipient", recipient));
    }
}

#[tokio::test]
async fn rendering_and_mail_run_in_the_recipient_scope() {
    let app = support::builder(NotifyOptions::default())
        .override_value::<dyn RecipientScope>(Arc::new(TagRecipient))
        .module(GreetingTemplates)
        .build()
        .await;
    let user = TestUser::new();
    let tag = format!("user:{}", user.id);

    app.get::<Notifier>()
        .await
        .send(&user, Greeting)
        .await
        .unwrap();

    let rows = support::rows(&app).await;
    assert_eq!(rows[0].data, json!({ "for": tag }));
    let mail = support::sent_mail(&app).await;
    assert_eq!(mail[0].subject, format!("for {tag}"));
    assert_eq!(mail[0].text.as_deref(), Some(format!("body for {tag}").as_str()));
}

#[tokio::test]
async fn queued_mail_renders_in_the_recipient_scope_when_delivered() {
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let app = support::builder(NotifyOptions {
        queue: true,
        ..NotifyOptions::default()
    })
    .override_value::<dyn Dispatcher>(dispatcher.clone())
    .override_value::<dyn RecipientScope>(Arc::new(TagRecipient))
    .module(GreetingTemplates)
    .build()
    .await;
    let user = TestUser::new();

    app.get::<Notifier>()
        .await
        .send(&user, Greeting)
        .await
        .unwrap();
    // A worker runs the jobs later, outside any scope.
    for job in dispatcher.jobs() {
        support::run_job(&app, &job).await.unwrap();
    }

    let mail = support::sent_mail(&app).await;
    assert_eq!(
        mail[0].text.as_deref(),
        Some(format!("body for user:{}", user.id).as_str())
    );
}

#[tokio::test]
async fn without_a_scope_rendering_runs_as_before() {
    let app = support::builder(NotifyOptions::default())
        .module(GreetingTemplates)
        .build()
        .await;

    app.get::<Notifier>()
        .await
        .send(&TestUser::new(), Greeting)
        .await
        .unwrap();

    assert_eq!(support::rows(&app).await[0].data, json!({ "for": "none" }));
    assert_eq!(
        support::sent_mail(&app).await[0].text.as_deref(),
        Some("body for none")
    );
}
