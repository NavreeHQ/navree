//! [`Attachment`]: a file sent with a message, and the name, type and size
//! rules every transport applies.

use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use lettre::message::header::ContentType;
use serde::{Deserialize, Serialize};

use crate::{MailError, Message};

/// The largest message, attachments base64-encoded, sent over SMTP: the
/// limit of the common mailbox providers (Gmail takes 25 MB).
pub const SMTP_MAX_MESSAGE_BYTES: usize = 25_000_000;
/// The largest message Amazon SES's v2 API takes (40 MB, encoded).
pub const SES_MAX_MESSAGE_BYTES: usize = 40_000_000;
/// The largest message Mailgun takes (25 MB).
pub const MAILGUN_MAX_MESSAGE_BYTES: usize = 25_000_000;
/// The largest message Postmark takes (10 MB, encoded).
pub const POSTMARK_MAX_MESSAGE_BYTES: usize = 10_000_000;
/// The largest message Resend takes (40 MB, encoded).
pub const RESEND_MAX_MESSAGE_BYTES: usize = 40_000_000;

/// File extensions no transport sends: the executables and scripts SES and
/// Postmark refuse, refused by every transport alike so that switching
/// transports never changes what can be sent.
pub const BLOCKED_EXTENSIONS: &[&str] = &[
    "ade", "adp", "app", "asp", "bas", "bat", "cer", "chm", "cmd", "com", "cpl", "crt", "csh",
    "der", "exe", "fxp", "gadget", "hlp", "hta", "inf", "ins", "isp", "its", "jar", "js", "jse",
    "ksh", "lib", "lnk", "mad", "maf", "mag", "mam", "maq", "mar", "mas", "mat", "mau", "mav",
    "maw", "mda", "mdb", "mde", "mdt", "mdw", "mdz", "msc", "msh", "msh1", "msh1xml", "msh2",
    "msh2xml", "mshxml", "msi", "msp", "mst", "ops", "pcd", "pif", "plg", "prf", "prg", "ps1",
    "ps1xml", "ps2", "ps2xml", "psc1", "psc2", "reg", "scf", "scr", "sct", "shb", "shs", "sys",
    "tmp", "url", "vb", "vba", "vbe", "vbs", "vps", "vsmacros", "vss", "vst", "vsw", "vxd", "ws",
    "wsc", "wsf", "wsh", "xnk",
];

/// A file sent with a message: a name, a MIME type and the bytes.
///
/// ```
/// use navree_mail::{Attachment, Message};
///
/// let pdf = b"%PDF-1.7 ...".to_vec();
/// let message = Message::new()
///     .to("Harpreet Kaur <harpreet@example.in>")
///     .subject("Invoice INV-2026-0042")
///     .text("Your invoice is attached.")
///     .attach("INV-2026-0042.pdf", "application/pdf", pdf);
/// assert_eq!(message.attachments[0].filename, "INV-2026-0042.pdf");
/// assert_eq!(Attachment::content_type_for("rates.csv"), "text/csv");
/// ```
///
/// It serializes with the content in base64, so a message with attachments
/// travels inside a queued job.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// The file name the recipient sees (`INV-2026-0042.pdf`).
    pub filename: String,
    /// The MIME type (`application/pdf`, `text/calendar; method=REQUEST`).
    pub content_type: String,
    /// The bytes.
    #[serde(with = "base64_content")]
    pub content: Vec<u8>,
}

impl std::fmt::Debug for Attachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attachment")
            .field("filename", &self.filename)
            .field("content_type", &self.content_type)
            .field("bytes", &self.content.len())
            .finish()
    }
}

impl Attachment {
    /// A file named `filename` of type `content_type` holding `content`.
    pub fn new(
        filename: impl Into<String>,
        content_type: impl Into<String>,
        content: impl Into<Vec<u8>>,
    ) -> Attachment {
        Attachment {
            filename: filename.into(),
            content_type: content_type.into(),
            content: content.into(),
        }
    }

    /// The file at `path`, named after it, its type guessed from its
    /// extension ([`content_type_for`](Self::content_type_for)). Reads the
    /// whole file; [`MailError::Attachment`] when it cannot be read.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Attachment, MailError> {
        let path = path.as_ref();
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| MailError::Attachment {
                filename: path.display().to_string(),
                reason: "the path has no file name in UTF-8",
            })?
            .to_owned();
        let content = std::fs::read(path).map_err(|_| MailError::Attachment {
            filename: filename.clone(),
            reason: "the file cannot be read",
        })?;
        Ok(Attachment {
            content_type: Self::content_type_for(&filename).to_owned(),
            filename,
            content,
        })
    }

    /// The MIME type of a file name's extension; `application/octet-stream`
    /// for one not in the list.
    pub fn content_type_for(filename: &str) -> &'static str {
        let extension = filename
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .unwrap_or_default();
        match extension.as_str() {
            "pdf" => "application/pdf",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            "txt" => "text/plain",
            "csv" => "text/csv",
            "html" | "htm" => "text/html",
            "ics" => "text/calendar",
            "json" => "application/json",
            "xml" => "application/xml",
            "zip" => "application/zip",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xls" => "application/vnd.ms-excel",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "ppt" => "application/vnd.ms-powerpoint",
            "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "odt" => "application/vnd.oasis.opendocument.text",
            "ods" => "application/vnd.oasis.opendocument.spreadsheet",
            "mp3" => "audio/mpeg",
            "mp4" => "video/mp4",
            _ => "application/octet-stream",
        }
    }

    /// The content in standard base64, as the API transports send it.
    pub(crate) fn base64(&self) -> String {
        STANDARD.encode(&self.content)
    }

    /// Checks the name and the type: a name of 1 to 255 bytes without line
    /// breaks, control characters or path separators, an extension that is
    /// not [blocked](BLOCKED_EXTENSIONS), and a `type/subtype` MIME type.
    pub fn check(&self) -> Result<(), MailError> {
        let refuse = |reason: &'static str| MailError::Attachment {
            filename: self.filename.clone(),
            reason,
        };
        let name = self.filename.as_str();
        if name.trim().is_empty() || name.len() > 255 {
            return Err(refuse("a file name is 1 to 255 bytes"));
        }
        if name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
        {
            return Err(refuse(
                "a file name has no line breaks, control characters or path separators",
            ));
        }
        if let Some((_, extension)) = name.rsplit_once('.')
            && BLOCKED_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        {
            return Err(refuse(
                "executable and script files are not sent (SES and Postmark refuse them)",
            ));
        }
        if self.content_type.contains(['\r', '\n'])
            || !self.content_type.contains('/')
            || ContentType::parse(&self.content_type).is_err()
        {
            return Err(refuse(
                "the content type must be a MIME type such as `application/pdf`",
            ));
        }
        Ok(())
    }
}

/// Base64 length of `bytes` bytes.
fn encoded_len(bytes: usize) -> usize {
    bytes.div_ceil(3) * 4
}

/// The size of `message` as the providers count it: the bodies plus every
/// attachment base64-encoded.
pub fn encoded_size(message: &Message) -> usize {
    let bodies = message.text.as_deref().map_or(0, str::len)
        + message.html.as_deref().map_or(0, str::len)
        + message.subject.len();
    let files: usize = message
        .attachments
        .iter()
        .map(|attachment| encoded_len(attachment.content.len()))
        .sum();
    bodies + files
}

/// [`MailError::TooLarge`] when `message` is over `transport`'s `limit`.
pub(crate) fn check_size(
    transport: &'static str,
    message: &Message,
    limit: usize,
) -> Result<(), MailError> {
    let size = encoded_size(message);
    if size > limit {
        return Err(MailError::TooLarge {
            transport,
            size,
            limit,
        });
    }
    Ok(())
}

/// Serde of attachment bytes as standard base64.
mod base64_content {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD
            .decode(text.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // CONN-102
    #[test]
    fn attachments_round_trip_through_a_queued_job_in_base64() {
        let attachment = Attachment::new(
            "rates.csv",
            "text/csv",
            b"city,rate\nLudhiana,18\n".to_vec(),
        );
        let json = serde_json::to_value(&attachment).unwrap();
        assert_eq!(json["content"], "Y2l0eSxyYXRlCkx1ZGhpYW5hLDE4Cg==");
        let back: Attachment = serde_json::from_value(json).unwrap();
        assert_eq!(back, attachment);
        assert!(
            !format!("{attachment:?}").contains("Ludhiana"),
            "Debug hides the bytes"
        );
    }

    // CONN-102
    #[test]
    fn names_and_types_are_checked() {
        let ok = |name: &str, kind: &str| Attachment::new(name, kind, vec![1]).check();
        assert!(ok("INV-2026-0042.pdf", "application/pdf").is_ok());
        assert!(ok("invite.ics", "text/calendar; method=REQUEST").is_ok());
        assert!(ok("ਰਸੀਦ.pdf", "application/pdf").is_ok(), "UTF-8 names");
        for (name, kind) in [
            ("", "application/pdf"),
            ("../etc/passwd", "text/plain"),
            ("a\\b.txt", "text/plain"),
            ("line\nbreak.txt", "text/plain"),
            ("setup.EXE", "application/octet-stream"),
            ("macro.vbs", "text/plain"),
            ("invoice.pdf", "pdf"),
            ("invoice.pdf", "application/pdf\r\nBcc: x@example.com"),
        ] {
            assert!(
                matches!(ok(name, kind), Err(MailError::Attachment { .. })),
                "{name:?} {kind:?}"
            );
        }
        assert!(ok(&format!("{}.pdf", "a".repeat(252)), "application/pdf").is_err());
    }

    // CONN-102
    #[test]
    fn types_follow_extensions() {
        assert_eq!(Attachment::content_type_for("a.PDF"), "application/pdf");
        assert_eq!(Attachment::content_type_for("invite.ics"), "text/calendar");
        assert_eq!(
            Attachment::content_type_for("sheet.xlsx"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(
            Attachment::content_type_for("noextension"),
            "application/octet-stream"
        );
    }

    // CONN-102
    #[test]
    fn sizes_count_attachments_base64_encoded() {
        assert_eq!(encoded_len(0), 0);
        assert_eq!(encoded_len(1), 4);
        assert_eq!(encoded_len(3), 4);
        assert_eq!(encoded_len(4), 8);
        let message = Message::new().subject("abc").text("1234").attach(
            "a.bin",
            "application/octet-stream",
            vec![0; 300],
        );
        assert_eq!(encoded_size(&message), 3 + 4 + 400);
        assert!(check_size("postmark", &message, 407).is_ok());
        assert!(matches!(
            check_size("postmark", &message, 406),
            Err(MailError::TooLarge {
                transport: "postmark",
                size: 407,
                limit: 406
            })
        ));
    }
}
