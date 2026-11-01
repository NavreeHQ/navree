//! The embedded templates of every generated file.
//!
//! Templates live in `crates/navree-cli/templates/<name>.j2` and are compiled
//! into the binary with `include_str!`. Blocks trim their own line
//! (`trim_blocks` + `lstrip_blocks`), the trailing newline is kept, nothing
//! is auto-escaped, and an undefined variable is an error.

use std::sync::OnceLock;

use navree_template::{AutoEscape, Environment, UndefinedBehavior};
use serde::Serialize;

macro_rules! templates {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../templates/", $name, ".j2")))),*]
    };
}

/// `(name, source)` of every template.
const TEMPLATES: &[(&str, &str)] = templates![
    "module/mod.rs",
    "module/controller.rs",
    "module/service.rs",
    "module/entity.rs",
    "module/dto.rs",
    "module/migrations/mod.rs",
    "module/migrations/create_table.rs",
    "module/tests/mod.rs",
    "app/Cargo.toml",
    "app/gitignore",
    "app/env.example",
    "app/README.md",
    "app/AGENTS.md",
    "app/CLAUDE.md",
    "app/config/app.toml",
    "app/dock.toml",
    "app/src/main.rs",
    "app/src/lib.rs",
    "app/src/app.rs",
    "app/src/abilities.rs",
    "app/src/modules/mod.rs",
    "app/src/modules/users/mod.rs",
    "app/src/modules/users/controller.rs",
    "app/src/modules/users/service.rs",
    "app/src/modules/users/entity.rs",
    "app/src/modules/users/dto.rs",
    "app/src/modules/users/migrations/mod.rs",
    "app/src/modules/users/migrations/create_user_profiles_table.rs",
    "app/src/modules/users/tests/mod.rs",
    "app/tests/app.rs",
    "app/storage/gitignore",
    "web/package.json",
    "web/tsconfig.json",
    "web/next.config.ts",
    "web/env.example",
    "web/README.md",
    "web/lib/bff.ts",
    "web/app/layout.tsx",
    "web/app/page.tsx",
    "web/app/login/page.tsx",
    "web/app/api/auth/login/route.ts",
    "web/app/api/auth/logout/route.ts",
    "web/app/api/proxy/route.ts",
    "make/controller.rs",
    "make/service.rs",
    "make/entity.rs",
    "make/migration.rs",
    "make/job.rs",
    "make/event.rs",
    "make/listener.rs",
    "make/mail.rs",
    "make/mail.html",
    "make/mail.txt",
    "make/notification.rs",
    "make/policy.rs",
    "make/command.rs",
    "make/agent.rs",
    "make/test.rs",
    "make/mcp.rs",
    "make/resource.rs",
    "make/model.rs",
    "make/factory.rs",
    "make/seeder.rs",
    "make/model_controller.rs",
    "make/model_policy.rs",
];

/// A template failed to render (a bug in the template or its context).
#[derive(Debug, thiserror::Error)]
#[error("cannot render template `{name}`: {source}")]
pub struct TemplateError {
    /// The template name.
    pub name: String,
    /// The template engine's error.
    #[source]
    pub source: navree_template::Error,
}

fn environment() -> &'static Environment {
    static ENV: OnceLock<Environment> = OnceLock::new();
    ENV.get_or_init(|| {
        let mut env = Environment::new();
        env.set_trim_blocks(true);
        env.set_lstrip_blocks(true);
        env.set_keep_trailing_newline(true);
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_auto_escape_callback(|_| AutoEscape::None);
        for (name, source) in TEMPLATES.iter().copied().chain(crate::kits::templates()) {
            env.add_template(name, source)
                .unwrap_or_else(|error| panic!("embedded template `{name}` is invalid: {error}"));
        }
        env
    })
}

/// Renders template `name` with `context`.
pub fn render(name: &str, context: impl Serialize) -> Result<String, TemplateError> {
    let error = |source| TemplateError {
        name: name.to_owned(),
        source,
    };
    environment()
        .get_template(name)
        .map_err(error)?
        .render(&context)
        .map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_template_parses() {
        for (name, _) in TEMPLATES.iter().copied().chain(crate::kits::templates()) {
            assert!(environment().get_template(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn unknown_variables_are_errors() {
        let error = render("module/mod.rs", serde_json::json!({})).unwrap_err();
        assert!(error.to_string().contains("module/mod.rs"), "{error}");
    }
}
