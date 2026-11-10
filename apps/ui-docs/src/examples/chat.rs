//! The parts of a conversation with an assistant: Conversation, ChatMessage, PromptInput,
//! ToolCallCard and Reasoning, ApprovalCard, SurfaceFrame, Suggestions, Sources and
//! Citation, and ThreadList.

use navree::ui::genui::{
    ApprovalCard, ApprovalState, ChatLabels, ChatMessage, ChatState, Citation, Conversation,
    PromptInput, Reasoning, Sources, Suggestions, SurfaceFrame, ThreadList, ToolCallCard,
    ToolState, approval_notice, chat_notice,
};
use navree::ui::*;
use serde_json::json;

use crate::catalog::{Example, single};

const SOURCE: &str = include_str!("chat.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!(
            "conversation",
            "default",
            "A conversation",
            conversation_default
        )
        .start()
        .width(620)
        .height(420),
        single!(
            "conversation",
            "empty",
            "Before the first message",
            conversation_empty
        )
        .start()
        .width(620)
        .height(150),
        single!(
            "chat-message",
            "default",
            "A question and an answer",
            message_default
        )
        .start()
        .width(620)
        .height(260),
        single!(
            "chat-message",
            "states",
            "Being written, stopped and failed",
            message_states
        )
        .start()
        .width(620)
        .height(420),
        single!(
            "chat-message",
            "parts",
            "With its reasoning and sources",
            message_parts
        )
        .start()
        .width(620)
        .height(330),
        single!("prompt-input", "default", "The composer", prompt_default)
            .start()
            .width(620)
            .height(240),
        single!(
            "prompt-input",
            "error",
            "A message the server refused",
            prompt_error
        )
        .start()
        .width(620)
        .height(280),
        single!(
            "tool-call-card",
            "default",
            "A tool running, done and failed",
            tool_default
        )
        .start()
        .width(620)
        .height(330),
        single!(
            "tool-call-card",
            "reasoning",
            "Reasoning, folded",
            tool_reasoning
        )
        .start()
        .width(620)
        .height(190),
        single!(
            "approval-card",
            "default",
            "A call waiting for the reader",
            approval_default
        )
        .start()
        .width(620)
        .height(420),
        single!(
            "surface-frame",
            "default",
            "A component the assistant chose",
            frame_default
        )
        .start()
        .width(620)
        .height(260),
        single!(
            "surface-frame",
            "gone",
            "A component the reader can no longer see, and one that failed",
            frame_gone
        )
        .start()
        .width(620)
        .height(210),
        single!(
            "suggestions",
            "default",
            "Messages to send with one press",
            suggestions_default
        )
        .start()
        .width(620)
        .height(110),
        single!(
            "sources",
            "default",
            "The sources of an answer",
            sources_default
        )
        .start()
        .width(620)
        .height(190),
        single!(
            "sources",
            "citations",
            "Numbered references in an answer",
            sources_citations
        )
        .start()
        .width(620)
        .height(240),
        single!(
            "thread-list",
            "default",
            "The reader's conversations",
            threads_default
        )
        .start()
        .width(420)
        .height(190),
    ]
}

fn conversation_default() -> Conversation {
    let labels = ChatLabels::default();
    Conversation::new("kitchen-help", "/assistant/7/items").items([
        ChatMessage::user("m1", "0001", "How did UPI payments do this week?").into_item(&labels),
        ToolCallCard::new("t1", "0002", "Looked up payments by method")
            .state(ToolState::Done)
            .arguments(json!({ "from": "2026-11-02", "to": "2026-11-08" }))
            .into_item(&labels),
        ChatMessage::assistant(
            "m2",
            "0003",
            "UPI took **62%** of the week's ₹4,86,250, ahead of cards at 21%.",
        )
        .into_item(&labels),
        chat_notice("n1", "0004", "You refunded order KK-1042"),
        approval_notice(
            "n2",
            "0005",
            "Refund an order",
            &ApprovalState::Waiting,
            &labels,
        ),
    ])
}

fn conversation_empty() -> Conversation {
    Conversation::new("kitchen-help", "/assistant/9/items")
        .empty(Text::muted("Ask about your orders, payments or refunds."))
}

fn message_default() -> Conversation {
    let labels = ChatLabels::default();
    Conversation::new("kitchen-help", "/assistant/7/items").items([
        ChatMessage::user("m1", "0001", "Which city ordered the most this month?")
            .into_item(&labels),
        ChatMessage::assistant(
            "m2",
            "0002",
            "**Ludhiana** ordered the most: 1,240 orders worth ₹18,40,000.\n\n- Jalandhar: 860 orders\n- Amritsar: 702 orders",
        )
        .author("Assistant")
        .into_item(&labels),
    ])
}

fn message_states() -> Conversation {
    let labels = ChatLabels::default();
    Conversation::new("states", "/assistant/8/items").items([
        ChatMessage::assistant("a1", "0001", "UPI payments grew by 12% in Amritsar and")
            .state(ChatState::Writing)
            .stop("/assistant/8/stop", None)
            .into_item(&labels),
        ChatMessage::assistant("a2", "0002", "Refunds are listed by order number, and")
            .state(ChatState::Stopped)
            .into_item(&labels),
        ChatMessage::assistant("a3", "0003", "The report for")
            .state(ChatState::Failed)
            .into_item(&labels),
    ])
}

fn message_parts() -> Conversation {
    let labels = ChatLabels::default();
    Conversation::new("kitchen-help", "/assistant/7/items").items([ChatMessage::assistant(
        "m1",
        "0001",
        "The GST on October sales was **₹82,350**.",
    )
    .with((
        Reasoning::new("I added the CGST and SGST columns of the October invoices."),
        Sources::new()
            .source("GST summary for October", "/reports/gst/2026-10")
            .source("Income Tax Department", "https://www.incometax.gov.in/"),
    ))
    .into_item(&labels)])
}

fn prompt_default() -> PromptInput {
    PromptInput::new("assistant-composer", "/assistant/7/messages")
}

fn prompt_error() -> PromptInput {
    PromptInput::new("assistant-composer", "/assistant/7/messages")
        .value("Refund order KK-1042 and every order from Ludhiana after")
        .error("Messages can be up to 4,000 characters.")
}

fn tool_default() -> Conversation {
    let labels = ChatLabels::default();
    Conversation::new("tools", "/assistant/7/items").items([
        ToolCallCard::new("t1", "0001", "Looking up orders").into_item(&labels),
        ToolCallCard::new("t2", "0002", "Looked up payments by method")
            .state(ToolState::Done)
            .arguments(json!({ "from": "2026-11-02", "to": "2026-11-08", "city": "Ludhiana" }))
            .into_item(&labels),
        ToolCallCard::new("t3", "0003", "Looked up the courier's status")
            .state(ToolState::Failed)
            .arguments(json!({ "awb": "KK1042884" }))
            .into_item(&labels),
    ])
}

fn tool_reasoning() -> Stack {
    Stack::gap(3).children((
        Reasoning::new(
            "The question is about **last week**, so I need the orders from Monday to Sunday and their payment methods.",
        ),
        Reasoning::new("The reader asked for Ludhiana only.").open(true),
    ))
}

fn approval_default() -> ApprovalCard {
    ApprovalCard::new(
        "call_1042",
        "Refund an order",
        json!({ "order": "KK-1042", "amount": "₹1,250", "reason": "Wrong item delivered" }),
    )
    .routes("/assistant/7/approve", "/assistant/7/reject")
}

fn frame_default() -> Conversation {
    let body = Stack::gap(3).children((
        Progress::new(62, 100, "UPI").show_value(),
        Progress::new(21, 100, "Cards").show_value(),
        Progress::new(17, 100, "Cash on delivery").show_value(),
    ));
    Conversation::new("frames", "/assistant/7/items").items([SurfaceFrame::new(
        "s1",
        "Payments by method",
        body.into(),
    )
    .into_item("0001")])
}

fn frame_gone() -> Conversation {
    Conversation::new("frames", "/assistant/7/items").items([
        SurfaceFrame::unavailable("s2").into_item("0001"),
        SurfaceFrame::failed("s3").into_item("0002"),
    ])
}

fn suggestions_default() -> Suggestions {
    Suggestions::new("/assistant/7/messages")
        .suggestion("Show unpaid orders")
        .suggestion("Which city grew the most?")
        .suggestion("Refunds this week")
}

fn sources_default() -> Sources {
    Sources::new()
        .source("Payments by method, October", "/reports/payments")
        .source("Income Tax Department", "https://www.incometax.gov.in/")
}

fn sources_citations() -> Stack {
    Stack::gap(3).children((
        el("p").children((
            Text::new("UPI took 62% of the week's payments").inline(),
            Citation::new(1, "/reports/payments"),
            Text::new(" and cards 21%").inline(),
            Citation::new(2, "https://www.incometax.gov.in/"),
            Text::new(".").inline(),
        )),
        Sources::new()
            .source("Payments by method, October", "/reports/payments")
            .source("Income Tax Department", "https://www.incometax.gov.in/"),
    ))
}

fn threads_default() -> ThreadList {
    ThreadList::new("Your conversations")
        .thread("Payments by method this week", "/assistant/7", true)
        .thread("Pending refunds in Ludhiana", "/assistant/6", false)
        .thread("GST summary for October", "/assistant/5", false)
}
