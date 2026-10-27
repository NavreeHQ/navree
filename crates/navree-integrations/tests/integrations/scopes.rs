//! The scope sets the tests' feature module declares.

use navree_integrations::{IntegrationsModule, ModuleBuilderIntegrationsExt, ScopeSet};
use navree_kernel::{Module, ModuleBuilder};

/// Read users and groups.
pub struct DirectoryRead;

impl ScopeSet for DirectoryRead {
    const NAME: &'static str = "directory";
    const DESCRIPTION: &'static str = "Read users and groups";

    fn scopes(provider: &str) -> &'static [&'static str] {
        match provider {
            "microsoft" => &[
                "https://graph.microsoft.com/User.Read.All",
                "https://graph.microsoft.com/Group.Read.All",
            ],
            "google" => &[
                "https://www.googleapis.com/auth/admin.directory.user.readonly",
                "https://www.googleapis.com/auth/admin.directory.group.readonly",
            ],
            _ => &[],
        }
    }
}

/// Send mail as any user.
pub struct MailSend;

impl ScopeSet for MailSend {
    const NAME: &'static str = "mail_send";
    const DESCRIPTION: &'static str = "Send mail as any user";

    fn scopes(provider: &str) -> &'static [&'static str] {
        match provider {
            "microsoft" => &["https://graph.microsoft.com/Mail.Send"],
            "google" => &["https://www.googleapis.com/auth/gmail.send"],
            _ => &[],
        }
    }
}

/// A feature module that needs both: one through `for_feature`, one
/// through the builder extension.
pub struct FeatureModule;

impl Module for FeatureModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.imports(IntegrationsModule::for_feature::<DirectoryRead>())
            .integration_scope_set::<MailSend>();
    }
}
