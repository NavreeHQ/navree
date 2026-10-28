//! The provider payloads: Amazon SES receipt notifications, Mailgun route
//! forms and Postmark inbound webhooks, read into [`Incoming`].

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::compose::Compose;
use super::error::InboundError;
use super::mail::{Envelope, Incoming, Verdict, Verdicts};

/// Where an SES notification's message is.
#[derive(Debug)]
pub(crate) enum SesContent {
    /// In the notification (an SNS action).
    Inline(Bytes),
    /// In a bucket (an S3 action).
    S3 {
        /// The bucket.
        bucket: String,
        /// The object key.
        key: String,
    },
}

/// An SES receipt notification (`notificationType: Received`), read.
#[derive(Debug)]
pub(crate) struct SesReceipt {
    pub(crate) content: SesContent,
    pub(crate) provider_id: Option<String>,
    pub(crate) envelope: Envelope,
    pub(crate) verdicts: Verdicts,
}

fn malformed(provider: &'static str, reason: impl Into<String>) -> InboundError {
    InboundError::Malformed {
        provider,
        reason: reason.into(),
    }
}

/// Reads an SES receipt notification (the SNS message, parsed as JSON).
pub(crate) fn ses(notification: &Value) -> Result<SesReceipt, InboundError> {
    let kind = notification["notificationType"]
        .as_str()
        .or_else(|| notification["eventType"].as_str())
        .unwrap_or_default();
    if kind != "Received" {
        return Err(malformed(
            "ses",
            format!("expected a `Received` notification, got `{kind}`"),
        ));
    }
    let mail = &notification["mail"];
    let receipt = &notification["receipt"];
    let strings = |value: &Value| -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    // The rule's recipients first: `mail.destination` also lists the
    // message's other recipients, outside your domains.
    let mut recipients = strings(&receipt["recipients"]);
    if recipients.is_empty() {
        recipients = strings(&mail["destination"]);
    }
    let verdict = |name: &str| receipt[name]["status"].as_str().and_then(Verdict::parse);
    let verdicts = Verdicts {
        spam: verdict("spamVerdict"),
        virus: verdict("virusVerdict"),
        spf: verdict("spfVerdict"),
        dkim: verdict("dkimVerdict"),
        dmarc: verdict("dmarcVerdict"),
    };
    let action = &receipt["action"];
    let content = match action["type"].as_str().unwrap_or_default() {
        "S3" => SesContent::S3 {
            bucket: action["bucketName"]
                .as_str()
                .ok_or_else(|| malformed("ses", "the S3 action has no `bucketName`"))?
                .to_owned(),
            key: action["objectKey"]
                .as_str()
                .ok_or_else(|| malformed("ses", "the S3 action has no `objectKey`"))?
                .to_owned(),
        },
        _ => {
            let content = notification["content"].as_str().ok_or_else(|| {
                malformed(
                    "ses",
                    "the notification has no `content` (use an SNS action, or an S3 action with `mail.inbound.ses.disk`)",
                )
            })?;
            let encoding = action["encoding"].as_str().unwrap_or("UTF8");
            let bytes = if encoding.eq_ignore_ascii_case("BASE64") {
                STANDARD
                    .decode(content.trim())
                    .map_err(|_| malformed("ses", "`content` is not base64"))?
            } else {
                content.as_bytes().to_vec()
            };
            SesContent::Inline(Bytes::from(bytes))
        }
    };
    Ok(SesReceipt {
        content,
        provider_id: mail["messageId"].as_str().map(str::to_owned),
        envelope: Envelope {
            sender: mail["source"].as_str().map(str::to_owned),
            recipients,
        },
        verdicts,
    })
}

/// A file posted with a Mailgun route.
#[derive(Clone, Debug)]
pub(crate) struct FormFile {
    /// The form field (`attachment-1`).
    pub(crate) field: String,
    /// The file name.
    pub(crate) filename: Option<String>,
    /// The media type.
    pub(crate) content_type: String,
    /// The content.
    pub(crate) content: Bytes,
}

/// The fields and files of a Mailgun route's POST.
#[derive(Clone, Debug, Default)]
pub(crate) struct MailgunForm {
    pub(crate) fields: Vec<(String, String)>,
    pub(crate) files: Vec<FormFile>,
}

impl MailgunForm {
    pub(crate) fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Reads a Mailgun route's POST: the whole message (`body-mime`, a route
/// URL ending in `mime`) or the parsed fields, rebuilt.
pub(crate) fn mailgun(form: &MailgunForm) -> Result<Incoming, InboundError> {
    let headers: Vec<(String, String)> = form
        .field("message-headers")
        .and_then(|json| serde_json::from_str::<Vec<(String, String)>>(json).ok())
        .unwrap_or_default();
    let header = |name: &str| {
        headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    let signal = |name: &str| form.field(name).or_else(|| header(name));
    let verdicts = Verdicts {
        spam: signal("X-Mailgun-Sflag").map(|flag| {
            if flag.trim().eq_ignore_ascii_case("yes") {
                Verdict::Fail
            } else {
                Verdict::Pass
            }
        }),
        spf: signal("X-Mailgun-Spf").and_then(Verdict::parse),
        dkim: signal("X-Mailgun-Dkim-Check-Result").and_then(Verdict::parse),
        ..Verdicts::default()
    };
    let recipients: Vec<String> = form
        .field("recipient")
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let envelope = Envelope {
        sender: form.field("sender").map(str::to_owned),
        recipients,
    };
    let provider_id = form
        .field("Message-Id")
        .or_else(|| header("Message-Id"))
        .map(|id| id.trim().trim_matches(['<', '>']).to_owned());

    if let Some(mime) = form.field("body-mime") {
        return Ok(Incoming {
            provider_id,
            envelope,
            verdicts,
            ..Incoming::new("mailgun", mime.to_owned())
        });
    }

    if form.field("from").is_none() && headers.is_empty() {
        return Err(malformed(
            "mailgun",
            "the form has neither `body-mime` nor the message fields",
        ));
    }
    let mut compose = Compose::new().source("mailgun");
    for (name, value) in &headers {
        compose = compose.raw_header(name, value);
    }
    if let Some(from) = form.field("from") {
        compose = compose.from(from);
    }
    if let Some(subject) = form.field("subject") {
        compose = compose.subject(subject);
    }
    if let Some(text) = form.field("body-plain") {
        compose = compose.text(text);
    }
    if let Some(html) = form.field("body-html") {
        compose = compose.html(html);
    }
    // `content-id-map`: {"<logo@example.com>": "attachment-1"}.
    let inline: Vec<(String, String)> = form
        .field("content-id-map")
        .and_then(|json| serde_json::from_str::<serde_json::Map<String, Value>>(json).ok())
        .map(|map| {
            map.into_iter()
                .filter_map(|(cid, field)| {
                    Some((
                        field.as_str()?.to_owned(),
                        cid.trim_matches(['<', '>']).to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    for file in &form.files {
        let name = file.filename.clone().unwrap_or_else(|| file.field.clone());
        match inline.iter().find(|(field, _)| *field == file.field) {
            Some((_, cid)) => {
                compose = compose.inline(
                    cid.clone(),
                    name,
                    file.content_type.clone(),
                    file.content.clone(),
                );
            }
            None => {
                compose = compose.attach(name, file.content_type.clone(), file.content.clone());
            }
        }
    }
    let mut incoming = compose.build();
    incoming.provider_id = provider_id;
    incoming.envelope = envelope;
    incoming.verdicts = verdicts;
    Ok(incoming)
}

impl Incoming {
    /// Reads a Postmark inbound webhook's JSON. The raw message is
    /// `RawEmail` when the stream includes it ("Include raw email content
    /// in JSON payload"); otherwise it is rebuilt from the fields, headers
    /// and attachments. The envelope recipient is `OriginalRecipient`.
    ///
    /// # Errors
    ///
    /// [`InboundError::Malformed`] when the payload is not Postmark's.
    pub fn from_postmark(payload: &Value) -> Result<Self, InboundError> {
        if !payload.is_object() || (payload["From"].is_null() && payload["RawEmail"].is_null()) {
            return Err(malformed(
                "postmark",
                "the payload is not a Postmark inbound message",
            ));
        }
        let headers: Vec<(String, String)> = payload["Headers"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some((
                            item["Name"].as_str()?.to_owned(),
                            item["Value"].as_str().unwrap_or_default().to_owned(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let header = |name: &str| {
            headers
                .iter()
                .find(|(header, _)| header.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        };
        let verdicts = Verdicts {
            spam: header("X-Spam-Status").map(|status| {
                if status.trim().to_ascii_lowercase().starts_with("yes") {
                    Verdict::Fail
                } else {
                    Verdict::Pass
                }
            }),
            spf: header("Received-SPF")
                .and_then(|value| value.split_whitespace().next())
                .and_then(Verdict::parse),
            ..Verdicts::default()
        };
        let emails = |name: &str| -> Vec<String> {
            payload[name]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item["Email"].as_str())
                        .filter(|email| !email.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let recipients = match payload["OriginalRecipient"]
            .as_str()
            .filter(|r| !r.is_empty())
        {
            Some(original) => vec![original.to_owned()],
            None => [emails("ToFull"), emails("CcFull"), emails("BccFull")].concat(),
        };
        let envelope = Envelope {
            sender: payload["FromFull"]["Email"]
                .as_str()
                .or_else(|| payload["From"].as_str())
                .map(str::to_owned),
            recipients,
        };
        let provider_id = payload["MessageID"].as_str().map(str::to_owned);

        if let Some(raw) = payload["RawEmail"].as_str().filter(|raw| !raw.is_empty()) {
            return Ok(Incoming {
                provider_id,
                envelope,
                verdicts,
                ..Incoming::new("postmark", raw.to_owned())
            });
        }

        let named = |item: &Value| -> Option<String> {
            let email = item["Email"].as_str().filter(|email| !email.is_empty())?;
            Some(
                match item["Name"].as_str().filter(|name| !name.is_empty()) {
                    Some(name) => format!("{name} <{email}>"),
                    None => email.to_owned(),
                },
            )
        };
        let mut compose = Compose::new().source("postmark");
        for (name, value) in &headers {
            compose = compose.raw_header(name, value);
        }
        if let Some(from) = named(&payload["FromFull"]).or_else(|| {
            payload["From"]
                .as_str()
                .filter(|f| !f.is_empty())
                .map(str::to_owned)
        }) {
            compose = compose.from(from);
        }
        for (list, cc) in [("ToFull", false), ("CcFull", true)] {
            for item in payload[list].as_array().into_iter().flatten() {
                if let Some(address) = named(item) {
                    compose = if cc {
                        compose.cc(address)
                    } else {
                        compose.to(address)
                    };
                }
            }
        }
        if let Some(reply_to) = payload["ReplyTo"].as_str().filter(|r| !r.is_empty()) {
            compose = compose.reply_to(reply_to);
        }
        if let Some(subject) = payload["Subject"].as_str() {
            compose = compose.subject(subject);
        }
        if let Some(date) = payload["Date"]
            .as_str()
            .and_then(|date| DateTime::parse_from_rfc2822(date.trim()).ok())
        {
            compose = compose.date(date.with_timezone(&Utc));
        }
        if let Some(text) = payload["TextBody"].as_str().filter(|t| !t.is_empty()) {
            compose = compose.text(text);
        }
        if let Some(html) = payload["HtmlBody"].as_str().filter(|h| !h.is_empty()) {
            compose = compose.html(html);
        }
        for attachment in payload["Attachments"].as_array().into_iter().flatten() {
            let content = STANDARD
                .decode(attachment["Content"].as_str().unwrap_or_default().trim())
                .map_err(|_| malformed("postmark", "an attachment's `Content` is not base64"))?;
            let name = attachment["Name"]
                .as_str()
                .unwrap_or("attachment")
                .to_owned();
            let content_type = attachment["ContentType"]
                .as_str()
                .unwrap_or("application/octet-stream")
                .to_owned();
            match attachment["ContentID"].as_str().filter(|id| !id.is_empty()) {
                Some(cid) => {
                    compose = compose.inline(
                        cid.trim_matches(['<', '>']).to_owned(),
                        name,
                        content_type,
                        content,
                    );
                }
                None => compose = compose.attach(name, content_type, content),
            }
        }
        let mut incoming = compose.build();
        incoming.provider_id = provider_id;
        incoming.envelope = envelope;
        incoming.verdicts = verdicts;
        Ok(incoming)
    }
}
