//! The Markdown of the API reference page's descriptions.
//!
//! Invariants: whatever the text, the output has only the elements and
//! attributes the writer uses, each attribute in double quotes with no raw
//! quote or `<` inside; a link has no control character and a scheme, when it
//! has one, of `http`, `https`, `mailto` or `tel`; a picture comes from a path
//! on this site.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_api::markdown::render;

const TAGS: [&str; 22] = [
    "p",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "pre",
    "code",
    "strong",
    "em",
    "del",
    "a",
    "img",
    "br",
    "hr",
    "blockquote",
    "table",
    "thead",
    "tr",
    "th",
    "td",
];

fn allowed_tag(name: &str) -> bool {
    let name = name.strip_prefix('/').unwrap_or(name);
    TAGS.contains(&name)
}

fn has_control(url: &str) -> bool {
    url.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
}

/// The scheme of an address: what comes before the first `:`, when no `/`, `?` or `#`
/// comes sooner.
fn scheme(url: &str) -> Option<&str> {
    let end = url.find([':', '/', '?', '#'])?;
    if url.as_bytes()[end] == b':' {
        Some(&url[..end])
    } else {
        None
    }
}

fn check_link(url: &str) {
    assert!(!has_control(url), "{url}");
    if let Some(scheme) = scheme(url) {
        assert!(
            ["http", "https", "mailto", "tel"]
                .iter()
                .any(|name| name.eq_ignore_ascii_case(scheme)),
            "{url}"
        );
    }
}

fn check_image(url: &str) {
    assert!(!has_control(url), "{url}");
    assert!(scheme(url).is_none(), "{url}");
    let bytes = url.as_bytes();
    let slash = |at: usize| matches!(bytes.get(at), Some(b'/' | b'\\'));
    assert!(
        bytes.first() != Some(&b'\\') && !(slash(0) && slash(1)),
        "{url}"
    );
}

fn check(html: &str) {
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        let tag = &rest[start + 1..];
        let end = tag.find('>').expect("every tag closes");
        let body = &tag[..end];
        assert!(!body.contains('<'), "{body}");
        let (name, attributes) = body.split_once(' ').unwrap_or((body, ""));
        assert!(allowed_tag(name), "{name} in {html}");
        let mut attributes = attributes;
        while !attributes.is_empty() {
            let (key, value) = attributes.split_once("=\"").expect("a quoted attribute");
            let (value, after) = value.split_once('"').expect("a closed attribute");
            match (name, key) {
                ("a", "href") => check_link(value),
                ("a", "rel") => assert_eq!(value, "noopener noreferrer"),
                ("a" | "img", "title") => {}
                ("img", "src") => check_image(value),
                ("img", "alt") => {}
                ("ol", "start") => assert!(value.parse::<u64>().is_ok(), "{value}"),
                ("th", "scope") => assert_eq!(value, "col"),
                _ => panic!("{name} has {key}"),
            }
            attributes = after.strip_prefix(' ').unwrap_or(after);
        }
        rest = &tag[end + 1..];
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    check(&render(text));
});
