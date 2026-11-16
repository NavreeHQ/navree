//! [`EmbeddedLocales`]: the Fluent files a plugin ships in its binary.

use std::fmt;

use navree_kernel::{ModuleBuilder, PublishTag, Publishable};

/// Fluent files a plugin embeds with `navree::embed_locales!("locales")`: each
/// file's locale, the path it was read from and its source. Contributed with
/// `m.locales(..)`; navree-i18n loads them after the app's own files when the
/// app has `I18nModule`.
///
/// ```
/// use navree_contracts::EmbeddedLocales;
///
/// const FILES: &[(&str, &str, &str)] = &[
///     ("en", "locales/en/announcements.ftl", "announcements-title = Notices for Ludhiana\n"),
///     ("pa", "locales/pa/announcements.ftl", "announcements-title = ਲੁਧਿਆਣਾ ਲਈ ਸੂਚਨਾਵਾਂ\n"),
/// ];
///
/// let locales = EmbeddedLocales::from(FILES);
/// assert_eq!(locales.files().len(), 2);
/// assert_eq!(locales.files()[0].0, "en");
/// assert_eq!(format!("{locales:?}"), "EmbeddedLocales(2 file(s))");
/// ```
#[derive(Clone, Copy)]
pub struct EmbeddedLocales(&'static [(&'static str, &'static str, &'static str)]);

impl EmbeddedLocales {
    /// The files as (locale, path, source), in the order they were embedded.
    pub fn files(&self) -> &'static [(&'static str, &'static str, &'static str)] {
        self.0
    }
}

impl From<&'static [(&'static str, &'static str, &'static str)]> for EmbeddedLocales {
    fn from(files: &'static [(&'static str, &'static str, &'static str)]) -> Self {
        Self(files)
    }
}

impl fmt::Debug for EmbeddedLocales {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EmbeddedLocales({} file(s))", self.0.len())
    }
}

/// A plugin's translations on a [`ModuleBuilder`].
///
/// ```
/// use navree_contracts::{EmbeddedLocales, ModuleBuilderLocalesExt};
/// use navree_kernel::{Module, ModuleBuilder};
///
/// const FILES: &[(&str, &str, &str)] = &[(
///     "en",
///     "locales/en/announcements.ftl",
///     "announcements-title = Notices for Ludhiana\n",
/// )];
///
/// struct AnnouncementsModule;
///
/// impl Module for AnnouncementsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.plugin(navree_kernel::plugin_info!(namespace = "announcements"))
///             .locales(EmbeddedLocales::from(FILES));
///     }
/// }
/// ```
pub trait ModuleBuilderLocalesExt {
    /// Contributes the files, and a `Publishable` (tag `locales`). Each file is
    /// published by `plugin:publish` to `locales/{locale}/{namespace}/` and by
    /// `i18n:publish {plugin}` below `i18n.dir`; its message ids start with the
    /// plugin's namespace. A module that names no plugin may call it only with
    /// no files.
    fn locales(&mut self, files: EmbeddedLocales) -> &mut Self;
}

impl ModuleBuilderLocalesExt for ModuleBuilder {
    fn locales(&mut self, files: EmbeddedLocales) -> &mut Self {
        self.contribute(files);
        if files.0.is_empty() {
            return self;
        }
        let publishable = files.0.iter().fold(
            Publishable::new(PublishTag::Locales),
            |publishable, &(locale, path, source)| {
                publishable.file(publish_path(locale, path), source.as_bytes())
            },
        );
        self.contribute(publishable)
    }
}

/// The path a locale file is published under: `{locale}/{rest}`, where `rest`
/// is the part of `path` after its first `/{locale}/`, else after a leading
/// `{locale}/`, else all of `path`. The kernel checks the result at boot.
fn publish_path(locale: &str, path: &str) -> String {
    let inside = format!("/{locale}/");
    let rest = path
        .find(&inside)
        .map(|at| &path[at + inside.len()..])
        .or_else(|| {
            path.strip_prefix(locale)
                .and_then(|after| after.strip_prefix('/'))
        })
        .unwrap_or(path);
    format!("{locale}/{rest}")
}

#[cfg(test)]
mod tests {
    use navree_kernel::{Module, ModuleBuilder, Navree, PluginInfo, PublishTag};

    use super::*;

    const FILES: &[(&str, &str, &str)] = &[
        (
            "en",
            "locales/en/announcements.ftl",
            "announcements-title = Notices for Ludhiana\n",
        ),
        (
            "hi",
            "locales/hi/announcements.ftl",
            "announcements-title = लुधियाना के लिए सूचनाएँ\n",
        ),
    ];

    /// A plugin of this crate shipping the files it holds; `m.locales` before `m.plugin`.
    struct Announcements(&'static [(&'static str, &'static str, &'static str)]);
    impl Module for Announcements {
        fn register(&self, m: &mut ModuleBuilder) {
            m.locales(EmbeddedLocales::from(self.0))
                .plugin(PluginInfo::new(
                    "navree-plugin-announcements",
                    "navree_contracts",
                    "announcements",
                    "1.0.0",
                ));
        }
    }

    #[test]
    fn m_locales_contributes_without_i18n() {
        let report = Navree::app()
            .module(Announcements(FILES))
            .plugin_report()
            .expect("valid");
        let module = &report.plugins[0].modules[0];
        assert_eq!(module.publishable, [(PublishTag::Locales, 2)]);
        assert!(
            module
                .contributions
                .contains(&("navree_contracts::locales::EmbeddedLocales", 1))
        );
        assert!(
            module
                .contributions
                .contains(&("navree_kernel::plugin::Publishable", 1))
        );
        assert_eq!(EmbeddedLocales::from(FILES).files(), FILES);
        assert_eq!(
            format!("{:?}", EmbeddedLocales::from(FILES)),
            "EmbeddedLocales(2 file(s))"
        );
        // No files: nothing to publish, so a module that names no plugin may call it.
        const NONE: &[(&str, &str, &str)] = &[];
        struct Empty;
        impl Module for Empty {
            fn register(&self, m: &mut ModuleBuilder) {
                m.locales(EmbeddedLocales::from(NONE));
            }
        }
        Navree::app()
            .module(Empty)
            .validate()
            .expect("nothing to publish");
    }

    #[test]
    fn the_publish_path_of_a_locale_file_is_its_locale_then_the_rest() {
        for (locale, path, expected) in [
            ("en", "locales/en/announcements.ftl", "en/announcements.ftl"),
            (
                "pa-IN",
                "locales/pa-IN/mail/welcome.ftl",
                "pa-IN/mail/welcome.ftl",
            ),
            ("en", "en/announcements.ftl", "en/announcements.ftl"),
            ("en", "/srv/ludhiana/locales/en/a.ftl", "en/a.ftl"),
            ("en", "locales/en/sub/en/a.ftl", "en/sub/en/a.ftl"),
            ("en", "a.ftl", "en/a.ftl"),
        ] {
            assert_eq!(publish_path(locale, path), expected, "{path}");
        }
    }

    #[test]
    fn unsafe_locale_paths_fail_the_boot() {
        const UNSAFE: &[(&str, &str, &str)] = &[
            ("en", "locales/en/../../secret.ftl", "a = 1\n"),
            ("en", "/etc/passwd", "a = 1\n"),
            ("en", "locales/en/mail\\welcome.ftl", "a = 1\n"),
            ("en", "locales/en/con.ftl", "a = 1\n"),
            ("en", "locales/en/Notices.ftl", "a = 1\n"),
            ("en", "locales/en/notices.ftl", "a = 1\n"),
            ("e n", "locales/e n/a.ftl", "a = 1\n"),
        ];
        let text = Navree::app()
            .module(Announcements(UNSAFE))
            .validate()
            .expect_err("unsafe")
            .to_string();
        for path in [
            "en/../../secret.ftl",
            "en//etc/passwd",
            "en/mail\\welcome.ftl",
            "en/con.ftl",
            "en/notices.ftl",
            "e n/a.ftl",
        ] {
            assert!(
                text.contains(&format!(
                    "the locales file `{path}` is not a safe relative path inside its folder"
                )),
                "{path}: {text}"
            );
        }
    }
}
