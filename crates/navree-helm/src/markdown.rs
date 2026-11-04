//! Markdown fields, rendered without raw HTML.

use navree_ui::Markdown;

/// `text` as HTML: CommonMark with tables, strikethrough and task lists, drawn by the UI
/// toolkit's Markdown, so one set of rules decides what a link or raw HTML may be. Raw HTML
/// is shown as text, a link keeps only `http(s)`, `mailto`, `tel` and relative URLs (any
/// other link keeps its words and loses the link), and images come from this site only.
pub(crate) fn render(text: &str) -> String {
    Markdown::trusted(text).to_fragment_html()
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
            "# Notes

**Bold** <script>alert(1)</script> [x](javascript:alert(1)) [y](https://navree.com)",
        );
        assert!(html.contains("<h1>Notes</h1>"), "{html}");
        assert!(html.contains("<strong>Bold</strong>"), "{html}");
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        // The link with a script in it keeps its words and loses the link.
        assert!(!html.contains("javascript:"), "{html}");
        assert!(!html.contains(r##"href="#""##), "{html}");
        assert!(html.contains("x"), "{html}");
        assert!(
            html.contains(r#"<a href="https://navree.com" rel="noopener noreferrer" data-external>y</a>"#),
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
