//! The `format` and `signed` templates of [`Hmac`](crate::Hmac) sources.

/// One piece of a template.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    /// Literal text.
    Text(String),
    /// `{name}`.
    Field(Field),
}

/// A placeholder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    /// `{timestamp}`: the signed Unix timestamp.
    Timestamp,
    /// `{signature}`: the encoded signature.
    Signature,
    /// `{id}`: the sender's delivery id.
    Id,
    /// `{body}`: the raw request body.
    Body,
}

/// Splits `template` into text and the placeholders in `allowed`.
fn tokenize(template: &str, allowed: &[(&str, Field)]) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut rest = template;
    while !rest.is_empty() {
        let Some(open) = rest.find('{') else {
            tokens.push(Token::Text(rest.to_owned()));
            break;
        };
        if open > 0 {
            tokens.push(Token::Text(rest[..open].to_owned()));
        }
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| format!("unclosed `{{` in `{template}`"))?;
        let name = &after[..close];
        let field = allowed
            .iter()
            .find(|(allowed, _)| *allowed == name)
            .map(|(_, field)| *field)
            .ok_or_else(|| {
                let names: Vec<String> = allowed.iter().map(|(n, _)| format!("{{{n}}}")).collect();
                format!(
                    "unknown placeholder `{{{name}}}` (use {})",
                    names.join(", ")
                )
            })?;
        tokens.push(Token::Field(field));
        rest = &after[close + 1..];
    }
    Ok(tokens)
}

/// The shape of a signature header value (`t={timestamp},v1={signature}`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SignatureFormat(Vec<Token>);

/// What [`SignatureFormat::extract`] found in a header value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Extracted {
    pub(crate) signature: String,
    pub(crate) timestamp: Option<String>,
}

impl SignatureFormat {
    /// Parses `format`: `{signature}` exactly once, `{timestamp}` at most
    /// once, and text between two placeholders.
    pub(crate) fn parse(format: &str) -> Result<Self, String> {
        let tokens = tokenize(
            format,
            &[
                ("signature", Field::Signature),
                ("timestamp", Field::Timestamp),
            ],
        )?;
        let count = |field| tokens.iter().filter(|t| **t == Token::Field(field)).count();
        if count(Field::Signature) != 1 {
            return Err("it must contain `{signature}` exactly once".to_owned());
        }
        if count(Field::Timestamp) > 1 {
            return Err("it may contain `{timestamp}` at most once".to_owned());
        }
        if tokens
            .windows(2)
            .any(|pair| matches!(pair, [Token::Field(_), Token::Field(_)]))
        {
            return Err("two placeholders must be separated by text".to_owned());
        }
        Ok(Self(tokens))
    }

    /// Whether the format carries the timestamp.
    #[cfg(test)]
    pub(crate) fn has_timestamp(&self) -> bool {
        self.0.contains(&Token::Field(Field::Timestamp))
    }

    /// Reads the placeholders out of `value`; `None` when it does not have
    /// the format's shape. A placeholder runs up to the first occurrence of
    /// the text after it (or to the end).
    pub(crate) fn extract(&self, value: &str) -> Option<Extracted> {
        let mut rest = value;
        let mut signature = None;
        let mut timestamp = None;
        let mut tokens = self.0.iter().peekable();
        while let Some(token) = tokens.next() {
            match token {
                Token::Text(text) => rest = rest.strip_prefix(text.as_str())?,
                Token::Field(field) => {
                    let end = match tokens.peek() {
                        Some(Token::Text(next)) => rest.find(next.as_str())?,
                        _ => rest.len(),
                    };
                    let captured = rest[..end].to_owned();
                    rest = &rest[end..];
                    match field {
                        Field::Signature => signature = Some(captured),
                        Field::Timestamp => timestamp = Some(captured),
                        Field::Id | Field::Body => return None,
                    }
                }
            }
        }
        if !rest.is_empty() {
            return None;
        }
        Some(Extracted {
            signature: signature?,
            timestamp,
        })
    }
}

/// What is signed (`{timestamp}.{body}`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SignedTemplate(Vec<Token>);

impl SignedTemplate {
    /// Parses `signed`: text and `{timestamp}`, `{id}` and `{body}`.
    pub(crate) fn parse(signed: &str) -> Result<Self, String> {
        tokenize(
            signed,
            &[
                ("timestamp", Field::Timestamp),
                ("id", Field::Id),
                ("body", Field::Body),
            ],
        )
        .map(Self)
    }

    /// Whether the template uses `field`.
    pub(crate) fn uses(&self, field: Field) -> bool {
        self.0.contains(&Token::Field(field))
    }

    /// The bytes to sign.
    pub(crate) fn render(&self, timestamp: &str, id: &str, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len() + 64);
        for token in &self.0 {
            match token {
                Token::Text(text) => out.extend_from_slice(text.as_bytes()),
                Token::Field(Field::Timestamp) => out.extend_from_slice(timestamp.as_bytes()),
                Token::Field(Field::Id) => out.extend_from_slice(id.as_bytes()),
                Token::Field(Field::Body) => out.extend_from_slice(body),
                Token::Field(Field::Signature) => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_extract_the_signature_and_timestamp() {
        let format = SignatureFormat::parse("t={timestamp},v1={signature}").unwrap();
        assert!(format.has_timestamp());
        assert_eq!(
            format.extract("t=1792000000,v1=abc123"),
            Some(Extracted {
                signature: "abc123".to_owned(),
                timestamp: Some("1792000000".to_owned()),
            })
        );
        assert_eq!(format.extract("v1=abc123"), None);
        assert_eq!(format.extract("t=1,v2=abc"), None);

        let prefixed = SignatureFormat::parse("sha256={signature}").unwrap();
        assert!(!prefixed.has_timestamp());
        assert_eq!(prefixed.extract("sha256=ff").unwrap().signature, "ff");
        assert_eq!(prefixed.extract("sha1=ff"), None);

        let bare = SignatureFormat::parse("{signature}").unwrap();
        assert_eq!(
            bare.extract("anything=goes").unwrap().signature,
            "anything=goes"
        );
    }

    #[test]
    fn invalid_formats_are_refused() {
        assert!(SignatureFormat::parse("t={timestamp}").is_err());
        assert!(SignatureFormat::parse("{signature}{timestamp}").is_err());
        assert!(SignatureFormat::parse("{signature},{signature}").is_err());
        assert!(SignatureFormat::parse("{signature").is_err());
        assert!(SignatureFormat::parse("{body}={signature}").is_err());
    }

    #[test]
    fn signed_templates_render_their_fields() {
        let signed = SignedTemplate::parse("v0:{timestamp}:{body}").unwrap();
        assert!(signed.uses(Field::Timestamp));
        assert!(!signed.uses(Field::Id));
        assert_eq!(signed.render("17", "id", b"{}"), b"v0:17:{}".to_vec());
        let with_id = SignedTemplate::parse("{id}.{timestamp}.{body}").unwrap();
        assert_eq!(with_id.render("17", "msg_1", b"x"), b"msg_1.17.x".to_vec());
        assert!(SignedTemplate::parse("{signature}").is_err());
    }
}
