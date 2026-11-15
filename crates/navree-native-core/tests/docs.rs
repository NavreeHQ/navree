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

/// What a snapshot never keeps is said for what it is: the fields the page
/// marks. A field the page does not mark is kept, and a page that wants one
/// never kept marks it.
#[test]
fn the_readme_says_exactly_what_a_snapshot_never_keeps() {
    let readme = read(README);
    for words in [
        "never kept: a field the page marks as a password, a one-time code or a card's",
        "a plain or number field the page does not mark is kept",
    ] {
        assert!(readme.contains(words), "the README does not say: {words}");
    }
}

/// What an app's author runs into, said where they will look.
#[test]
fn the_readme_settles_what_an_app_will_run_into() {
    let readme = read(README);
    for words in [
        // An equal value starts nothing, a table's query included.
        "re-tapping the chosen segment is not a refresh",
        // A change names no other address for its form.
        "an apply needs a button",
        // What a kept shell draws before the server names anybody.
        "are drawn before the server names anybody",
        // An unknown kind of text.
        "an unknown kind of text is drawn as plain text",
        // A write that never left the device.
        "a write that never left the device is not a write",
    ] {
        assert!(readme.contains(words), "the README does not say: {words}");
    }
}
