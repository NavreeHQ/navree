//! `#[derive(Settings)]`: the constants it writes.

use navree_settings::{ScopeKind, SettingsDefinition, SettingsGroup};

use crate::support::{NotificationSettings, PortalSettings, ViewPreferences};

/// Team portal visibility for managers.
///
/// Shown on the manager dashboard.
#[derive(
    navree_settings::Settings,
    Clone,
    Debug,
    Default,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    garde::Validate,
)]
pub struct TeamPortalSettings {
    #[garde(skip)]
    pub visible: bool,
}

#[test]
fn the_derive_writes_the_documented_constants() {
    assert_eq!(PortalSettings::KEY, "portal");
    assert_eq!(
        PortalSettings::SCOPES,
        [
            ScopeKind::Global,
            ScopeKind::Organization,
            ScopeKind::Workspace,
            ScopeKind::User
        ]
    );
    assert_eq!(PortalSettings::DESCRIPTION, "What the member portal shows.");
    assert_eq!(PortalSettings::PERMISSION, None);
    assert_eq!(PortalSettings::READ_PERMISSION, None);

    assert_eq!(NotificationSettings::SCOPES, [ScopeKind::Global, ScopeKind::Workspace]);
    assert_eq!(NotificationSettings::PERMISSION, Some("notifications.manage"));
}

#[test]
fn keys_default_to_the_type_name_and_levels_to_the_trait() {
    assert_eq!(ViewPreferences::KEY, "view-preferences");
    assert_eq!(ViewPreferences::SCOPES, [ScopeKind::User]);
    assert_eq!(TeamPortalSettings::KEY, "team-portal");
    assert_eq!(
        TeamPortalSettings::SCOPES,
        [
            ScopeKind::Global,
            ScopeKind::Organization,
            ScopeKind::Workspace
        ]
    );
    assert_eq!(
        TeamPortalSettings::DESCRIPTION,
        "Team portal visibility for managers. Shown on the manager dashboard."
    );
    let definition = SettingsDefinition::of::<TeamPortalSettings>();
    assert_eq!(definition.key, "team-portal");
    assert!(definition.type_name.ends_with("TeamPortalSettings"));
}
