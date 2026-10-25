//! Channel names: their kind (public, private, presence), validation, and the
//! patterns channel authorizers are registered under (`users.{id}`).

use std::collections::HashMap;

/// The longest channel name accepted (Pusher's limit).
pub(crate) const MAX_CHANNEL_NAME: usize = 200;

/// What a channel name's prefix makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChannelKind {
    /// Anyone may subscribe.
    Public,
    /// `private-…`: subscribing needs a signature from the auth endpoint.
    Private,
    /// `presence-…`: like private, plus a member list.
    Presence,
}

const PRIVATE: &str = "private-";
const PRESENCE: &str = "presence-";

impl ChannelKind {
    /// The kind of `channel`.
    pub(crate) fn of(channel: &str) -> ChannelKind {
        if channel.starts_with(PRIVATE) {
            ChannelKind::Private
        } else if channel.starts_with(PRESENCE) {
            ChannelKind::Presence
        } else {
            ChannelKind::Public
        }
    }
}

/// `channel` without its `private-` or `presence-` prefix (what channel
/// patterns are matched against).
pub(crate) fn unprefixed(channel: &str) -> &str {
    channel
        .strip_prefix(PRIVATE)
        .or_else(|| channel.strip_prefix(PRESENCE))
        .unwrap_or(channel)
}

/// Whether `c` may appear in a channel name.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '=' | '@' | ',' | '.' | ';')
}

/// Whether `name` is a valid channel name: 1 to 200 characters out of
/// `A-Z a-z 0-9 _ - = @ , . ;`.
pub(crate) fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_CHANNEL_NAME && name.chars().all(is_name_char)
}

/// A piece of a [`ChannelPattern`].
#[derive(Clone, Debug)]
enum Token {
    Literal(String),
    Param(String),
}

/// A channel pattern such as `crm.deals` or `users.{id}`: literal text and
/// `{name}` parameters, each matching one or more characters other than `.`.
#[derive(Clone, Debug)]
pub(crate) struct ChannelPattern {
    source: &'static str,
    tokens: Vec<Token>,
}

impl ChannelPattern {
    /// Parses `pattern`; the error says what is wrong with it.
    pub(crate) fn parse(pattern: &'static str) -> Result<ChannelPattern, String> {
        if pattern.is_empty() {
            return Err("a channel pattern cannot be empty".to_owned());
        }
        if pattern.starts_with(PRIVATE) || pattern.starts_with(PRESENCE) {
            return Err(format!(
                "write the pattern without the `private-` or `presence-` prefix (`{}`)",
                unprefixed(pattern)
            ));
        }
        let mut tokens = Vec::new();
        let mut rest = pattern;
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix('{') {
                let end = after
                    .find('}')
                    .ok_or_else(|| "unbalanced braces: `{` without `}`".to_owned())?;
                let name = &after[..end];
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Err(format!(
                        "invalid parameter name `{{{name}}}`: use letters, digits and `_`"
                    ));
                }
                if matches!(tokens.last(), Some(Token::Param(_))) {
                    return Err(format!(
                        "adjacent parameters before `{{{name}}}` are ambiguous; separate them with literal text"
                    ));
                }
                tokens.push(Token::Param(name.to_owned()));
                rest = &after[end + 1..];
            } else {
                let end = rest.find(['{', '}']).unwrap_or(rest.len());
                if rest[end..].starts_with('}') {
                    return Err("unbalanced braces: `}` without `{`".to_owned());
                }
                let literal = &rest[..end];
                if let Some(bad) = literal.chars().find(|&c| !is_name_char(c)) {
                    return Err(format!("character `{bad}` cannot appear in a channel name"));
                }
                tokens.push(Token::Literal(literal.to_owned()));
                rest = &rest[end..];
            }
        }
        Ok(ChannelPattern {
            source: pattern,
            tokens,
        })
    }

    /// The pattern as written.
    pub(crate) fn source(&self) -> &'static str {
        self.source
    }

    /// The parameters when `name` (without prefix) matches the pattern.
    pub(crate) fn matches(&self, name: &str) -> Option<HashMap<String, String>> {
        let mut params = Vec::new();
        match_tokens(&self.tokens, name, &mut params).then(|| params.into_iter().collect())
    }
}

/// Matches `tokens` against all of `input`, collecting parameters; a
/// parameter takes as much as it can (but no `.`) and gives back on failure.
fn match_tokens(tokens: &[Token], input: &str, params: &mut Vec<(String, String)>) -> bool {
    match tokens.split_first() {
        None => input.is_empty(),
        Some((Token::Literal(literal), rest)) => input
            .strip_prefix(literal.as_str())
            .is_some_and(|tail| match_tokens(rest, tail, params)),
        Some((Token::Param(name), rest)) => {
            let longest = input.find('.').unwrap_or(input.len());
            for end in (1..=longest).rev() {
                if !input.is_char_boundary(end) {
                    continue;
                }
                params.push((name.clone(), input[..end].to_owned()));
                if match_tokens(rest, &input[end..], params) {
                    return true;
                }
                params.pop();
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_the_prefix() {
        assert_eq!(ChannelKind::of("crm.deals"), ChannelKind::Public);
        assert_eq!(ChannelKind::of("private-crm.deals"), ChannelKind::Private);
        assert_eq!(ChannelKind::of("presence-room.1"), ChannelKind::Presence);
        assert_eq!(ChannelKind::of("privatechat"), ChannelKind::Public);
        assert_eq!(unprefixed("private-crm.deals"), "crm.deals");
        assert_eq!(unprefixed("presence-room.1"), "room.1");
        assert_eq!(unprefixed("news"), "news");
    }

    #[test]
    fn channel_names_are_validated() {
        assert!(is_valid_name(
            "private-users.0190e6a1-7c1b-7000-8000-000000000001"
        ));
        assert!(is_valid_name("a_b-c=d@e,f.g;h"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("has space"));
        assert!(!is_valid_name("slash/no"));
        assert!(!is_valid_name("é"));
        assert!(is_valid_name(&"x".repeat(MAX_CHANNEL_NAME)));
        assert!(!is_valid_name(&"x".repeat(MAX_CHANNEL_NAME + 1)));
    }

    #[test]
    fn literal_patterns_match_exactly() {
        let pattern = ChannelPattern::parse("crm.deals").unwrap();
        assert_eq!(pattern.source(), "crm.deals");
        assert_eq!(pattern.matches("crm.deals"), Some(HashMap::new()));
        assert_eq!(pattern.matches("crm.deals.1"), None);
        assert_eq!(pattern.matches("crm.deal"), None);
    }

    #[test]
    fn parameters_capture_one_segment() {
        let pattern = ChannelPattern::parse("users.{id}").unwrap();
        let params = pattern.matches("users.42").unwrap();
        assert_eq!(params.get("id").map(String::as_str), Some("42"));
        assert_eq!(pattern.matches("users."), None, "parameters are not empty");
        assert_eq!(
            pattern.matches("users.4.2"),
            None,
            "parameters stop at dots"
        );

        let pattern = ChannelPattern::parse("teams.{team}.chat-{room}").unwrap();
        let params = pattern.matches("teams.t1.chat-general").unwrap();
        assert_eq!(params["team"], "t1");
        assert_eq!(params["room"], "general");
        assert_eq!(pattern.matches("teams.t1.general"), None);

        // Backtracking: the parameter gives back what the literal needs.
        let pattern = ChannelPattern::parse("order-{id}-items").unwrap();
        assert_eq!(pattern.matches("order-a-b-items").unwrap()["id"], "a-b");
    }

    #[test]
    fn invalid_patterns_are_rejected_with_a_reason() {
        for (pattern, reason) in [
            ("", "empty"),
            ("users.{id", "unbalanced"),
            ("users.id}", "unbalanced"),
            ("users.{}", "parameter name"),
            ("users.{a-b}", "parameter name"),
            ("users.{a}{b}", "adjacent"),
            ("private-crm.deals", "prefix"),
            ("presence-room.{id}", "prefix"),
            ("bad name", "character"),
        ] {
            let error = ChannelPattern::parse(pattern).expect_err(pattern);
            assert!(error.contains(reason), "{pattern}: {error}");
        }
    }
}
