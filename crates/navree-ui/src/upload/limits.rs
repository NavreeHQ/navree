//! What an upload accepts: the size of a file, how many files, and which kinds.

use super::labels::{UploadLabels, fill};

/// The most files one upload may allow.
const MOST_FILES: usize = 100;

/// What an upload accepts. The page shows these as hints and the server holds every file
/// to them, so a browser that ignores the hints, or is not a browser, gets the same answer.
///
/// ```
/// use navree_ui::UploadLimits;
///
/// let limits = UploadLimits::new(2 * 1024 * 1024)
///     .max_files(5)
///     .accept(["image/png", "image/jpeg"]);
/// assert!(limits.allows("image/png"));
/// assert!(!limits.allows("application/pdf"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadLimits {
    bytes: u64,
    files: usize,
    types: Vec<String>,
}

impl Default for UploadLimits {
    /// Ten megabytes, one file, any kind.
    fn default() -> Self {
        Self::new(10 * 1024 * 1024)
    }
}

impl UploadLimits {
    /// Files of at most `max_bytes` bytes (at least one), one file, any kind.
    pub fn new(max_bytes: u64) -> Self {
        Self {
            bytes: max_bytes.max(1),
            files: 1,
            types: Vec::new(),
        }
    }

    /// Allows `files` files in one send: at least one, at most 100.
    pub fn max_files(mut self, files: usize) -> Self {
        self.files = files.clamp(1, MOST_FILES);
        self
    }

    /// Adds the kinds of file that are accepted: MIME types such as `image/png` and
    /// wildcards such as `image/*`. An entry that is neither is dropped. With none, any
    /// kind is accepted. The kind is read from the file's first bytes, never from its name
    /// or from what the client says it is.
    pub fn accept(mut self, types: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        for kind in types {
            if let Some(kind) = clean_type(kind.as_ref())
                && !self.types.contains(&kind)
            {
                self.types.push(kind);
            }
        }
        self
    }

    /// The largest file, in bytes.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The most files in one send.
    pub fn files(&self) -> usize {
        self.files
    }

    /// The accepted kinds, lower case. Empty means any.
    pub fn types(&self) -> &[String] {
        &self.types
    }

    /// Whether a file of type `mime` is accepted.
    pub fn allows(&self, mime: &str) -> bool {
        self.types.is_empty()
            || self.types.iter().any(|kind| {
                kind.eq_ignore_ascii_case(mime)
                    || kind.strip_suffix('*').is_some_and(|prefix| {
                        mime.len() > prefix.len()
                            && mime
                                .get(..prefix.len())
                                .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
                    })
            })
    }

    /// A size in words: `800 B`, `1.5 KB`, `10 KB`, `2 MB`, `1.5 GB`. Binary units, and no
    /// `.0`. The browser module writes sizes the same way.
    ///
    /// ```
    /// use navree_ui::UploadLimits;
    ///
    /// assert_eq!(UploadLimits::size_text(1536), "1.5 KB");
    /// assert_eq!(UploadLimits::size_text(2 * 1024 * 1024), "2 MB");
    /// ```
    pub fn size_text(bytes: u64) -> String {
        const KB: f64 = 1024.0;
        if bytes < 1024 {
            return format!("{bytes} B");
        }
        let value = bytes as f64;
        let (number, unit) = if bytes < 1024 * 1024 {
            (value / KB, "KB")
        } else if bytes < 1024 * 1024 * 1024 {
            (value / (KB * KB), "MB")
        } else {
            (value / (KB * KB * KB), "GB")
        };
        let text = if unit == "KB" && number >= 10.0 {
            format!("{number:.0}")
        } else {
            let one = format!("{number:.1}");
            one.strip_suffix(".0").unwrap_or(&one).to_owned()
        };
        format!("{text} {unit}")
    }

    /// The kinds in words: `PNG or JPG`, `PDF, images or CSV`. Empty when any kind is
    /// accepted.
    pub fn types_text(&self, labels: &UploadLabels) -> String {
        let mut names: Vec<String> = Vec::new();
        for kind in &self.types {
            let name = type_name(kind, labels);
            if !names.contains(&name) {
                names.push(name);
            }
        }
        match names.len() {
            0 => String::new(),
            1 => names.remove(0),
            count => format!(
                "{} {} {}",
                names[..count - 1].join(", "),
                labels.or,
                names[count - 1]
            ),
        }
    }

    /// The limits as one sentence for the hint: `PNG or JPG, up to 2 MB, up to 5 files`.
    pub fn sentence(&self, labels: &UploadLabels) -> String {
        let mut parts: Vec<String> = Vec::new();
        let types = self.types_text(labels);
        if !types.is_empty() {
            parts.push(types);
        }
        let size = Self::size_text(self.bytes);
        parts.push(fill(&labels.up_to, &[("size", size.as_str())]));
        if self.files > 1 {
            let count = self.files.to_string();
            parts.push(fill(&labels.up_to_files, &[("count", count.as_str())]));
        }
        parts.join(", ")
    }

    /// The same limits as navree-storage's upload rules, which hold a file to them while
    /// it is written.
    #[cfg(feature = "storage")]
    pub fn rules(&self) -> navree_storage::UploadRules {
        navree_storage::UploadRules::new(self.bytes, self.types.iter().cloned())
    }
}

/// `raw` as `type/subtype` or `type/*` in lower case, or nothing: only letters, digits and
/// `. + - _` get through, so no entry can carry anything into an attribute.
fn clean_type(raw: &str) -> Option<String> {
    let lower = raw.trim().to_ascii_lowercase();
    let (kind, sub) = lower.split_once('/')?;
    let word = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || matches!(b, b'.' | b'+' | b'-' | b'_')
            })
    };
    (word(kind) && (word(sub) || sub == "*")).then_some(lower)
}

/// A kind of file as people name it.
fn type_name(kind: &str, labels: &UploadLabels) -> String {
    let word = match kind {
        "image/png" => "PNG",
        "image/jpeg" | "image/jpg" => "JPG",
        "image/webp" => "WebP",
        "image/gif" => "GIF",
        "image/avif" => "AVIF",
        "image/svg+xml" => "SVG",
        "application/pdf" => "PDF",
        "application/zip" => "ZIP",
        "application/json" => "JSON",
        "text/csv" => "CSV",
        "application/msword"
        | "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "Word",
        "application/vnd.ms-excel"
        | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "Excel",
        "application/vnd.ms-powerpoint"
        | "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            "PowerPoint"
        }
        "image/*" => return labels.images.clone(),
        "video/*" => return labels.videos.clone(),
        "audio/*" => return labels.audio.clone(),
        "text/*" | "text/plain" => return labels.text.clone(),
        other => {
            return match other.split_once('/') {
                Some((_, sub)) if sub != "*" => sub.trim_start_matches("x-").to_ascii_uppercase(),
                _ => other.to_owned(),
            };
        }
    };
    word.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_named_once_and_joined_with_or() {
        let labels = UploadLabels::default();
        let named = |types: &[&str]| {
            UploadLimits::new(1)
                .accept(types.iter().copied())
                .types_text(&labels)
        };
        assert_eq!(named(&["image/png"]), "PNG");
        assert_eq!(named(&["image/png", "image/jpeg"]), "PNG or JPG");
        assert_eq!(
            named(&["image/jpeg", "image/jpg", "image/webp"]),
            "JPG or WebP"
        );
        assert_eq!(
            named(&["application/pdf", "image/*", "text/csv"]),
            "PDF, images or CSV"
        );
        assert_eq!(named(&["application/x-tar"]), "TAR");
        assert_eq!(named(&[]), "");
    }

    #[test]
    fn a_hostile_kind_is_dropped() {
        let limits =
            UploadLimits::new(1).accept(["image/png\"><script>", "*/*", "image/ *", "<b>/x"]);
        assert!(limits.types().is_empty());
    }
}
