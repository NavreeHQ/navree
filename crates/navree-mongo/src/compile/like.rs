//! `LIKE` patterns as MongoDB regular expressions.

/// One piece of a `LIKE` pattern.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Piece {
    /// `%`: any run of characters.
    Any,
    /// `_`: one character.
    One,
    /// A literal character.
    Literal(char),
}

/// The regular expression of a `LIKE` pattern: `%` matches any run of
/// characters, `_` one character, a backslash makes the next character
/// literal (as on PostgreSQL and MySQL), everything else is literal. The
/// expression is anchored where the pattern does not start or end with
/// `%`, so `acme%` is `^acme` (a prefix, which an index serves), `%acme%`
/// is `acme`, and `acme` is `^acme\z` (no newline may follow, as with
/// `LIKE`). Run it with the `s` option, so `%` and `_` match newlines too.
///
/// ```text
/// like_regex("acme%")    == "^acme"
/// like_regex("%a_c%")    == "a.c"
/// like_regex("50\\%")    == "^50%\\z"
/// like_regex("1+1=2")    == "^1\\+1=2\\z"
/// ```
pub(crate) fn like_regex(pattern: &str) -> String {
    let mut pieces = Vec::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        let piece = match c {
            '\\' => Piece::Literal(chars.next().unwrap_or('\\')),
            '%' => Piece::Any,
            '_' => Piece::One,
            c => Piece::Literal(c),
        };
        // `%%` is `%`.
        if !(piece == Piece::Any && pieces.last() == Some(&Piece::Any)) {
            pieces.push(piece);
        }
    }
    let anchored_start = pieces.first() != Some(&Piece::Any);
    let anchored_end = pieces.last() != Some(&Piece::Any);
    let inner = {
        let start = usize::from(!anchored_start);
        let end = pieces.len() - usize::from(!anchored_end && !pieces.is_empty());
        &pieces[start.min(end)..end]
    };
    let mut regex = String::new();
    if anchored_start {
        regex.push('^');
    }
    for piece in inner {
        match piece {
            Piece::Any => regex.push_str(".*"),
            Piece::One => regex.push('.'),
            Piece::Literal(c) => {
                if "\\^$.|?*+()[]{}".contains(*c) {
                    regex.push('\\');
                }
                regex.push(*c);
            }
        }
    }
    if anchored_end {
        regex.push_str("\\z");
    }
    regex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_become_anchored_regexes() {
        assert_eq!(like_regex("acme"), "^acme\\z");
        assert_eq!(like_regex("acme%"), "^acme");
        assert_eq!(like_regex("%acme"), "acme\\z");
        assert_eq!(like_regex("%acme%"), "acme");
        assert_eq!(like_regex("%a_c%"), "a.c");
        assert_eq!(like_regex("a%%b"), "^a.*b\\z");
        assert_eq!(like_regex("%"), "");
        assert_eq!(like_regex("%%"), "");
        assert_eq!(like_regex(""), "^\\z");
    }

    #[test]
    fn metacharacters_and_escapes_are_literal() {
        assert_eq!(like_regex("1+1=2"), "^1\\+1=2\\z");
        assert_eq!(like_regex("(a|b).*"), "^\\(a\\|b\\)\\.\\*\\z");
        assert_eq!(like_regex("50\\%"), "^50%\\z");
        assert_eq!(like_regex("a\\_b%"), "^a_b");
        assert_eq!(like_regex("ends\\"), "^ends\\\\\\z");
        assert_eq!(like_regex("$5 [net]"), "^\\$5 \\[net\\]\\z");
    }
}
