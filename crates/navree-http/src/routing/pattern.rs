//! [`PathPattern`]: a route's path, parsed and checked once when the route is
//! mounted.
//!
//! The grammar, a segment at a time after a leading `/`:
//!
//! - a **static** segment: one or more bytes of `A-Z a-z 0-9` and
//!   `- . _ ~ ! $ & ' ( ) * + , ; = : @`, matched byte for byte against the
//!   request's raw (still percent-encoded) segment. `.` and `..` are refused,
//!   and so are `%`, `{`, `}` and `\`: a static segment never needs decoding;
//! - a **parameter** `{name}`: one non-empty segment;
//! - a **catch-all** `{*name}`, last: the rest of the path, at least one
//!   byte, slashes included.
//!
//! Names are `[A-Za-z_][A-Za-z0-9_]*`, unique within the pattern. A
//! parameter is a whole segment (`/v{n}` and `/{name}.json` are refused), a
//! pattern has at most [`MAX_PARAMS`] parameters, no empty segment and no
//! trailing slash; `/` alone is the root. Two patterns have the same
//! [`shape`](PathPattern::shape) when they differ only in parameter names:
//! they match the same paths, so mounting both for one method is a conflict.

use std::fmt;
use std::sync::Arc;

/// The most parameters one pattern may have.
pub const MAX_PARAMS: usize = 16;

/// The longest parameter name accepted.
const MAX_NAME: usize = 64;

/// One segment of a [`PathPattern`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    /// Matched byte for byte.
    Static(Box<str>),
    /// `{name}`: one segment.
    Param,
    /// `{*name}`: the rest of the path.
    CatchAll,
}

/// A parsed route path: its segments and its parameter names in order.
///
/// ```
/// use navree_http::routing::{PathPattern, Segment};
///
/// let pattern = PathPattern::parse("/deals/{id}/files/{*path}").unwrap();
/// let names: Vec<&str> = pattern.param_names().iter().map(|name| &**name).collect();
/// assert_eq!(names, ["id", "path"]);
/// assert_eq!(pattern.segments()[0], Segment::Static("deals".into()));
/// assert_eq!(pattern.shape(), "/deals/{}/files/{*}");
/// assert!(PathPattern::parse("/deals/:id").is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathPattern {
    text: Box<str>,
    segments: Vec<Segment>,
    names: Arc<[Box<str>]>,
}

impl PathPattern {
    /// Parses and checks `pattern` (see the [module documentation](self)).
    pub fn parse(pattern: &str) -> Result<Self, PatternError> {
        let problem = |kind| PatternError {
            pattern: pattern.to_owned(),
            kind,
        };
        let Some(rest) = pattern.strip_prefix('/') else {
            return Err(problem(PatternErrorKind::NotAbsolute));
        };
        let mut segments = Vec::new();
        let mut names: Vec<Box<str>> = Vec::new();
        if !rest.is_empty() {
            let parts: Vec<&str> = rest.split('/').collect();
            let last = parts.len() - 1;
            for (index, part) in parts.into_iter().enumerate() {
                let segment = parse_segment(part).map_err(problem)?;
                match &segment {
                    Parsed::Static(_) => {}
                    Parsed::Param(name) | Parsed::CatchAll(name) => {
                        if matches!(segment, Parsed::CatchAll(_)) && index != last {
                            return Err(problem(PatternErrorKind::CatchAllNotLast(
                                (*name).to_owned(),
                            )));
                        }
                        if names.iter().any(|seen| **seen == **name) {
                            return Err(problem(PatternErrorKind::DuplicateName(
                                (*name).to_owned(),
                            )));
                        }
                        if names.len() == MAX_PARAMS {
                            return Err(problem(PatternErrorKind::TooManyParams));
                        }
                        names.push((*name).into());
                    }
                }
                segments.push(match segment {
                    Parsed::Static(text) => Segment::Static(text.into()),
                    Parsed::Param(_) => Segment::Param,
                    Parsed::CatchAll(_) => Segment::CatchAll,
                });
            }
        }
        Ok(Self {
            text: pattern.into(),
            segments,
            names: names.into(),
        })
    }

    /// The pattern as written.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The segments after the leading `/` (none for the root).
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The parameter names, in order.
    pub fn param_names(&self) -> &[Box<str>] {
        &self.names
    }

    /// The names, shared (matches hand them out without copying).
    pub(crate) fn shared_names(&self) -> Arc<[Box<str>]> {
        self.names.clone()
    }

    /// The pattern with its parameter names left out (`/deals/{}`,
    /// `/files/{*}`): two patterns with the same shape match the same paths.
    pub fn shape(&self) -> String {
        if self.segments.is_empty() {
            return "/".to_owned();
        }
        let mut out = String::new();
        for segment in &self.segments {
            out.push('/');
            match segment {
                Segment::Static(text) => out.push_str(text),
                Segment::Param => out.push_str("{}"),
                Segment::CatchAll => out.push_str("{*}"),
            }
        }
        out
    }
}

impl fmt::Display for PathPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

enum Parsed<'a> {
    Static(&'a str),
    Param(&'a str),
    CatchAll(&'a str),
}

fn parse_segment(part: &str) -> Result<Parsed<'_>, PatternErrorKind> {
    if part.is_empty() {
        return Err(PatternErrorKind::EmptySegment);
    }
    if part.starts_with(':') || part.starts_with('*') {
        return Err(PatternErrorKind::OldSyntax(part.to_owned()));
    }
    if part.contains(['{', '}']) {
        let Some(inner) = part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) else {
            let balanced = part.matches('{').count() == part.matches('}').count();
            return Err(if balanced {
                PatternErrorKind::PartialParam(part.to_owned())
            } else {
                PatternErrorKind::UnbalancedBraces(part.to_owned())
            });
        };
        let (catch_all, name) = match inner.strip_prefix('*') {
            Some(name) => (true, name),
            None => (false, inner),
        };
        if !is_name(name) {
            return Err(PatternErrorKind::InvalidName(part.to_owned()));
        }
        return Ok(if catch_all {
            Parsed::CatchAll(name)
        } else {
            Parsed::Param(name)
        });
    }
    if part == "." || part == ".." {
        return Err(PatternErrorKind::DotSegment);
    }
    if let Some(byte) = part.bytes().find(|byte| !is_static_byte(*byte)) {
        return Err(PatternErrorKind::InvalidChar(
            part.to_owned(),
            char::from(byte),
        ));
    }
    Ok(Parsed::Static(part))
}

/// `[A-Za-z_][A-Za-z0-9_]*`, at most [`MAX_NAME`] bytes.
fn is_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= MAX_NAME
        && bytes
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// What a static segment may hold: RFC 3986 `pchar` without `%`.
fn is_static_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
}

/// A route path that cannot be mounted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError {
    /// The pattern as written.
    pub pattern: String,
    /// What is wrong with it.
    pub kind: PatternErrorKind,
}

/// What is wrong with a route path.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternErrorKind {
    /// It does not start with `/`.
    NotAbsolute,
    /// `//`, or a trailing `/` (other than the root `/`).
    EmptySegment,
    /// `.` or `..` as a segment.
    DotSegment,
    /// `:id` or `*rest`, the syntax of older routers.
    OldSyntax(String),
    /// `{` without `}` or the other way round.
    UnbalancedBraces(String),
    /// A parameter that is part of a segment (`v{n}`, `{name}.json`).
    PartialParam(String),
    /// A parameter name that is empty, too long or not `[A-Za-z_][A-Za-z0-9_]*`.
    InvalidName(String),
    /// The same parameter name twice.
    DuplicateName(String),
    /// A catch-all that is not the last segment.
    CatchAllNotLast(String),
    /// More than [`MAX_PARAMS`] parameters.
    TooManyParams,
    /// A byte a static segment may not hold.
    InvalidChar(String, char),
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            PatternErrorKind::NotAbsolute => write!(f, "`{}` must start with `/`", self.pattern),
            PatternErrorKind::EmptySegment => write!(
                f,
                "`{}` has an empty segment (`//` or a trailing `/`)",
                self.pattern
            ),
            PatternErrorKind::DotSegment => {
                write!(f, "`{}` has a `.` or `..` segment", self.pattern)
            }
            PatternErrorKind::OldSyntax(segment) => write!(
                f,
                "segment `{segment}` uses old capture syntax; write `{{param}}` or `{{*rest}}`"
            ),
            PatternErrorKind::UnbalancedBraces(segment) => {
                write!(f, "segment `{segment}` has unbalanced braces")
            }
            PatternErrorKind::PartialParam(segment) => write!(
                f,
                "segment `{segment}` mixes text and a parameter; a parameter is a whole segment"
            ),
            PatternErrorKind::InvalidName(segment) => write!(
                f,
                "segment `{segment}`: a parameter name is a letter or `_` then letters, digits or `_` (at most {MAX_NAME})"
            ),
            PatternErrorKind::DuplicateName(name) => {
                write!(f, "`{}` names the parameter `{name}` twice", self.pattern)
            }
            PatternErrorKind::CatchAllNotLast(name) => write!(
                f,
                "`{}`: the catch-all `{{*{name}}}` must be the last segment",
                self.pattern
            ),
            PatternErrorKind::TooManyParams => write!(
                f,
                "`{}` has more than {MAX_PARAMS} parameters",
                self.pattern
            ),
            PatternErrorKind::InvalidChar(segment, byte) => write!(
                f,
                "segment `{segment}` holds `{}`, which a static segment may not",
                byte.escape_default()
            ),
        }
    }
}

impl std::error::Error for PatternError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(pattern: &str) -> PatternErrorKind {
        PathPattern::parse(pattern).expect_err(pattern).kind
    }

    fn names(pattern: &PathPattern) -> Vec<&str> {
        pattern.param_names().iter().map(|name| &**name).collect()
    }

    #[test]
    fn statics_params_and_catch_alls_parse() {
        let root = PathPattern::parse("/").unwrap();
        assert!(root.segments().is_empty());
        assert_eq!(root.shape(), "/");

        let pattern = PathPattern::parse("/api/v1/deals/{id}/notes/{note_id}").unwrap();
        assert_eq!(names(&pattern), ["id", "note_id"]);
        assert_eq!(pattern.shape(), "/api/v1/deals/{}/notes/{}");
        assert_eq!(pattern.as_str(), "/api/v1/deals/{id}/notes/{note_id}");

        let files = PathPattern::parse("/files/{*path}").unwrap();
        assert_eq!(
            files.segments(),
            [Segment::Static("files".into()), Segment::CatchAll]
        );
        assert_eq!(names(&files), ["path"]);

        for ok in [
            "/.well-known/openid-configuration",
            "/_navree/assets",
            "/a~b/c-d/e_f/g.h",
            "/x!$&'()*+,;=:@",
            "/{_}",
            "/{A1_b2}",
        ] {
            assert!(PathPattern::parse(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn shapes_ignore_parameter_names() {
        let a = PathPattern::parse("/deals/{id}/files/{*path}").unwrap();
        let b = PathPattern::parse("/deals/{deal}/files/{*rest}").unwrap();
        assert_eq!(a.shape(), b.shape());
        assert_ne!(a, b);
    }

    #[test]
    fn malformed_patterns_are_refused_with_a_reason() {
        assert_eq!(kind("deals"), PatternErrorKind::NotAbsolute);
        assert_eq!(kind(""), PatternErrorKind::NotAbsolute);
        assert_eq!(kind("/deals/"), PatternErrorKind::EmptySegment);
        assert_eq!(kind("//deals"), PatternErrorKind::EmptySegment);
        assert_eq!(kind("/a/./b"), PatternErrorKind::DotSegment);
        assert_eq!(kind("/a/.."), PatternErrorKind::DotSegment);
        assert_eq!(
            kind("/deals/:id"),
            PatternErrorKind::OldSyntax(":id".into())
        );
        assert_eq!(
            kind("/files/*path"),
            PatternErrorKind::OldSyntax("*path".into())
        );
        assert_eq!(
            kind("/api/{id"),
            PatternErrorKind::UnbalancedBraces("{id".into())
        );
        assert_eq!(kind("/v{n}"), PatternErrorKind::PartialParam("v{n}".into()));
        assert_eq!(
            kind("/{name}.json"),
            PatternErrorKind::PartialParam("{name}.json".into())
        );
        assert_eq!(kind("/{}"), PatternErrorKind::InvalidName("{}".into()));
        assert_eq!(kind("/{*}"), PatternErrorKind::InvalidName("{*}".into()));
        assert_eq!(kind("/{1a}"), PatternErrorKind::InvalidName("{1a}".into()));
        assert_eq!(
            kind("/{a-b}"),
            PatternErrorKind::InvalidName("{a-b}".into())
        );
        assert_eq!(
            kind(&format!("/{{{}}}", "a".repeat(65))),
            PatternErrorKind::InvalidName(format!("{{{}}}", "a".repeat(65)))
        );
        assert_eq!(
            kind("/{id}/x/{id}"),
            PatternErrorKind::DuplicateName("id".into())
        );
        assert_eq!(
            kind("/{*rest}/x"),
            PatternErrorKind::CatchAllNotLast("rest".into())
        );
        let many: String = (0..=MAX_PARAMS).map(|i| format!("/{{p{i}}}")).collect();
        assert_eq!(kind(&many), PatternErrorKind::TooManyParams);
        for (pattern, byte) in [
            ("/caf%C3%A9", '%'),
            ("/a\\b", '\\'),
            ("/a b", ' '),
            ("/a\"b", '"'),
            ("/é", '\u{c3}'),
        ] {
            assert!(
                matches!(kind(pattern), PatternErrorKind::InvalidChar(_, found) if found == byte),
                "{pattern}"
            );
        }
    }

    #[test]
    fn errors_name_the_pattern_and_the_fix() {
        let error = PathPattern::parse("/deals/:id").unwrap_err();
        assert_eq!(
            error.to_string(),
            "segment `:id` uses old capture syntax; write `{param}` or `{*rest}`"
        );
        let error = PathPattern::parse("/api/{id").unwrap_err();
        assert_eq!(error.to_string(), "segment `{id` has unbalanced braces");
        let error = PathPattern::parse("/{id}/{id}").unwrap_err();
        assert!(error.to_string().contains("`id` twice"), "{error}");
    }
}
