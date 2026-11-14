//! The Rust name of a proto method, as the generated server trait spells it
//! (`GetDeal` is `get_deal`).

/// The Rust name prost-build gives the proto method `proto_method`, the name
/// the generated server trait spells: snake case (`GetDeal` is `get_deal`,
/// `GetHTTPStatus` is `get_http_status`), then `r#` before a Rust keyword
/// (`Type` is `r#type`), `_` after `_`, `super`, `self`, `Self`, `extern`
/// and `crate` (`Self` is `self_`), and `_` before a leading digit (`3D` is
/// `_3d`). A name already in that form reads as itself.
pub(crate) fn rust_name(proto_method: &str) -> String {
    sanitize(snake_case(proto_method))
}

/// Snake case as prost-build writes it (heck 0.5's `to_snake_case`): words
/// split on every character that is not alphanumeric and, inside a word,
/// before an upper-case character that follows a lower-case one (a digit
/// keeps the case before it), and before an upper-case character that
/// follows an upper-case one and precedes a lower-case one; each word
/// lower-cased, joined with `_`.
fn snake_case(name: &str) -> String {
    /// The case of the last cased character of the current word.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Boundary,
        Lower,
        Upper,
    }

    let mut out = String::with_capacity(name.len() + 4);
    let mut first_word = true;
    for word in name.split(|c: char| !c.is_alphanumeric()) {
        let mut chars = word.char_indices().peekable();
        let mut start = 0;
        let mut mode = Mode::Boundary;
        while let Some((i, c)) = chars.next() {
            let Some(&(next_i, next)) = chars.peek() else {
                push_word(&mut out, &mut first_word, &word[start..]);
                break;
            };
            let next_mode = if c.is_lowercase() {
                Mode::Lower
            } else if c.is_uppercase() {
                Mode::Upper
            } else {
                mode
            };
            if next_mode == Mode::Lower && next.is_uppercase() {
                push_word(&mut out, &mut first_word, &word[start..next_i]);
                start = next_i;
                mode = Mode::Boundary;
            } else if mode == Mode::Upper && c.is_uppercase() && next.is_lowercase() {
                push_word(&mut out, &mut first_word, &word[start..i]);
                start = i;
                mode = Mode::Boundary;
            } else {
                mode = next_mode;
            }
        }
    }
    out
}

/// Appends `word` lower-cased, after a `_` unless it is the first word.
fn push_word(out: &mut String, first_word: &mut bool, word: &str) {
    if !std::mem::take(first_word) {
        out.push('_');
    }
    let mut chars = word.chars().peekable();
    while let Some(c) = chars.next() {
        // A final capital sigma lower-cases to the final form.
        if c == 'Σ' && chars.peek().is_none() {
            out.push('ς');
        } else {
            out.extend(c.to_lowercase());
        }
    }
}

/// prost-build's `sanitize_identifier`: a name Rust would not take as it is
/// gets `r#` before it, `_` after it or `_` before it.
fn sanitize(name: String) -> String {
    let keyword = matches!(
        name.as_str(),
        "as" | "break"
            | "const"
            | "continue"
            | "else"
            | "enum"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "static"
            | "struct"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "async"
            | "await"
            | "try"
            | "gen"
    );
    if keyword {
        format!("r#{name}")
    } else if matches!(
        name.as_str(),
        "_" | "super" | "self" | "Self" | "extern" | "crate"
    ) {
        format!("{name}_")
    } else if name.starts_with(|c: char| c.is_numeric()) {
        format!("_{name}")
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_method_names_turn_into_rust_names_as_prost_build_writes_them() {
        for (proto, rust) in [
            ("GetDeal", "get_deal"),
            ("ListDeals", "list_deals"),
            ("GetHTTPStatus", "get_http_status"),
            ("XMLHttpRequest", "xml_http_request"),
            ("ListDealsV2", "list_deals_v2"),
            ("Get2FA", "get2_fa"),
            ("Get_Deal", "get_deal"),
            ("ABC", "abc"),
            ("ping", "ping"),
            ("Type", "r#type"),
            ("Self", "self_"),
            ("3D", "_3d"),
        ] {
            assert_eq!(rust_name(proto), rust, "{proto}");
            // A name already in Rust form reads as itself, so every Rust name
            // passes the boot check.
            let bare = rust.trim_start_matches("r#");
            assert_eq!(rust_name(bare).trim_start_matches("r#"), bare, "{rust}");
        }
        // Not in Rust form: each reads as another name (the boot check
        // refuses them).
        for declared in ["DeleteDeal", "deleteDeal", "get-deal", "ListDeals"] {
            assert_ne!(rust_name(declared), declared, "{declared}");
        }
    }
}
