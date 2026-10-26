//! `ai.text` and `ai.structured` against `AiFake`: answers, request options,
//! conversations, attachments, structured output and its repair loop.

use std::time::Duration;

use garde::Validate;
use navree_ai::{
    AiError, AiFake, AiOptions, Attachment, FakeReply, Finish, Message, ModelConfig, Role,
};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

#[tokio::test]
async fn text_answers_with_who_answered_and_the_usage() {
    let fake = AiFake::new();
    let ai = fake.ai();
    fake.respond(FakeReply::text("Acme is in negotiation.").with_usage(120, 9));

    let response = ai
        .text("Summarize the Acme deal")
        .system("You write two-sentence updates.")
        .model("fast")
        .max_tokens(200)
        .user(Uuid::nil())
        .await
        .unwrap();

    assert_eq!(response.text, "Acme is in negotiation.");
    assert_eq!(response.to_string(), "Acme is in negotiation.");
    assert_eq!(response.model, "fast");
    assert_eq!(response.provider, "fake");
    assert_eq!(response.usage.input_tokens, 120);
    assert_eq!(response.usage.output_tokens, 9);
    assert_eq!(response.finish, Finish::Stop);
    fake.assert_prompt_count(1);
    let request = &fake.requests()[0];
    assert_eq!(request.model(), "fast");
    assert_eq!(request.system(), Some("You write two-sentence updates."));
    assert_eq!(request.max_tokens(), 200);
    assert_eq!(request.user(), Some(Uuid::nil()));
    assert_eq!(request.last_user_message(), "Summarize the Acme deal");
    assert!(request.tools().is_empty());
}

#[tokio::test]
async fn earlier_turns_continue_a_conversation() {
    let fake = AiFake::new();
    fake.respond(FakeReply::text("Acme renewal options"));

    fake.ai()
        .text(vec![
            Message::user("Draft a one-line subject for the Acme renewal email."),
            Message::assistant("Your Acme renewal: two options for next year"),
            Message::user("Shorter, and without the colon."),
        ])
        .await
        .unwrap();

    let messages = fake.requests()[0].messages().to_vec();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1].role, Role::Assistant);
    assert_eq!(fake.requests()[0].last_user_message(), "Shorter, and without the colon.");
}

#[tokio::test]
async fn the_default_model_and_its_max_tokens_apply() {
    let fake = AiFake::new();
    let ai = fake.ai_with(AiOptions {
        default: "smart".into(),
        models: [("smart".to_owned(), ModelConfig::new("anthropic", "claude-opus-5-5").max_tokens(777))].into(),
        ..AiOptions::default()
    });
    fake.respond(FakeReply::text("ok"));

    ai.text("hi").await.unwrap();

    assert_eq!(fake.requests()[0].model(), "smart");
    assert_eq!(fake.requests()[0].max_tokens(), 777);
}

#[tokio::test]
async fn attachments_go_into_the_user_message() {
    let fake = AiFake::new();
    fake.respond(FakeReply::text("A contract."));

    fake.ai()
        .text("What is this?")
        .attach(Attachment::pdf(vec![1u8, 2, 3]))
        .await
        .unwrap();

    let requests = fake.requests();
    let message = &requests[0].messages()[0];
    assert_eq!(message.attachments().count(), 1);
    assert_eq!(message.text(), "What is this?");
}

#[tokio::test]
async fn refusals_and_cut_offs_are_answers_for_text() {
    let fake = AiFake::new();
    fake.respond(FakeReply::refusal());

    let response = fake.ai().text("something").await.unwrap();

    assert_eq!(response.finish, Finish::Refused);
}

#[tokio::test]
async fn a_request_nothing_answers_fails_naming_the_prompt() {
    let fake = AiFake::new();

    let error = fake.ai().text("Is anyone there?").await.unwrap_err();

    assert_eq!(
        error,
        AiError::NoFakeReply {
            prompt: "Is anyone there?".into()
        }
    );
}

#[tokio::test]
async fn respond_when_answers_matching_requests_every_time() {
    let fake = AiFake::new();
    fake.respond_when(|r| r.last_user_message().contains("Globex"), FakeReply::text("Globex!"))
        .respond(FakeReply::text("queued"));
    let ai = fake.ai();

    assert_eq!(ai.text("About Globex").await.unwrap().text, "Globex!");
    assert_eq!(ai.text("About Globex again").await.unwrap().text, "Globex!");
    assert_eq!(ai.text("Something else").await.unwrap().text, "queued");
    fake.assert_prompted(|r| r.last_user_message() == "Something else");
    fake.assert_not_prompted(|r| r.last_user_message() == "Initech");
    fake.assert_prompt_count(3);
}

#[test]
fn assert_nothing_prompted_holds_for_a_new_fake() {
    AiFake::new().assert_nothing_prompted();
}

/// How a customer email reads.
#[derive(Debug, PartialEq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum Sentiment {
    Positive,
    Neutral,
    /// The customer is unhappy, considering leaving, or asking to cancel.
    AtRisk,
}

#[derive(Debug, Deserialize, ToSchema)]
struct Triage {
    sentiment: Sentiment,
    /// One sentence for the account owner.
    reason: String,
}

/// A follow-up email.
#[derive(Debug, Deserialize, ToSchema, Validate)]
struct FollowUpEmail {
    /// The subject line.
    #[garde(length(chars, min = 1, max = 80))]
    subject: String,
    /// The body.
    #[garde(length(chars, min = 1, max = 2000))]
    body: String,
    #[garde(skip)]
    send_on: Option<chrono::NaiveDate>,
}

#[tokio::test]
async fn structured_output_is_a_typed_value_with_the_schema_attached() {
    let fake = AiFake::new();
    fake.respond(FakeReply::json(json!({ "sentiment": "at_risk", "reason": "They asked to cancel." })));

    let triage = fake.ai().structured::<Triage>("I want to cancel.").model("fast").await.unwrap();

    assert_eq!(triage.sentiment, Sentiment::AtRisk);
    assert_eq!(triage.reason, "They asked to cancel.");
    assert!(fake.requests()[0].is_structured());
}

#[tokio::test]
async fn an_enum_alone_is_structured_too() {
    let fake = AiFake::new();
    fake.respond(FakeReply::json(json!({ "value": "neutral" })));

    let sentiment = fake.ai().structured::<Sentiment>("Thanks for the update.").await.unwrap();

    assert_eq!(sentiment, Sentiment::Neutral);
}

#[tokio::test]
async fn answers_that_do_not_deserialize_are_sent_back_for_repair() {
    let fake = AiFake::new();
    fake.respond_with([
        FakeReply::text("Sure! It is at risk."),
        FakeReply::json(json!({ "sentiment": "at_risk", "reason": "Cancelling." })),
    ]);

    let triage = fake.ai().structured::<Triage>("I want to cancel.").await.unwrap();

    assert_eq!(triage.sentiment, Sentiment::AtRisk);
    fake.assert_prompt_count(2);
    let repair = &fake.requests()[1];
    assert_eq!(repair.messages()[1].text(), "Sure! It is at risk.");
    assert!(repair.last_user_message().contains("does not match the required JSON schema"));
}

#[tokio::test]
async fn a_repair_still_carries_the_attachments() {
    let fake = AiFake::new();
    fake.respond_with([
        FakeReply::text("It reads like a contract."),
        FakeReply::json(json!({ "sentiment": "neutral", "reason": "A plain contract." })),
    ]);

    fake.ai()
        .structured::<Triage>("How does this read?")
        .attach(Attachment::pdf(vec![1u8, 2, 3]))
        .await
        .unwrap();

    let requests = fake.requests();
    for request in &requests {
        let attachments: usize = request.messages().iter().map(|m| m.attachments().count()).sum();
        assert_eq!(attachments, 1, "{:?}", request.messages());
    }
    assert_eq!(requests[1].messages()[0].text(), "How does this read?");
    assert_eq!(requests[1].messages()[0].attachments().count(), 1);
}

#[tokio::test]
async fn validated_checks_the_garde_rules_and_repairs() {
    let fake = AiFake::new();
    fake.respond_with([
        FakeReply::json(json!({ "subject": "x".repeat(100), "body": "Hi", "send_on": null })),
        FakeReply::json(json!({ "subject": "Next steps", "body": "Hi Dana", "send_on": "2026-10-30" })),
    ]);

    let email = fake
        .ai()
        .structured::<FollowUpEmail>("Write a follow-up.")
        .validated()
        .await
        .unwrap();

    assert_eq!(email.subject, "Next steps");
    assert_eq!(email.send_on, chrono::NaiveDate::from_ymd_opt(2026, 10, 30));
    let repair = fake.requests()[1].last_user_message();
    assert!(
        repair.contains("subject must be shorter than or equal to 80 characters"),
        "{repair}"
    );
}

#[tokio::test]
async fn without_validated_the_rules_are_not_checked() {
    let fake = AiFake::new();
    fake.respond(FakeReply::json(json!({ "subject": "x".repeat(100), "body": "Hi", "send_on": null })));

    let email = fake.ai().structured::<FollowUpEmail>("Write a follow-up.").await.unwrap();

    assert_eq!(email.subject.len(), 100);
}

#[tokio::test]
async fn repairs_stop_after_structured_retries() {
    let fake = AiFake::new();
    fake.respond_when(|_| true, FakeReply::text("not json"));
    let ai = fake.ai_with(AiOptions {
        structured_retries: 1,
        ..AiOptions::default()
    });

    let error = ai.structured::<Triage>("x").await.unwrap_err();

    assert!(matches!(error, AiError::InvalidOutput { .. }), "{error:?}");
    fake.assert_prompt_count(2);
    assert_eq!(error.status().as_u16(), 502);
}

#[tokio::test]
async fn a_refusal_fails_a_structured_request() {
    let fake = AiFake::new();
    fake.respond(FakeReply::refusal());

    let error = fake.ai().structured::<Triage>("x").model("fast").await.unwrap_err();

    assert_eq!(error, AiError::Refused { model: "fast".into() });
}

#[tokio::test]
async fn timeouts_are_per_request() {
    let fake = AiFake::new();
    fake.respond(FakeReply::text("ok"));

    let response = fake.ai().text("hi").timeout(Duration::from_secs(1)).stop(["END"]).temperature(0.1).await;

    assert!(response.is_ok());
}
