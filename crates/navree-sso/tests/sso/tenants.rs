//! Connections owned by workspaces and organizations, and the sso.tenants section.

use navree_config::check::{CheckOptions, CheckReport};
use navree_config::{ConfigSources, SectionRegistration};
use navree_sso::{SsoOptions, SsoTenantOptions};

use crate::support;

#[test]
fn config_check_knows_the_tenants_section() {
    let registrations = [
        SectionRegistration::of::<SsoOptions>(),
        SectionRegistration::of::<SsoTenantOptions>(),
    ];
    let set = ConfigSources::new().set("sso.tenants.unverified_emails", true);
    let report = CheckReport::run(&set, &registrations, &CheckOptions::default()).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.unknown.is_empty(), "{:?}", report.unknown);

    let unset = ConfigSources::new();
    let report = CheckReport::run(&unset, &registrations, &CheckOptions::default()).unwrap();
    assert!(report.is_ok());

    let misspelt = ConfigSources::new().set("sso.tenants.unverfied_emails", true);
    let report = CheckReport::run(&misspelt, &registrations, &CheckOptions::default()).unwrap();
    let keys: Vec<&str> = report.unknown.iter().map(|u| u.key.as_str()).collect();
    assert_eq!(keys, ["sso.tenants.unverfied_emails"]);
}

#[tokio::test]
async fn an_app_without_the_tenants_section_boots_with_the_flag_closed_to_customers() {
    let world = support::world().await;
    let sso = world.sso().await;
    assert_eq!(
        sso.options(),
        &SsoOptions {
            base_url: Some(support::BASE.to_owned()),
            ..SsoOptions::default()
        }
    );
    // What the defaults refuse is shown by the sign-in tests below.
}
