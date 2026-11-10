//! The model vendors' own APIs: one small real request to each. There is no
//! sandbox mode to call, so each test asks a one-word question with a low
//! output limit, which costs a fraction of a cent. Opt in: a test runs only
//! when the keys for its vendor are set, and prints `SKIPPED` and passes
//! otherwise, so a fork or a machine without the accounts is not told it
//! failed. They are `#[ignore]`d too; run them with `-- --ignored sandbox_`
//! (the `sandbox` job of `.github/workflows/integrations.yml` does, with the
//! keys from the repository's secrets).
//!
//! | Vendor | Variables |
//! |---|---|
//! | Anthropic | `NAVREE_SANDBOX_ANTHROPIC_KEY`, `NAVREE_SANDBOX_ANTHROPIC_MODEL` |
//! | OpenAI | `NAVREE_SANDBOX_OPENAI_KEY`, `NAVREE_SANDBOX_OPENAI_MODEL` |
//! | Google Gemini | `NAVREE_SANDBOX_GEMINI_KEY`, `NAVREE_SANDBOX_GEMINI_MODEL` |
//!
//! The model is named by the account's owner, not here: vendors retire
//! models, and a name written in a test would be wrong in a year. What the
//! requests do not show: streaming, tools and structured output, which the
//! conformance suite runs against local servers.

use navree_ai::{Ai, AiOptions, ModelConfig, ProviderConfig};

/// The value of `variable` when it is set and not empty.
fn sandbox(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => {
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

/// Asks `provider` (a table named for its driver) a one-word question and
/// checks the answer and the count of tokens.
async fn answers_a_question(driver: &str, account: ProviderConfig, model: String) {
    let options = AiOptions {
        default: "sandbox".to_owned(),
        providers: [(driver.to_owned(), account)].into(),
        models: [("sandbox".to_owned(), ModelConfig::new(driver, model))].into(),
        ..AiOptions::default()
    };
    let ai = Ai::new(options).expect("the vendor's settings");

    let answer = ai
        .text("Which city is the capital of Punjab, India? Answer with its name only.")
        .max_tokens(200)
        .await
        .expect("the vendor answers");

    assert_eq!(answer.provider, driver);
    assert!(!answer.text.trim().is_empty(), "{answer:?}");
    assert!(
        answer.text.to_lowercase().contains("chandigarh"),
        "{}",
        answer.text
    );
    assert!(answer.usage.input_tokens > 0, "{:?}", answer.usage);
    assert!(answer.usage.output_tokens > 0, "{:?}", answer.usage);
}

#[tokio::test]
#[ignore = "calls the vendors' APIs"]
async fn sandbox_anthropic_answers_a_one_word_question() {
    let test = "sandbox_anthropic_answers_a_one_word_question";
    let (Some(key), Some(model)) = (
        sandbox("NAVREE_SANDBOX_ANTHROPIC_KEY", test),
        sandbox("NAVREE_SANDBOX_ANTHROPIC_MODEL", test),
    ) else {
        return;
    };
    answers_a_question("anthropic", ProviderConfig::anthropic(key), model).await;
}

#[tokio::test]
#[ignore = "calls the vendors' APIs"]
async fn sandbox_openai_answers_a_one_word_question() {
    let test = "sandbox_openai_answers_a_one_word_question";
    let (Some(key), Some(model)) = (
        sandbox("NAVREE_SANDBOX_OPENAI_KEY", test),
        sandbox("NAVREE_SANDBOX_OPENAI_MODEL", test),
    ) else {
        return;
    };
    answers_a_question("openai", ProviderConfig::openai(key), model).await;
}

#[tokio::test]
#[ignore = "calls the vendors' APIs"]
async fn sandbox_gemini_answers_a_one_word_question() {
    let test = "sandbox_gemini_answers_a_one_word_question";
    let (Some(key), Some(model)) = (
        sandbox("NAVREE_SANDBOX_GEMINI_KEY", test),
        sandbox("NAVREE_SANDBOX_GEMINI_MODEL", test),
    ) else {
        return;
    };
    answers_a_question("gemini", ProviderConfig::gemini(key), model).await;
}
