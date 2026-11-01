//! The plain text of an HTML body, for messages that have no text part:
//! html5ever's tokenizer reads the HTML (entities, comments, raw text
//! elements), the words are joined by one space, `<br>` and `</p>` end a
//! line, and what `<head>`, `<style>`, `<script>` and `<template>` hold is
//! left out.

use std::cell::{Cell, RefCell};

use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};

#[derive(Default)]
struct Text {
    out: RefCell<String>,
    /// Blanks were read since the last word.
    space: Cell<bool>,
    /// Nothing was written on the current line yet.
    line_start: Cell<bool>,
    /// The raw text element being skipped (`script`, `style`).
    skipping: RefCell<Option<String>>,
    in_head: Cell<bool>,
    in_template: Cell<bool>,
}

impl Text {
    fn tag(&self, tag: &Tag) -> TokenSinkResult<()> {
        let name: &str = &tag.name;
        let start = tag.kind == TagKind::StartTag;
        let skipped = self.skipping.borrow().clone();
        if let Some(skipped) = skipped {
            if !start && skipped == name {
                self.skipping.replace(None);
            }
            return TokenSinkResult::Continue;
        }
        match (name, start) {
            ("script", true) => {
                self.skipping.replace(Some(name.to_owned()));
                return TokenSinkResult::RawData(RawKind::ScriptData);
            }
            ("style", true) => {
                self.skipping.replace(Some(name.to_owned()));
                return TokenSinkResult::RawData(RawKind::Rawtext);
            }
            ("head", _) => self.in_head.set(start && !tag.self_closing),
            ("body", true) => self.in_head.set(false),
            ("template", _) => self.in_template.set(start && !tag.self_closing),
            ("br", _) | ("p", false) => self.line_break(),
            _ => {}
        }
        TokenSinkResult::Continue
    }

    fn line_break(&self) {
        self.out.borrow_mut().push('\n');
        self.space.set(false);
        self.line_start.set(true);
    }

    fn text(&self, text: &str) {
        if self.skipping.borrow().is_some() || self.in_head.get() || self.in_template.get() {
            return;
        }
        let mut out = self.out.borrow_mut();
        for c in text.chars() {
            if matches!(c, ' ' | '\t' | '\r' | '\n' | '\x0c') {
                self.space.set(true);
                continue;
            }
            if self.space.get() && !self.line_start.get() {
                out.push(' ');
            }
            out.push(c);
            self.space.set(false);
            self.line_start.set(false);
        }
    }
}

impl TokenSink for Text {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        match token {
            Token::TagToken(tag) => self.tag(&tag),
            Token::CharacterTokens(text) => {
                self.text(&text);
                TokenSinkResult::Continue
            }
            _ => TokenSinkResult::Continue,
        }
    }
}

/// The text of an HTML document.
pub(super) fn to_text(html: &str) -> String {
    let sink = Text {
        line_start: Cell::new(true),
        ..Text::default()
    };
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(html.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.out.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_lines_of_words() {
        for (html, text) in [
            (
                "<p>Hello <b>world</b></p><p>Again</p>",
                "Hello world\nAgain\n",
            ),
            (
                "  Line one<br>line\n  two<br/>three",
                "Line one\nline two\nthree",
            ),
            (
                "<html><head><title>T</title><style>p{x:1}</style></head><body>Body</body></html>",
                "Body",
            ),
            ("<script>if (a < b) { x() }</script>shown", "shown"),
            (
                "a &amp; b &lt;c&gt; &eacute;&#233;&#x41;&nbsp;z",
                "a & b <c> ééA\u{a0}z",
            ),
            ("x<!-- hidden --> y", "x y"),
            ("<template>t</template>visible", "visible"),
            ("<head><title>unclosed</title><body>after", "after"),
            ("a<span>b</span>c", "abc"),
        ] {
            assert_eq!(to_text(html), text, "{html:?}");
        }
    }
}
