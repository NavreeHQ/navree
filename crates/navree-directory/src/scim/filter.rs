//! SCIM filters and attribute paths (RFC 7644 §3.4.2.2 and §3.5.2):
//! [`Filter::parse`], [`PatchPath::parse`], and evaluation against a
//! resource's JSON.
//!
//! Attribute names are matched without regard to case. Strings compare
//! without regard to case too, except `id`, `externalId` and `$ref`, which
//! are case-exact (RFC 7643 §2.2 and the core schemas).

use std::cmp::Ordering;
use std::fmt;

use serde_json::Value;

/// The schema URN of the core user attributes; a path that names it is a
/// path to a top-level attribute.
pub(crate) const USER_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
/// The schema URN of the core group attributes.
pub(crate) const GROUP_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";
/// The schema URN of the enterprise user extension (RFC 7643 §4.3).
pub(crate) const ENTERPRISE_USER_SCHEMA: &str =
    "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User";

/// When `path` names an extension schema itself (its URN alone, which
/// parses as a URN and a last segment), that schema's URN.
pub(crate) fn extension_schema(path: &AttrPath) -> Option<&'static str> {
    let urn = path.urn.as_deref()?;
    if path.sub.is_some() {
        return None;
    }
    let whole = format!("{urn}:{}", path.name);
    whole
        .eq_ignore_ascii_case(ENTERPRISE_USER_SCHEMA)
        .then_some(ENTERPRISE_USER_SCHEMA)
}

/// A filter or path that does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterError(pub String);

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FilterError {}

/// An attribute path: an optional schema URN, an attribute and an optional
/// sub-attribute (`name.givenName`,
/// `urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:employeeNumber`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrPath {
    /// The schema URN, when the path names one (core URNs are dropped).
    pub urn: Option<String>,
    /// The attribute.
    pub name: String,
    /// The sub-attribute.
    pub sub: Option<String>,
}

impl AttrPath {
    /// A path to a top-level attribute.
    #[cfg(test)]
    pub(crate) fn top(name: &str) -> Self {
        Self {
            urn: None,
            name: name.to_owned(),
            sub: None,
        }
    }

    fn parse(text: &str) -> Result<AttrPath, FilterError> {
        if text.is_empty() {
            return Err(FilterError("an attribute path is empty".to_owned()));
        }
        // A URN ends at its last colon: `urn:...:2.0:User:employeeNumber`.
        let (urn, rest) = if text.len() > 4 && text[..4].eq_ignore_ascii_case("urn:") {
            match text.rfind(':') {
                Some(at) => (Some(&text[..at]), &text[at + 1..]),
                None => (None, text),
            }
        } else {
            (None, text)
        };
        let (name, sub) = match rest.split_once('.') {
            Some((name, sub)) => (name, Some(sub)),
            None => (rest, None),
        };
        if !is_attr_name(name) || sub.is_some_and(|sub| !is_attr_name(sub)) {
            return Err(FilterError(format!("`{text}` is not an attribute path")));
        }
        let urn = urn
            .filter(|urn| {
                !urn.eq_ignore_ascii_case(USER_SCHEMA) && !urn.eq_ignore_ascii_case(GROUP_SCHEMA)
            })
            .map(str::to_owned);
        Ok(AttrPath {
            urn,
            name: name.to_owned(),
            sub: sub.map(str::to_owned),
        })
    }

    /// Whether the attribute compares case-exact.
    fn case_exact(&self) -> bool {
        let leaf = self.sub.as_deref().unwrap_or(&self.name);
        self.urn.is_none()
            && (leaf.eq_ignore_ascii_case("id")
                || leaf.eq_ignore_ascii_case("externalId")
                || leaf == "$ref")
            && (self.sub.is_none() || leaf == "$ref")
    }

    /// The values at this path in `resource`: a multi-valued attribute
    /// gives one value per element (its `sub`-attribute, or its `value`
    /// sub-attribute when the path names none).
    fn values<'a>(&self, resource: &'a Value) -> Vec<&'a Value> {
        let Some(container) = container(resource, self.urn.as_deref()) else {
            return Vec::new();
        };
        let Some(value) = get_ci(container, &self.name) else {
            return Vec::new();
        };
        match (value, self.sub.as_deref()) {
            (Value::Array(items), Some(sub)) => {
                items.iter().filter_map(|item| get_ci(item, sub)).collect()
            }
            (Value::Array(items), None) => items
                .iter()
                .map(|item| match item {
                    Value::Object(_) => get_ci(item, "value").unwrap_or(&Value::Null),
                    other => other,
                })
                .collect(),
            (object @ Value::Object(_), Some(sub)) => get_ci(object, sub).into_iter().collect(),
            (value, None) => vec![value],
            (_, Some(_)) => Vec::new(),
        }
    }
}

impl fmt::Display for AttrPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(urn) = &self.urn {
            write!(f, "{urn}:")?;
        }
        f.write_str(&self.name)?;
        if let Some(sub) = &self.sub {
            write!(f, ".{sub}")?;
        }
        Ok(())
    }
}

/// The object an attribute of schema `urn` lives in: the resource itself,
/// or its extension object.
pub(crate) fn container<'a>(resource: &'a Value, urn: Option<&str>) -> Option<&'a Value> {
    match urn {
        None => Some(resource),
        Some(urn) => get_ci(resource, urn),
    }
}

/// `object[key]`, matching the key without regard to case.
pub(crate) fn get_ci<'a>(object: &'a Value, key: &str) -> Option<&'a Value> {
    let map = object.as_object()?;
    map.get(key).or_else(|| {
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    })
}

fn is_attr_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A comparison operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareOp {
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Contains.
    Co,
    /// Starts with.
    Sw,
    /// Ends with.
    Ew,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Ge,
    /// Less than.
    Lt,
    /// Less than or equal.
    Le,
}

impl CompareOp {
    fn parse(word: &str) -> Option<CompareOp> {
        Some(match word.to_ascii_lowercase().as_str() {
            "eq" => CompareOp::Eq,
            "ne" => CompareOp::Ne,
            "co" => CompareOp::Co,
            "sw" => CompareOp::Sw,
            "ew" => CompareOp::Ew,
            "gt" => CompareOp::Gt,
            "ge" => CompareOp::Ge,
            "lt" => CompareOp::Lt,
            "le" => CompareOp::Le,
            _ => return None,
        })
    }
}

/// A parsed SCIM filter.
#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    /// `attr op value`.
    Compare {
        /// The attribute.
        path: AttrPath,
        /// The operator.
        op: CompareOp,
        /// The value (a string, number, boolean or null).
        value: Value,
    },
    /// `attr pr`.
    Present(AttrPath),
    /// `a and b`.
    And(Box<Filter>, Box<Filter>),
    /// `a or b`.
    Or(Box<Filter>, Box<Filter>),
    /// `not (a)`.
    Not(Box<Filter>),
    /// `attr[filter]`: some element of multi-valued `attr` matches.
    ValuePath {
        /// The multi-valued attribute.
        path: AttrPath,
        /// The filter its elements are matched with.
        filter: Box<Filter>,
    },
}

impl Filter {
    /// Parses a filter (`userName eq "ada@acme.test"`).
    pub fn parse(text: &str) -> Result<Filter, FilterError> {
        let tokens = tokenize(text)?;
        let mut parser = Parser { tokens, at: 0 };
        let filter = parser.or_expr()?;
        if parser.at != parser.tokens.len() {
            return Err(FilterError(format!(
                "unexpected `{}` in the filter",
                parser.tokens[parser.at]
            )));
        }
        Ok(filter)
    }

    /// Whether `resource` (a SCIM resource's JSON) matches.
    pub fn matches(&self, resource: &Value) -> bool {
        match self {
            Filter::Compare { path, op, value } => path
                .values(resource)
                .into_iter()
                .any(|actual| compare(actual, *op, value, path.case_exact())),
            Filter::Present(path) => path.values(resource).into_iter().any(is_present),
            Filter::And(a, b) => a.matches(resource) && b.matches(resource),
            Filter::Or(a, b) => a.matches(resource) || b.matches(resource),
            Filter::Not(inner) => !inner.matches(resource),
            Filter::ValuePath { path, filter } => {
                let Some(container) = container(resource, path.urn.as_deref()) else {
                    return false;
                };
                match get_ci(container, &path.name) {
                    Some(Value::Array(items)) => items.iter().any(|item| filter.matches(item)),
                    Some(item @ Value::Object(_)) => filter.matches(item),
                    _ => false,
                }
            }
        }
    }

    /// `attr eq "value"` on a top-level attribute: the attribute's name and
    /// the value (what an indexed lookup can answer).
    pub fn simple_eq(&self) -> Option<(&str, &str)> {
        match self {
            Filter::Compare {
                path:
                    AttrPath {
                        urn: None,
                        name,
                        sub: None,
                    },
                op: CompareOp::Eq,
                value: Value::String(value),
            } => Some((name.as_str(), value.as_str())),
            _ => None,
        }
    }
}

fn is_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => true,
    }
}

fn compare(actual: &Value, op: CompareOp, expected: &Value, case_exact: bool) -> bool {
    match (actual, expected) {
        (Value::String(actual), Value::String(expected)) => {
            let (a, e) = if case_exact {
                (actual.clone(), expected.clone())
            } else {
                (actual.to_lowercase(), expected.to_lowercase())
            };
            match op {
                CompareOp::Eq => a == e,
                CompareOp::Ne => a != e,
                CompareOp::Co => a.contains(&e),
                CompareOp::Sw => a.starts_with(&e),
                CompareOp::Ew => a.ends_with(&e),
                CompareOp::Gt => a > e,
                CompareOp::Ge => a >= e,
                CompareOp::Lt => a < e,
                CompareOp::Le => a <= e,
            }
        }
        (Value::Number(actual), Value::Number(expected)) => {
            let (Some(a), Some(e)) = (actual.as_f64(), expected.as_f64()) else {
                return false;
            };
            let order = a.partial_cmp(&e);
            match op {
                CompareOp::Eq => order == Some(Ordering::Equal),
                CompareOp::Ne => order != Some(Ordering::Equal),
                CompareOp::Gt => order == Some(Ordering::Greater),
                CompareOp::Ge => matches!(order, Some(Ordering::Greater | Ordering::Equal)),
                CompareOp::Lt => order == Some(Ordering::Less),
                CompareOp::Le => matches!(order, Some(Ordering::Less | Ordering::Equal)),
                CompareOp::Co | CompareOp::Sw | CompareOp::Ew => false,
            }
        }
        (Value::Bool(actual), Value::Bool(expected)) => match op {
            CompareOp::Eq => actual == expected,
            CompareOp::Ne => actual != expected,
            _ => false,
        },
        // Booleans sent as strings ("True") compare as booleans.
        (Value::Bool(actual), Value::String(expected)) => {
            match expected.to_ascii_lowercase().as_str() {
                "true" | "false" => {
                    let expected = expected.eq_ignore_ascii_case("true");
                    match op {
                        CompareOp::Eq => *actual == expected,
                        CompareOp::Ne => *actual != expected,
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        (Value::Null, Value::Null) => op == CompareOp::Eq,
        (_, Value::Null) => op == CompareOp::Ne,
        _ => op == CompareOp::Ne,
    }
}

/// A PATCH path (RFC 7644 §3.5.2): an attribute path, or a value filter on
/// a multi-valued attribute with an optional sub-attribute
/// (`emails[type eq "work"].value`, `members[value eq "2819c223"]`).
#[derive(Clone, Debug, PartialEq)]
pub struct PatchPath {
    /// The attribute (with its sub-attribute for plain paths).
    pub attr: AttrPath,
    /// The value filter, for `attr[filter]` paths.
    pub filter: Option<Filter>,
    /// The sub-attribute after the filter (`.value`).
    pub sub: Option<String>,
}

impl PatchPath {
    /// Parses a PATCH path.
    pub fn parse(text: &str) -> Result<PatchPath, FilterError> {
        let text = text.trim();
        let Some(open) = text.find('[') else {
            return Ok(PatchPath {
                attr: AttrPath::parse(text)?,
                filter: None,
                sub: None,
            });
        };
        let close = text
            .rfind(']')
            .filter(|close| *close > open)
            .ok_or_else(|| FilterError(format!("`{text}` has an unclosed `[`")))?;
        let attr = AttrPath::parse(&text[..open])?;
        if attr.sub.is_some() {
            return Err(FilterError(format!(
                "`{text}`: a value filter applies to an attribute, not a sub-attribute"
            )));
        }
        let filter = Filter::parse(&text[open + 1..close])?;
        let rest = &text[close + 1..];
        let sub = if rest.is_empty() {
            None
        } else {
            let sub = rest
                .strip_prefix('.')
                .filter(|sub| is_attr_name(sub))
                .ok_or_else(|| FilterError(format!("`{text}` is not a PATCH path")))?;
            Some(sub.to_owned())
        };
        Ok(PatchPath {
            attr,
            filter: Some(filter),
            sub,
        })
    }
}

// -- tokens -----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Value(Value),
    Open,
    Close,
    OpenBracket,
    CloseBracket,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Word(word) => f.write_str(word),
            Token::Value(value) => write!(f, "{value}"),
            Token::Open => f.write_str("("),
            Token::Close => f.write_str(")"),
            Token::OpenBracket => f.write_str("["),
            Token::CloseBracket => f.write_str("]"),
        }
    }
}

fn tokenize(text: &str) -> Result<Vec<Token>, FilterError> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                tokens.push(Token::Open);
                i += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                i += 1;
            }
            '[' => {
                tokens.push(Token::OpenBracket);
                i += 1;
            }
            ']' => {
                tokens.push(Token::CloseBracket);
                i += 1;
            }
            '"' => {
                // A JSON string: find its closing quote, honouring escapes.
                let start = i;
                i += 1;
                let mut escaped = false;
                while i < chars.len() {
                    match chars[i] {
                        '\\' if !escaped => escaped = true,
                        '"' if !escaped => break,
                        _ => escaped = false,
                    }
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(FilterError(
                        "a string in the filter is not closed".to_owned(),
                    ));
                }
                let literal: String = chars[start..=i].iter().collect();
                let value: String = serde_json::from_str(&literal)
                    .map_err(|_| FilterError(format!("{literal} is not a valid string")))?;
                tokens.push(Token::Value(Value::String(value)));
                i += 1;
            }
            _ => {
                let start = i;
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !matches!(chars[i], '(' | ')' | '[' | ']' | '"')
                {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                tokens.push(word_token(word));
            }
        }
    }
    Ok(tokens)
}

fn word_token(word: String) -> Token {
    match word.to_ascii_lowercase().as_str() {
        "true" => Token::Value(Value::Bool(true)),
        "false" => Token::Value(Value::Bool(false)),
        "null" => Token::Value(Value::Null),
        _ => match serde_json::from_str::<serde_json::Number>(&word) {
            Ok(number) if word.starts_with(|c: char| c.is_ascii_digit() || c == '-') => {
                Token::Value(Value::Number(number))
            }
            _ => Token::Word(word),
        },
    }
}

// -- parser -----------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at += 1;
        token
    }

    fn keyword(&self, keyword: &str) -> bool {
        matches!(self.peek(), Some(Token::Word(word)) if word.eq_ignore_ascii_case(keyword))
    }

    fn or_expr(&mut self) -> Result<Filter, FilterError> {
        let mut left = self.and_expr()?;
        while self.keyword("or") {
            self.at += 1;
            let right = self.and_expr()?;
            left = Filter::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Filter, FilterError> {
        let mut left = self.unary()?;
        while self.keyword("and") {
            self.at += 1;
            let right = self.unary()?;
            left = Filter::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Filter, FilterError> {
        if self.keyword("not") {
            self.at += 1;
            if self.next() != Some(Token::Open) {
                return Err(FilterError("`not` must be followed by `(`".to_owned()));
            }
            let inner = self.or_expr()?;
            self.expect(&Token::Close)?;
            return Ok(Filter::Not(Box::new(inner)));
        }
        if self.peek() == Some(&Token::Open) {
            self.at += 1;
            let inner = self.or_expr()?;
            self.expect(&Token::Close)?;
            return Ok(inner);
        }
        self.attr_expr()
    }

    fn expect(&mut self, token: &Token) -> Result<(), FilterError> {
        match self.next() {
            Some(found) if &found == token => Ok(()),
            Some(found) => Err(FilterError(format!(
                "expected `{token}` in the filter, found `{found}`"
            ))),
            None => Err(FilterError(format!(
                "expected `{token}` at the end of the filter"
            ))),
        }
    }

    fn attr_expr(&mut self) -> Result<Filter, FilterError> {
        let path = match self.next() {
            Some(Token::Word(word)) => AttrPath::parse(&word)?,
            Some(other) => {
                return Err(FilterError(format!(
                    "expected an attribute in the filter, found `{other}`"
                )));
            }
            None => return Err(FilterError("the filter is empty".to_owned())),
        };
        if self.peek() == Some(&Token::OpenBracket) {
            self.at += 1;
            let inner = self.or_expr()?;
            self.expect(&Token::CloseBracket)?;
            return Ok(Filter::ValuePath {
                path,
                filter: Box::new(inner),
            });
        }
        let op = match self.next() {
            Some(Token::Word(word)) if word.eq_ignore_ascii_case("pr") => {
                return Ok(Filter::Present(path));
            }
            Some(Token::Word(word)) => CompareOp::parse(&word)
                .ok_or_else(|| FilterError(format!("`{word}` is not a filter operator")))?,
            Some(other) => {
                return Err(FilterError(format!(
                    "expected an operator after `{path}`, found `{other}`"
                )));
            }
            None => {
                return Err(FilterError(format!("expected an operator after `{path}`")));
            }
        };
        let value = match self.next() {
            Some(Token::Value(value)) => value,
            Some(other) => {
                return Err(FilterError(format!(
                    "expected a value after `{path}`, found `{other}`"
                )));
            }
            None => return Err(FilterError(format!("expected a value after `{path}`"))),
        };
        Ok(Filter::Compare { path, op, value })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn user() -> Value {
        json!({
            "schemas": [USER_SCHEMA],
            "id": "2819c223-7f76-453a-919d-413861904646",
            "externalId": "Ada-1",
            "userName": "Ada@Acme.test",
            "active": true,
            "name": { "givenName": "Ada", "familyName": "Lovelace" },
            "title": "Engineer",
            "emails": [
                { "value": "ada@acme.test", "type": "work", "primary": true },
                { "value": "ada@home.test", "type": "home" }
            ],
            "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User": {
                "employeeNumber": "701984",
                "manager": { "value": "26118915-6090-4610-87e4-49d8ca9f808d" }
            },
            "meta": { "lastModified": "2026-10-27T08:00:00Z", "resourceType": "User" },
            "loginCount": 12
        })
    }

    fn matches(filter: &str) -> bool {
        Filter::parse(filter).unwrap().matches(&user())
    }

    #[test]
    fn the_rfc_examples_parse() {
        for filter in [
            r#"userName eq "bjensen""#,
            r#"name.familyName co "O'Malley""#,
            r#"userName sw "J""#,
            r#"urn:ietf:params:scim:schemas:core:2.0:User:userName sw "J""#,
            "title pr",
            r#"meta.lastModified gt "2011-05-13T04:42:34Z""#,
            r#"title pr and userType eq "Employee""#,
            r#"title pr or userType eq "Intern""#,
            r#"schemas eq "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User""#,
            r#"userType eq "Employee" and (emails co "example.com" or emails.value co "example.org")"#,
            r#"userType ne "Employee" and not (emails co "example.com" or emails.value co "example.org")"#,
            r#"userType eq "Employee" and (emails.type eq "work")"#,
            r#"userType eq "Employee" and emails[type eq "work" and value co "@example.com"]"#,
            r#"emails[type eq "work" and value co "@example.com"] or ims[type eq "xmpp" and value co "@foo.com"]"#,
        ] {
            Filter::parse(filter).unwrap_or_else(|e| panic!("{filter}: {e}"));
        }
    }

    #[test]
    fn comparisons_ignore_case_except_for_ids() {
        assert!(matches(r#"userName eq "ada@acme.test""#));
        assert!(matches(r#"USERNAME EQ "ADA@ACME.TEST""#));
        assert!(matches(r#"externalId eq "Ada-1""#));
        assert!(!matches(r#"externalId eq "ada-1""#));
        assert!(!matches(r#"id eq "2819C223-7F76-453A-919D-413861904646""#));
        assert!(matches(r#"name.familyName sw "love""#));
        assert!(matches(r#"title ew "neer""#));
        assert!(matches(r#"title co "gin""#));
        assert!(matches(r#"title ne "Manager""#));
        assert!(matches(r#"meta.lastModified gt "2026-01-01T00:00:00Z""#));
        assert!(!matches(r#"meta.lastModified lt "2026-01-01T00:00:00Z""#));
        assert!(matches("loginCount ge 12"));
        assert!(!matches("loginCount gt 12"));
        assert!(matches("active eq true"));
        assert!(matches(r#"active eq "True""#));
        assert!(!matches("active eq false"));
    }

    #[test]
    fn multi_valued_attributes_match_any_element() {
        assert!(matches(r#"emails co "home.test""#));
        assert!(matches(r#"emails.value eq "ada@home.test""#));
        assert!(matches(r#"emails.type eq "work""#));
        assert!(matches(r#"emails[type eq "work" and value co "acme"]"#));
        assert!(!matches(r#"emails[type eq "home" and value co "acme"]"#));
        assert!(matches(r#"emails[primary eq true]"#));
    }

    #[test]
    fn logic_follows_precedence_and_parentheses() {
        // `and` binds tighter than `or`.
        assert!(matches(r#"title eq "x" and title eq "y" or userName pr"#));
        assert!(!matches(
            r#"title eq "x" and (title eq "y" or userName pr)"#
        ));
        assert!(matches(r#"not (title eq "Manager")"#));
        assert!(!matches(r#"not (title pr)"#));
        assert!(matches(r#"(title pr) and ((userName pr))"#));
    }

    #[test]
    fn presence_needs_a_non_empty_value() {
        assert!(matches("title pr"));
        assert!(matches("emails pr"));
        assert!(!matches("nickName pr"));
        let mut user = user();
        user["nickName"] = json!("");
        assert!(!Filter::parse("nickName pr").unwrap().matches(&user));
    }

    #[test]
    fn extension_attributes_are_reached_through_their_urn() {
        assert!(matches(
            r#"urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:employeeNumber eq "701984""#
        ));
        assert!(matches(
            r#"urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:manager.value pr"#
        ));
        assert!(matches(
            r#"urn:ietf:params:scim:schemas:core:2.0:User:userName sw "ada""#
        ));
    }

    #[test]
    fn simple_equality_is_recognized() {
        let filter = Filter::parse(r#"userName eq "ada@acme.test""#).unwrap();
        assert_eq!(filter.simple_eq(), Some(("userName", "ada@acme.test")));
        assert_eq!(
            Filter::parse(r#"displayName eq "Engineering""#)
                .unwrap()
                .simple_eq(),
            Some(("displayName", "Engineering"))
        );
        assert_eq!(
            Filter::parse(r#"userName co "ada""#).unwrap().simple_eq(),
            None
        );
        assert_eq!(
            Filter::parse(r#"name.givenName eq "Ada""#)
                .unwrap()
                .simple_eq(),
            None
        );
    }

    #[test]
    fn strings_keep_their_escapes() {
        let filter = Filter::parse(r#"title eq "say \"hi\" \\ there""#).unwrap();
        assert_eq!(
            filter,
            Filter::Compare {
                path: AttrPath::top("title"),
                op: CompareOp::Eq,
                value: json!("say \"hi\" \\ there"),
            }
        );
    }

    #[test]
    fn bad_filters_say_what_is_wrong() {
        for (filter, message) in [
            ("", "the filter is empty"),
            ("userName", "expected an operator after `userName`"),
            ("userName eq", "expected a value after `userName`"),
            (r#"userName is "x""#, "`is` is not a filter operator"),
            (r#"userName eq "x"#, "a string in the filter is not closed"),
            (
                r#"(userName eq "x""#,
                "expected `)` at the end of the filter",
            ),
            (r#"userName eq "x" junk"#, "unexpected `junk` in the filter"),
            (r#"not userName eq "x""#, "`not` must be followed by `(`"),
            (
                r#"emails[type eq "work""#,
                "expected `]` at the end of the filter",
            ),
            (r#"1bad eq "x""#, "`1bad` is not an attribute path"),
            (
                r#""x" eq "x""#,
                "expected an attribute in the filter, found `\"x\"`",
            ),
            (
                r#"user..name eq "x""#,
                "`user..name` is not an attribute path",
            ),
        ] {
            let error = Filter::parse(filter).unwrap_err();
            assert_eq!(error.0, message, "{filter}");
        }
    }

    #[test]
    fn patch_paths_parse() {
        let path = PatchPath::parse("members").unwrap();
        assert_eq!(path.attr, AttrPath::top("members"));
        assert!(path.filter.is_none());

        let path = PatchPath::parse("name.givenName").unwrap();
        assert_eq!(path.attr.sub.as_deref(), Some("givenName"));

        let path = PatchPath::parse(r#"emails[type eq "work"].value"#).unwrap();
        assert_eq!(path.attr, AttrPath::top("emails"));
        assert_eq!(path.sub.as_deref(), Some("value"));
        assert!(
            path.filter
                .as_ref()
                .unwrap()
                .matches(&json!({ "type": "work" }))
        );

        let path = PatchPath::parse(r#"members[value eq "2819c223"]"#).unwrap();
        assert_eq!(path.attr.name, "members");
        assert!(path.sub.is_none());

        let path =
            PatchPath::parse("urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:manager")
                .unwrap();
        assert_eq!(
            path.attr.urn.as_deref(),
            Some("urn:ietf:params:scim:schemas:extension:enterprise:2.0:User")
        );
        assert_eq!(path.attr.name, "manager");

        let path = PatchPath::parse("urn:ietf:params:scim:schemas:core:2.0:User:userName").unwrap();
        assert_eq!(path.attr, AttrPath::top("userName"));

        assert!(PatchPath::parse(r#"emails[type eq "work""#).is_err());
        assert!(PatchPath::parse(r#"emails[type eq "work"]value"#).is_err());
        assert!(PatchPath::parse(r#"name.givenName[x eq "y"]"#).is_err());
        assert!(PatchPath::parse("").is_err());
    }
}
