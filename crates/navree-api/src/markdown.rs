//! The descriptions in the document, read as Markdown and written as HTML.
//!
//! navree-markdown does the reading, and its rules are the ones the UI toolkit's
//! Markdown follows: raw HTML in a description is text, a link goes to `http`, `https`,
//! `mailto`, `tel`, a path, a query or a fragment, a picture comes from this site, and
//! the text and its nesting are held to a bound. This module only writes what the reader
//! says: a description can't add an element, an attribute or a script to the page, since
//! every piece of text and every address passes through `escape_into`.
//!
//! A description is trusted text (the authors of the app wrote it): a picture from a path
//! on this site is drawn, and a link to another site carries `rel="noopener noreferrer"`.
//! Headings start at `h4`, because the page's own headings stop at `h3`.

use navree_markdown::{Image, Item, Link, Reader, Settings, Tag};

/// The level `#` becomes: the page's own headings stop at `h3`.
const FIRST_HEADING: u8 = 4;

/// The HTML of `text`, read as Markdown.
#[must_use]
pub fn render(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    // What closes each open container, innermost last.
    let mut closing: Vec<&'static str> = Vec::new();
    for item in Reader::new(text, Settings::trusted()) {
        match item {
            Item::Start(tag) => closing.push(open(&tag, &mut out)),
            Item::End(_) => {
                if let Some(close) = closing.pop() {
                    out.push_str(close);
                }
            }
            Item::Text(text) => escape_into(&text, &mut out),
            Item::Code(code) => {
                out.push_str("<code>");
                escape_into(&code, &mut out);
                out.push_str("</code>");
            }
            Item::SoftBreak => out.push('\n'),
            Item::HardBreak => out.push_str("<br>"),
            Item::Rule => out.push_str("<hr>"),
            // The box can't be drawn without markup an assistive reader needs words for.
            Item::Task(done) => out.push_str(if done { "[x] " } else { "[ ] " }),
            Item::CodeBlock(block) => {
                out.push_str("<pre><code>");
                escape_into(&block.code, &mut out);
                out.push_str("</code></pre>");
            }
            Item::Image(image) => picture(&image, &mut out),
            Item::Cut => out.push_str("<p>The rest of this text is not shown.</p>"),
        }
    }
    // The reader closes everything it opens; this is for a stream cut short.
    while let Some(close) = closing.pop() {
        out.push_str(close);
    }
    out
}

/// Writes the opening of a container and returns what closes it.
fn open(tag: &Tag, out: &mut String) -> &'static str {
    match tag {
        Tag::Paragraph | Tag::RawHtml => {
            out.push_str("<p>");
            "</p>"
        }
        Tag::Heading(level) => match (FIRST_HEADING + (*level).clamp(1, 6) - 1).min(6) {
            4 => {
                out.push_str("<h4>");
                "</h4>"
            }
            5 => {
                out.push_str("<h5>");
                "</h5>"
            }
            _ => {
                out.push_str("<h6>");
                "</h6>"
            }
        },
        Tag::Quote => {
            out.push_str("<blockquote>");
            "</blockquote>"
        }
        Tag::List(Some(first)) if *first != 1 => {
            out.push_str("<ol start=\"");
            out.push_str(&first.to_string());
            out.push_str("\">");
            "</ol>"
        }
        Tag::List(Some(_)) => {
            out.push_str("<ol>");
            "</ol>"
        }
        Tag::List(None) => {
            out.push_str("<ul>");
            "</ul>"
        }
        Tag::ListItem => {
            out.push_str("<li>");
            "</li>"
        }
        Tag::Table => {
            out.push_str("<table>");
            "</table>"
        }
        Tag::TableHead => {
            out.push_str("<thead><tr>");
            "</tr></thead>"
        }
        Tag::TableRow => {
            out.push_str("<tr>");
            "</tr>"
        }
        Tag::Cell { header: true, .. } => {
            out.push_str("<th scope=\"col\">");
            "</th>"
        }
        Tag::Cell { header: false, .. } => {
            out.push_str("<td>");
            "</td>"
        }
        Tag::Emphasis => {
            out.push_str("<em>");
            "</em>"
        }
        Tag::Strong => {
            out.push_str("<strong>");
            "</strong>"
        }
        Tag::Strikethrough => {
            out.push_str("<del>");
            "</del>"
        }
        Tag::Link(link) => {
            anchor(link, out);
            "</a>"
        }
        // A container this page has no element for: its content stays.
        Tag::Other => "",
    }
}

fn anchor(link: &Link, out: &mut String) {
    out.push_str("<a href=\"");
    escape_into(&link.href, out);
    out.push('"');
    if let Some(title) = &link.title {
        out.push_str(" title=\"");
        escape_into(title, out);
        out.push('"');
    }
    if link.off_site {
        out.push_str(" rel=\"noopener noreferrer\"");
    }
    out.push('>');
}

/// A picture from this site, or the words written for it.
fn picture(image: &Image, out: &mut String) {
    let Some(src) = &image.src else {
        escape_into(&image.alt, out);
        return;
    };
    out.push_str("<img src=\"");
    escape_into(src, out);
    out.push_str("\" alt=\"");
    escape_into(&image.alt, out);
    out.push('"');
    if let Some(title) = &image.title {
        out.push_str(" title=\"");
        escape_into(title, out);
        out.push('"');
    }
    out.push('>');
}

fn push_escaped(ch: char, out: &mut String) {
    match ch {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        '\'' => out.push_str("&#39;"),
        ch => out.push(ch),
    }
}

/// Appends `text`, escaped for element content and quoted attribute values.
pub(crate) fn escape_into(text: &str, out: &mut String) {
    for ch in text.chars() {
        push_escaped(ch, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_and_inline_marks() {
        assert_eq!(
            render("Hello **big** *world* ~~old~~ and `code`.\n\nNext."),
            "<p>Hello <strong>big</strong> <em>world</em> <del>old</del> and <code>code</code>.</p><p>Next.</p>"
        );
    }

    #[test]
    fn lists_headings_and_fences() {
        assert_eq!(
            render("# Title\n- one\n- two\n\n1. first\n```\n<a>\n```"),
            "<h4>Title</h4><ul><li>one</li><li>two</li></ul><ol><li>first</li></ol><pre><code>&lt;a&gt;\n</code></pre>"
        );
    }

    #[test]
    fn headings_start_at_four_and_stop_at_six() {
        assert_eq!(
            render("# a\n\n## b\n\n### c\n\n###### d"),
            "<h4>a</h4><h5>b</h5><h6>c</h6><h6>d</h6>"
        );
    }

    #[test]
    fn nested_lists_quotes_rules_and_numbers_are_read() {
        assert_eq!(
            render("- a\n  - b\n"),
            "<ul><li>a<ul><li>b</li></ul></li></ul>"
        );
        assert_eq!(
            render("3. c\n4. d\n"),
            "<ol start=\"3\"><li>c</li><li>d</li></ol>"
        );
        assert_eq!(render("> quoted"), "<blockquote><p>quoted</p></blockquote>");
        assert_eq!(render("a\n\n---\n\nb"), "<p>a</p><hr><p>b</p>");
        assert_eq!(
            render("- [x] done\n- [ ] open\n"),
            "<ul><li>[x] done</li><li>[ ] open</li></ul>"
        );
    }

    #[test]
    fn a_table_is_read() {
        assert_eq!(
            render("| a | b |\n|---|---|\n| 1 | 2 |\n"),
            "<table><thead><tr><th scope=\"col\">a</th><th scope=\"col\">b</th></tr></thead><tr><td>1</td><td>2</td></tr></table>"
        );
    }

    #[test]
    fn html_in_a_description_is_text() {
        let html = render("<script>alert(1)</script> <img src=x onerror=y>");
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        let inline = render("a <b onclick=x>bold</b> c");
        assert!(!inline.contains("<b"), "{inline}");
        assert!(inline.contains("&lt;b onclick=x&gt;"), "{inline}");
    }

    #[test]
    fn only_safe_links_stay_links() {
        assert_eq!(
            render("[docs](https://navree.com/a?x=1&y=2)"),
            "<p><a href=\"https://navree.com/a?x=1&amp;y=2\" rel=\"noopener noreferrer\">docs</a></p>"
        );
        assert_eq!(
            render("[here](/docs/start)"),
            "<p><a href=\"/docs/start\">here</a></p>"
        );
        assert_eq!(render("[bad](javascript:alert(1))"), "<p>bad</p>");
        assert_eq!(render("[bad](JaVaScRiPt:alert)"), "<p>bad</p>");
        assert_eq!(render("[bad](data:text/html,x)"), "<p>bad</p>");
        assert_eq!(render("[bad](vbscript:x)"), "<p>bad</p>");
        assert_eq!(render("[nothing]()"), "<p>nothing</p>");
    }

    #[test]
    fn a_link_to_another_site_by_a_bare_double_slash_is_marked_like_any_other() {
        assert_eq!(
            render("[far](//other.example/x)"),
            "<p><a href=\"//other.example/x\" rel=\"noopener noreferrer\">far</a></p>"
        );
    }

    #[test]
    fn a_title_is_escaped_in_its_attribute() {
        let html = render("[x](/a \"say \\\"hi\\\" <b>\")");
        assert!(
            html.contains("title=\"say &quot;hi&quot; &lt;b&gt;\""),
            "{html}"
        );
    }

    #[test]
    fn images_only_from_this_site() {
        assert_eq!(
            render("![logo](/logo.png)"),
            "<p><img src=\"/logo.png\" alt=\"logo\"></p>"
        );
        assert_eq!(render("![logo](https://evil.example/x.png)"), "<p>logo</p>");
        assert_eq!(render("![pixel](data:image/png;base64,AA)"), "<p>pixel</p>");
    }

    #[test]
    fn the_words_of_a_picture_are_escaped() {
        let html = render("![a \"quoted\" <b>](/x.png \"t\")");
        assert!(
            html.contains("alt=\"a &quot;quoted&quot; &lt;b&gt;\" title=\"t\""),
            "{html}"
        );
    }

    #[test]
    fn a_backslash_never_makes_a_path_of_another_host() {
        let page = render("![pix](/\\evil.example/p.png) [link](/\\evil.example/x) [ok](/a/b)");
        // The picture is not drawn, and the link is marked as the link to another site
        // it is.
        assert!(!page.contains("<img"), "{page}");
        assert!(
            page.contains("<a href=\"/\\evil.example/x\" rel=\"noopener noreferrer\">link</a>"),
            "{page}"
        );
        assert!(page.contains("<a href=\"/a/b\">ok</a>"), "{page}");
    }

    #[test]
    fn plain_identifiers_keep_their_underscores_and_stars() {
        assert_eq!(
            render("snake_case_name and 2 * 3 * 4"),
            "<p>snake_case_name and 2 * 3 * 4</p>"
        );
    }

    #[test]
    fn unclosed_marks_are_text() {
        assert_eq!(
            render("**bold `code [link](x"),
            "<p>**bold `code [link](x</p>"
        );
    }

    #[test]
    fn deep_nesting_ends() {
        let text = "*a ".repeat(50_000);
        assert!(render(&text).len() >= text.len());
        let brackets = "[".repeat(50_000);
        assert_eq!(render(&brackets), format!("<p>{brackets}</p>"));
        let quotes = format!("{}deep", "> ".repeat(500));
        let html = render(&quotes);
        assert!(html.contains("deep"), "{}", html.len());
        assert_eq!(
            html.matches("<blockquote>").count(),
            html.matches("</blockquote>").count()
        );
    }

    #[test]
    fn multibyte_text_survives_every_marker() {
        assert_eq!(
            render("*é* `ü` [ñ](/ñ) \\é"),
            "<p><em>é</em> <code>ü</code> <a href=\"/ñ\">ñ</a> \\é</p>"
        );
    }

    #[test]
    fn a_text_past_the_limit_says_so() {
        let text = "ਪੰਜਾਬ ".repeat(60_000);
        assert!(render(&text).ends_with("<p>The rest of this text is not shown.</p>"));
    }
}
