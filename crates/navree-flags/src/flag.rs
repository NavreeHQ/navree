//! [`Flag`]: a feature flag as a type, and [`FlagDefinition`], what the
//! module knows about one.

use serde::Serialize;

/// A feature flag. Implement it with `#[flag]` (navree-macros, re-exported
/// as `navree::flags::flag`) on a unit struct:
///
/// ```ignore
/// use navree::flags::flag;
///
/// /// The redesigned deal pipeline board.
/// #[flag]
/// pub struct NewPipeline;
///
/// /// AI summaries on the deal page.
/// #[flag(name = "ai-summaries", default = false, expose)]
/// pub struct AiSummaries;
/// ```
///
/// By hand, the same as the second flag:
///
/// ```
/// use navree_flags::Flag;
///
/// pub struct AiSummaries;
///
/// impl Flag for AiSummaries {
///     const NAME: &'static str = "ai-summaries";
///     const EXPOSE: bool = true;
///     const DESCRIPTION: &'static str = "AI summaries on the deal page.";
/// }
/// ```
///
/// Register each flag with `FlagsModule::for_feature::<F>()`.
pub trait Flag: 'static {
    /// The stored name (`new-pipeline`): lower-case letters, digits, `-`,
    /// `_` and `.`, at most [`MAX_NAME_LEN`] characters. State is keyed by
    /// it, so keep it stable.
    const NAME: &'static str;
    /// The value when nothing else decides (default `false`).
    const DEFAULT: bool = false;
    /// Whether `GET /api/flags` serves the flag to the front end (default
    /// `false`).
    const EXPOSE: bool = false;
    /// What the flag is for (the doc comment, with `#[flag]`), shown by
    /// `flags:list` and the admin routes.
    const DESCRIPTION: &'static str = "";
}

/// The longest flag name.
pub const MAX_NAME_LEN: usize = 100;

/// A registered flag: its name, default, exposure and description.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FlagDefinition {
    /// The stored name.
    pub name: &'static str,
    /// The value when nothing else decides.
    pub default: bool,
    /// Whether `GET /api/flags` serves it.
    pub expose: bool,
    /// What it is for.
    pub description: &'static str,
    /// The Rust type that defines it (for error messages).
    #[serde(skip)]
    pub type_name: &'static str,
}

impl FlagDefinition {
    /// The definition of `F`.
    pub fn of<F: Flag>() -> Self {
        FlagDefinition {
            name: F::NAME,
            default: F::DEFAULT,
            expose: F::EXPOSE,
            description: F::DESCRIPTION,
            type_name: std::any::type_name::<F>(),
        }
    }
}

/// Checks a flag name: 1 to [`MAX_NAME_LEN`] characters, lower-case ASCII
/// letters, digits, `-`, `_` and `.`, starting with a letter or a digit.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("a flag name must not be empty".to_owned());
    }
    if name.len() > MAX_NAME_LEN {
        return Err(format!(
            "the flag name `{name}` is longer than {MAX_NAME_LEN} characters"
        ));
    }
    let valid = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.');
    let first = name.chars().next().unwrap_or('-');
    if !name.chars().all(valid) || !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(format!(
            "the flag name `{name}` may only hold lower-case letters, digits, `-`, `_` and `.`, \
             and must start with a letter or a digit"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NewPipeline;

    impl Flag for NewPipeline {
        const NAME: &'static str = "new-pipeline";
    }

    struct DealExport;

    impl Flag for DealExport {
        const NAME: &'static str = "deal-export";
        const DEFAULT: bool = true;
        const EXPOSE: bool = true;
        const DESCRIPTION: &'static str = "The CSV export of deals.";
    }

    #[test]
    fn definitions_carry_the_constants() {
        let plain = FlagDefinition::of::<NewPipeline>();
        assert_eq!(plain.name, "new-pipeline");
        assert!(!plain.default && !plain.expose);
        assert_eq!(plain.description, "");
        assert!(plain.type_name.ends_with("NewPipeline"));

        let export = FlagDefinition::of::<DealExport>();
        assert!(export.default && export.expose);
        assert_eq!(export.description, "The CSV export of deals.");
        assert_eq!(
            serde_json::to_value(export).unwrap(),
            serde_json::json!({
                "name": "deal-export",
                "default": true,
                "expose": true,
                "description": "The CSV export of deals.",
            })
        );
    }

    #[test]
    fn names_are_checked() {
        for good in ["new-pipeline", "ai_summaries", "v2.board", "2fa"] {
            assert_eq!(check_name(good), Ok(()), "{good}");
        }
        for bad in ["", "New-Pipeline", "new pipeline", "-x", "émoji", ".x"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        assert!(check_name(&"a".repeat(MAX_NAME_LEN)).is_ok());
        assert!(check_name(&"a".repeat(MAX_NAME_LEN + 1)).is_err());
    }
}
