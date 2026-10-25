//! Level filters: a default level plus per-target levels, compiled into an
//! `EnvFilter` (`info,keel_queue=debug`).

use std::collections::BTreeMap;

use tracing_subscriber::EnvFilter;

use crate::LogError;

/// The level names accepted in options and at runtime.
const LEVELS: [&str; 6] = ["trace", "debug", "info", "warn", "error", "off"];

/// The canonical (lower-case) form of `level`, if it is a level name.
pub(crate) fn parse_level(level: &str) -> Result<&'static str, LogError> {
    let trimmed = level.trim();
    LEVELS
        .into_iter()
        .find(|known| known.eq_ignore_ascii_case(trimmed))
        .ok_or_else(|| LogError::InvalidLevel {
            level: level.to_owned(),
        })
}

/// Accepts targets made of ASCII letters, digits, `_`, `-`, `.` and `:`
/// (module paths such as `keel_db` or `keel::request`), so a target can
/// never inject other filter directives.
pub(crate) fn check_target(target: &str) -> Result<(), LogError> {
    let valid = !target.is_empty()
        && target
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'));
    if valid {
        Ok(())
    } else {
        Err(LogError::InvalidTarget {
            target: target.to_owned(),
        })
    }
}

/// The filter state: a default directive and per-target levels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Levels {
    /// The default level, or a raw directive list (`RUST_LOG` in tests).
    default: String,
    /// Target → canonical level.
    targets: BTreeMap<String, &'static str>,
}

impl Levels {
    /// Validated levels from options.
    pub(crate) fn new(level: &str, levels: &BTreeMap<String, String>) -> Result<Self, LogError> {
        let mut targets = BTreeMap::new();
        for (target, level) in levels {
            check_target(target)?;
            targets.insert(target.clone(), parse_level(level)?);
        }
        Ok(Self {
            default: parse_level(level)?.to_owned(),
            targets,
        })
    }

    /// A raw directive list (`EnvFilter` syntax), validated by [`filter`](Self::filter).
    pub(crate) fn raw(directives: impl Into<String>) -> Self {
        Self {
            default: directives.into(),
            targets: BTreeMap::new(),
        }
    }

    pub(crate) fn set_default(&mut self, level: &'static str) {
        self.default = level.to_owned();
    }

    pub(crate) fn set_target(&mut self, target: &str, level: &'static str) {
        self.targets.insert(target.to_owned(), level);
    }

    /// `default,target=level,...`, targets in name order.
    pub(crate) fn directives(&self) -> String {
        let mut directives = self.default.clone();
        for (target, level) in &self.targets {
            if !directives.is_empty() {
                directives.push(',');
            }
            directives.push_str(target);
            directives.push('=');
            directives.push_str(level);
        }
        directives
    }

    /// The compiled filter.
    pub(crate) fn filter(&self) -> Result<EnvFilter, LogError> {
        let directives = self.directives();
        EnvFilter::builder()
            .parse(&directives)
            .map_err(|error| LogError::InvalidFilter {
                directives,
                message: error.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_level_is_case_insensitive_and_canonical() {
        assert_eq!(parse_level(" WARN ").expect("level"), "warn");
        assert_eq!(parse_level("Off").expect("level"), "off");
        assert!(parse_level("verbose").is_err());
        assert!(parse_level("").is_err());
    }

    #[test]
    fn targets_allow_module_paths_only() {
        for ok in ["keel", "keel_db", "keel::request", "my-crate", "a.b"] {
            assert!(check_target(ok).is_ok(), "{ok}");
        }
        for bad in ["", " ", "a b", "a=b", "a,b", "a[x]", "a{x}", "ä"] {
            assert!(check_target(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn raw_directives_are_validated_when_compiled() {
        assert!(Levels::raw("info,hyper=warn").filter().is_ok());
        let error = Levels::raw("info,=[")
            .filter()
            .expect_err("invalid directives");
        assert!(matches!(error, LogError::InvalidFilter { .. }), "{error:?}");
    }
}
