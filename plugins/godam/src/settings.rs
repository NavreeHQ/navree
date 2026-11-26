//! Per-disk settings for a `godam` storage disk (`[storage.disks.NAME]`).

use std::fmt;

use navree::prelude::BoxError;
use serde::Deserialize;

/// Settings read from a `godam` disk's table (`[storage.disks.NAME]`).
#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GodamSettings {
    /// Optional folder inside the backing store that holds this disk's files,
    /// such as `"ludhiana/"`. Empty keeps files at the store root.
    pub prefix: String,
}

impl fmt::Debug for GodamSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GodamSettings").finish_non_exhaustive()
    }
}

impl GodamSettings {
    /// Checks that `prefix` is empty or a relative folder inside the disk.
    ///
    /// When `disk` is given, the error names `storage.disks.NAME.prefix` and
    /// never repeats any value from the table.
    pub fn validate(&self, disk: Option<&str>) -> Result<(), BoxError> {
        self.parse_prefix(disk).map(drop)
    }

    pub(crate) fn parse_prefix(&self, disk: Option<&str>) -> Result<String, BoxError> {
        let name = disk.unwrap_or("default");
        let invalid = || -> BoxError {
            format!(
                "storage.disks.{name}.prefix must be a folder inside the disk, such as \"ludhiana/\""
            )
            .into()
        };

        let raw = self.prefix.as_str();
        if raw.is_empty() {
            return Ok(String::new());
        }
        if raw.len() > 1024
            || raw.starts_with('/')
            || raw.contains('\\')
            || raw.contains(':')
            || raw.chars().any(char::is_control)
        {
            return Err(invalid());
        }

        let trimmed = raw.strip_suffix('/').unwrap_or(raw);
        if trimmed.is_empty() {
            return Err(invalid());
        }
        for segment in trimmed.split('/') {
            if segment.is_empty()
                || segment == "."
                || segment == ".."
                || segment.chars().all(|c| c == '.' || c == ' ')
            {
                return Err(invalid());
            }
        }

        Ok(trimmed.to_owned())
    }
}
