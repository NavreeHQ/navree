//! The Markdown of the API reference page's descriptions.
//!
//! Invariants: whatever the text, the output has only the elements and
//! attributes the reader writes, each attribute in double quotes with no raw
//! quote or `<` inside; a link goes to `http`, `https`, `mailto`, a path or a
//! fragment; an image comes from a path or a `data:image/` URI.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_api::markdown::render;

const TAGS: [&str; 17] = [
    "p", "h4", "h5", "h6", "ul", "ol", "li", "pre", "code", "strong", "em", "a", "img", "/p",
    "/li", "/a", "/ul",
];

fn allowed_tag(name: &str) -> bool {
    let name = name.strip_prefix('/').unwrap_or(name);
    TAGS.iter()
        .any(|tag| tag.strip_prefix('/').unwrap_or(tag) == name)
}

fn check_link(url: &str) {
    let lower = url.to_ascii_lowercase();
    assert!(
        lower.starts_with("https://")
            || lower.starts_with("http://")
            || lower.starts_with("mailto:")
            || url.starts_with('#')
            || (url.starts_with('/') && !url.starts_with("//")),
        "{url}"
    );
}

fn check_image(url: &str) {
    assert!(
        (url.starts_with('/') && !url.starts_with("//"))
            || url.to_ascii_lowercase().starts_with("data:image/"),
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
                ("img", "src") => check_image(value),
                ("img", "alt") => {}
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
