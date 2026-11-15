//! What the header and the README promise a client's author, held to the
//! words that carry the promise: a sentence that goes missing fails here.

const HEADER: &str = include_str!("../include/navree_core.h");
const README: &str = include_str!("../README.md");

/// The words of `text` on one line, as a reader takes them: no comment
/// leaders, no code marks, one space between words.
fn read(text: &str) -> String {
    text.replace(" * ", " ")
        .replace('`', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The core copies the body it is handed into the call before it can look at
/// its size, so the guard that keeps a very large body out of memory is the
/// client's transport, which refuses it before it reads it.
#[test]
fn a_client_is_told_to_refuse_a_body_past_the_cap_before_it_hands_it_over() {
    for (name, text) in [("the header", HEADER), ("the README", README)] {
        let text = read(text);
        assert!(
            text.contains("refuses a body larger than MAX_BODY before it hands it over"),
            "{name} does not say that a client refuses a larger body before it hands it over"
        );
    }
}
