//! A small Markdown reader for the descriptions in the document: paragraphs,
//! headings, lists, code blocks, inline code, emphasis, links and images.
//!
//! Everything else is text, and all text is escaped: a description can't add an
//! element, an attribute or a script to the page. A link goes to `http`,
//! `https`, `mailto`, a path or a fragment; an image comes from this origin or a
//! `data:image/` URI (the page's policy allows nothing else); anything else
//! keeps its text and loses the link or the image.

/// The deepest emphasis and link text nesting: past it, markers are text.
const MAX_INLINE_DEPTH: usize = 8;

/// The HTML of `text`, read as Markdown.
#[must_use]
pub fn render(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut lines = text.lines().peekable();
    let mut paragraph: Vec<&str> = Vec::new();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            flush_paragraph(&mut paragraph, &mut out);
        } else if let Some(fence) = fence_of(trimmed) {
            flush_paragraph(&mut paragraph, &mut out);
            out.push_str("<pre><code>");
            for code in lines.by_ref() {
                if code.trim_start().starts_with(fence) {
                    break;
                }
                escape_into(code, &mut out);
                out.push('\n');
            }
            out.push_str("</code></pre>");
        } else if let Some((level, title)) = heading_of(trimmed) {
            flush_paragraph(&mut paragraph, &mut out);
            // The page's own headings stop at h3: a description starts at h4.
            let tag = (level + 3).min(6);
            out.push_str(&format!("<h{tag}>"));
            inline(title, 0, &mut out);
            out.push_str(&format!("</h{tag}>"));
        } else if list_item(trimmed).is_some() {
            flush_paragraph(&mut paragraph, &mut out);
            let ordered = list_item(trimmed).is_some_and(|item| item.ordered);
            out.push_str(if ordered { "<ol>" } else { "<ul>" });
            let mut current = list_item(trimmed).map(|item| item.text);
            while let Some(item) = current {
                out.push_str("<li>");
                inline(item, 0, &mut out);
                out.push_str("</li>");
                current = None;
                if let Some(next) = lines.peek()
                    && let Some(next_item) = list_item(next.trim_start())
                    && next_item.ordered == ordered
                {
                    current = Some(next_item.text);
                    lines.next();
                }
            }
            out.push_str(if ordered { "</ol>" } else { "</ul>" });
        } else {
            paragraph.push(trimmed);
        }
    }
    flush_paragraph(&mut paragraph, &mut out);
    out
}

fn flush_paragraph(lines: &mut Vec<&str>, out: &mut String) {
    if lines.is_empty() {
        return;
    }
    out.push_str("<p>");
    let joined = lines.join("\n");
    inline(&joined, 0, out);
    out.push_str("</p>");
    lines.clear();
}

/// The closing marker of a code fence that `line` opens.
fn fence_of(line: &str) -> Option<&'static str> {
    if line.starts_with("```") {
        Some("```")
    } else if line.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

/// `## Title`.
fn heading_of(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let title = line.get(level..)?.strip_prefix(' ')?;
    Some((level, title.trim_end_matches([' ', '#'])))
}

struct ListItem<'a> {
    ordered: bool,
    text: &'a str,
}

/// `- item`, `* item` or `1. item`.
fn list_item(line: &str) -> Option<ListItem<'_>> {
    if let Some(text) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some(ListItem {
            ordered: false,
            text,
        });
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let text = line.get(digits..)?.strip_prefix(". ")?;
    Some(ListItem {
        ordered: true,
        text,
    })
}

/// Inline Markdown. Every recursive call takes a strictly shorter slice, and
/// `depth` caps how deep they go.
fn inline(text: &str, depth: usize, out: &mut String) {
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        let next = match ch {
            '\\' => escaped(rest, out),
            '`' => code_span(rest, out),
            '*' | '_' if depth < MAX_INLINE_DEPTH => emphasis(rest, ch, depth, out),
            '[' if depth < MAX_INLINE_DEPTH => link(rest, depth, out),
            '!' => image(rest, out),
            _ => None,
        };
        rest = next.unwrap_or_else(|| {
            push_escaped(ch, out);
            &rest[ch.len_utf8()..]
        });
    }
}

/// `\*` is `*`.
fn escaped<'a>(rest: &'a str, out: &mut String) -> Option<&'a str> {
    let after = rest.strip_prefix('\\')?;
    let ch = after.chars().next()?;
    if !ch.is_ascii_punctuation() {
        return None;
    }
    push_escaped(ch, out);
    Some(&after[ch.len_utf8()..])
}

/// `` `code` `` and ``` ``co`de`` ```.
fn code_span<'a>(rest: &'a str, out: &mut String) -> Option<&'a str> {
    let ticks = rest.bytes().take_while(|byte| *byte == b'`').count();
    let marker = rest.get(..ticks)?;
    let after = rest.get(ticks..)?;
    let end = after.find(marker)?;
    out.push_str("<code>");
    escape_into(after.get(..end)?, out);
    out.push_str("</code>");
    after.get(end + ticks..)
}

/// `**strong**`, `*emphasis*` and the same with `_`.
fn emphasis<'a>(rest: &'a str, marker: char, depth: usize, out: &mut String) -> Option<&'a str> {
    let double: &str = if marker == '*' { "**" } else { "__" };
    let single: &str = if marker == '*' { "*" } else { "_" };
    let (open, tag) = if rest.starts_with(double) {
        (double, "strong")
    } else {
        (single, "em")
    };
    let after = rest.strip_prefix(open)?;
    // `snake_case` and `2 * 3` stay as they are: an opener needs text after it.
    if after.chars().next().is_none_or(char::is_whitespace) {
        return None;
    }
    let end = after.find(open)?;
    let body = after.get(..end)?;
    if body.is_empty() || body.ends_with(char::is_whitespace) {
        return None;
    }
    let tail = after.get(end + open.len()..)?;
    if marker == '_' && tail.chars().next().is_some_and(char::is_alphanumeric) {
        return None;
    }
    out.push('<');
    out.push_str(tag);
    out.push('>');
    inline(body, depth + 1, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    Some(tail)
}

/// `[text](url)`, and `[text]` with a bad URL when the URL isn't allowed.
fn link<'a>(rest: &'a str, depth: usize, out: &mut String) -> Option<&'a str> {
    let (label, url, tail) = bracketed(rest)?;
    if is_safe_link(url) {
        out.push_str("<a href=\"");
        escape_into(url, out);
        out.push_str("\" rel=\"noopener noreferrer\">");
        inline(label, depth + 1, out);
        out.push_str("</a>");
    } else {
        inline(label, depth + 1, out);
    }
    Some(tail)
}

/// `![alt](url)`: an image from this origin or `data:image/`, else its alt
/// text.
fn image<'a>(rest: &'a str, out: &mut String) -> Option<&'a str> {
    let (alt, url, tail) = bracketed(rest.strip_prefix('!')?)?;
    if is_safe_image(url) {
        out.push_str("<img src=\"");
        escape_into(url, out);
        out.push_str("\" alt=\"");
        escape_into(alt, out);
        out.push_str("\">");
    } else {
        escape_into(alt, out);
    }
    Some(tail)
}

/// The label, the URL and what follows in `[label](url)...`.
fn bracketed(rest: &str) -> Option<(&str, &str, &str)> {
    let after = rest.strip_prefix('[')?;
    let close = after.find(']')?;
    let label = after.get(..close)?;
    let target = after.get(close + 1..)?.strip_prefix('(')?;
    let end = target.find(')')?;
    let url = target.get(..end)?.trim();
    Some((label, url, target.get(end + 1..)?))
}

/// A control character, or a backslash: browsers read `/\\host` as `//host`.
fn has_unsafe_byte(url: &str) -> bool {
    url.bytes()
        .any(|byte| byte.is_ascii_control() || byte == b'\\')
}

fn is_safe_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    !has_unsafe_byte(url)
        && (lower.starts_with("https://")
            || lower.starts_with("http://")
            || lower.starts_with("mailto:")
            || url.starts_with('#')
            || (url.starts_with('/') && !url.starts_with("//")))
}

fn is_safe_image(url: &str) -> bool {
    !has_unsafe_byte(url)
        && ((url.starts_with('/') && !url.starts_with("//"))
            || url.to_ascii_lowercase().starts_with("data:image/"))
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
            render("Hello **big** *world* and `code`.\n\nNext."),
            "<p>Hello <strong>big</strong> <em>world</em> and <code>code</code>.</p><p>Next.</p>"
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
    fn html_in_a_description_is_text() {
        let html = render("<script>alert(1)</script> <img src=x onerror=y>");
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
    }

    #[test]
    fn only_safe_links_stay_links() {
        assert_eq!(
            render("[docs](https://navree.com/\"x)"),
            "<p><a href=\"https://navree.com/&quot;x\" rel=\"noopener noreferrer\">docs</a></p>"
        );
        assert_eq!(render("[bad](javascript:alert(1))"), "<p>bad)</p>");
        assert_eq!(render("[bad](JaVaScRiPt:alert)"), "<p>bad</p>");
        assert_eq!(render("[bad](//evil.example/x)"), "<p>bad</p>");
        assert_eq!(render("[bad](data:text/html,x)"), "<p>bad</p>");
    }

    #[test]
    fn images_only_from_this_origin_or_data_uris() {
        assert_eq!(
            render("![logo](/logo.png)"),
            "<p><img src=\"/logo.png\" alt=\"logo\"></p>"
        );
        assert_eq!(render("![logo](https://evil.example/x.png)"), "<p>logo</p>");
        assert!(render("![pixel](data:image/png;base64,AA)").contains("<img"));
    }

    #[test]
    fn a_backslash_never_makes_a_path_of_another_host() {
        let page = render("![pix](/\\evil.example/p.png) [link](/\\evil.example/x) [ok](/a/b)");
        assert!(!page.contains("src=\"/\\"), "{page}");
        assert!(!page.contains("href=\"/\\"), "{page}");
        assert!(page.contains("href=\"/a/b\""), "{page}");
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
    }

    #[test]
    fn multibyte_text_survives_every_marker() {
        assert_eq!(
            render("*é* `ü` [ñ](/ñ) \\é"),
            "<p><em>é</em> <code>ü</code> <a href=\"/ñ\" rel=\"noopener noreferrer\">ñ</a> \\é</p>"
        );
    }
}
