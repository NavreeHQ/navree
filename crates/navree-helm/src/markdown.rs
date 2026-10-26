//! Markdown fields, rendered without raw HTML.

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};

/// `text` as HTML: CommonMark with tables and strikethrough. Raw HTML is
/// shown as text, and links and images keep only `http(s)`, `mailto` and
/// relative URLs.
pub(crate) fn render(text: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let events = Parser::new_ext(text, options).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: safe_url(dest_url),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: safe_url(dest_url),
            title,
            id,
        }),
        other => other,
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

/// `url` when it is `http(s)`, `mailto` or relative; `#` otherwise.
fn safe_url(url: CowStr<'_>) -> CowStr<'_> {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    let scheme = lower
        .split_once(':')
        .map(|(scheme, _)| scheme)
        .filter(|scheme| !scheme.contains(['/', '?', '#']));
    match scheme {
        None => url,
        Some("http" | "https" | "mailto") => url,
        Some(_) => CowStr::Borrowed("#"),
    }
}

/// The first line of `text`, without Markdown markers, at most `max`
/// characters.
pub(crate) fn first_line(text: &str, max: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let line = line.trim_start_matches(['#', '>', '-', '*', ' ']);
    truncate(line, max)
}

/// `text` cut to `max` characters, with an ellipsis when cut.
pub(crate) fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_renders_without_raw_html() {
        let html = render(
            "# Notes\n\n**Bold** <script>alert(1)</script> [x](javascript:alert(1)) [y](https://navree.com)",
        );
        assert!(html.contains("<h1>Notes</h1>"), "{html}");
        assert!(html.contains("<strong>Bold</strong>"), "{html}");
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(html.contains(r##"<a href="#">x</a>"##), "{html}");
        assert!(
            html.contains(r#"<a href="https://navree.com">y</a>"#),
            "{html}"
        );
    }

    #[test]
    fn first_lines_for_lists() {
        assert_eq!(first_line("\n# Title\nmore", 80), "Title");
        assert_eq!(first_line("abcdef", 4), "abc…");
        assert_eq!(first_line("", 4), "");
    }
}
