//! [`TemplateDir`] and [`ModuleBuilderPluginTemplatesExt`]: the templates a plugin
//! ships in its binary.

use navree_kernel::{ModuleBuilder, PublishTag, Publishable};

use crate::module::EmbeddedTemplates;

/// Templates embedded with `navree::embed_templates!("views")`: each file's
/// path relative to the folder, and its source. Registered with
/// `m.plugin_templates(..)` under the plugin's namespace.
///
/// ```
/// use navree_view::TemplateDir;
///
/// const VIEWS: &[(&str, &str)] = &[
///     ("list.html", "<ul>{% for notice in notices %}<li>{{ notice }}</li>{% endfor %}</ul>"),
///     ("admin/edit.html", "<form>{{ notice }}</form>"),
/// ];
///
/// let dir = TemplateDir::from(VIEWS);
/// assert_eq!(dir.files().len(), 2);
/// assert_eq!(dir.files()[1].0, "admin/edit.html");
/// ```
#[derive(Clone, Copy)]
pub struct TemplateDir(&'static [(&'static str, &'static str)]);

impl TemplateDir {
    /// The files as (path relative to the folder, source), in the order they
    /// were embedded.
    pub fn files(&self) -> &'static [(&'static str, &'static str)] {
        self.0
    }
}

impl From<&'static [(&'static str, &'static str)]> for TemplateDir {
    fn from(files: &'static [(&'static str, &'static str)]) -> Self {
        Self(files)
    }
}

/// A plugin's templates on a [`ModuleBuilder`].
///
/// ```
/// use navree_kernel::{Module, ModuleBuilder};
/// use navree_view::{ModuleBuilderPluginTemplatesExt, TemplateDir};
///
/// const VIEWS: &[(&str, &str)] = &[(
///     "list.html",
///     "<ul>{% for notice in notices %}<li>{{ notice }}</li>{% endfor %}</ul>",
/// )];
///
/// struct AnnouncementsModule;
///
/// impl Module for AnnouncementsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.plugin(navree_kernel::plugin_info!(namespace = "announcements"))
///             .plugin_templates(TemplateDir::from(VIEWS));
///     }
/// }
/// ```
pub trait ModuleBuilderPluginTemplatesExt {
    /// Registers `dir` under the plugin's namespace (`list.html` becomes
    /// `announcements/list.html`) and makes it publishable (tag `views`). The
    /// namespace is read when the app boots, so `m.plugin(..)` may come before
    /// or after it. A file of the same name in a template folder
    /// (`resources/views/announcements/list.html`) wins, and
    /// `plugin:publish {plugin} --tag views` copies the files there. Build
    /// `dir` with `navree::embed_templates!("views")`.
    fn plugin_templates(&mut self, dir: TemplateDir) -> &mut Self;
}

impl ModuleBuilderPluginTemplatesExt for ModuleBuilder {
    fn plugin_templates(&mut self, dir: TemplateDir) -> &mut Self {
        self.contribute(EmbeddedTemplates {
            templates: dir.files().to_vec(),
            under_namespace: true,
        });
        if dir.files().is_empty() {
            return self;
        }
        let views = dir.files().iter().fold(
            Publishable::new(PublishTag::Views),
            |views, &(path, source)| views.file(path, source.as_bytes()),
        );
        self.contribute(views)
    }
}
