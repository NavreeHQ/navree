//! CONFIG-009: the environment name, parsed once. Only `production` changes
//! what modules do today; `staging` does not (undecided), and the one place
//! to change that is `Environment::behaves_like_production`.

use navree_config::{Config, ConfigSources, Environment};

fn env(pairs: &[(&str, &str)]) -> Config {
    Config::load(&ConfigSources::new().env_vars("APP_", pairs.iter().copied())).expect("load")
}

#[test]
fn names_parse_in_any_case_with_surrounding_spaces() {
    assert_eq!(Environment::parse("production"), Environment::Production);
    assert_eq!(Environment::parse("  Production "), Environment::Production);
    assert_eq!(Environment::parse("PRODUCTION"), Environment::Production);
    assert_eq!(Environment::parse("staging"), Environment::Staging);
    assert_eq!(Environment::parse("testing"), Environment::Testing);
    assert_eq!(Environment::parse(" Local"), Environment::Local);
    // Other names keep the name that was written (lower-cased).
    for other in ["development", "dev", "test", "stage"] {
        assert_eq!(
            Environment::parse(other),
            Environment::Other(other.to_owned())
        );
    }
    assert_eq!(
        Environment::parse("Development"),
        Environment::Other("development".to_owned())
    );
    assert_eq!(
        Environment::parse("QA"),
        Environment::Other("qa".to_owned())
    );
    assert_eq!(Environment::from("qa"), Environment::Other("qa".to_owned()));
}

#[test]
fn only_the_whole_word_production_is_production() {
    // `prod` and `live` are other names; they keep the development
    // behaviour they have always had.
    for other in ["prod", "live", "production-eu", "preproduction"] {
        assert!(!Environment::parse(other).is_production(), "{other}");
    }
    assert!(Environment::parse("production").is_production());
}

#[test]
fn names_print_in_their_canonical_form() {
    assert_eq!(Environment::Local.as_str(), "local");
    assert_eq!(Environment::Testing.to_string(), "testing");
    assert_eq!(Environment::Staging.as_str(), "staging");
    assert_eq!(Environment::Production.as_str(), "production");
    assert_eq!(Environment::parse("QA").to_string(), "qa");
    assert_eq!(Environment::parse("development").as_str(), "development");
}

#[test]
fn staging_does_not_behave_like_production_yet() {
    // Whether staging behaves like production is undecided: the default
    // stays as it was. Changing it is one
    // line in `Environment::behaves_like_production`.
    assert!(Environment::Production.behaves_like_production());
    assert!(!Environment::Staging.behaves_like_production());
    assert!(!Environment::Local.behaves_like_production());
    assert!(!Environment::Testing.behaves_like_production());
    assert!(!Environment::parse("qa").behaves_like_production());
    assert!(Environment::Staging.is_staging());
    assert!(Environment::Local.is_local());
}

#[test]
fn the_config_names_its_environment() {
    assert_eq!(
        env(&[("APP_ENV", "production")]).environment(),
        Some(Environment::Production)
    );
    assert_eq!(
        env(&[("APP_ENV", "Production")]).environment(),
        Some(Environment::Production)
    );
    assert_eq!(
        env(&[("APP_APP__ENV", "staging")]).environment(),
        Some(Environment::Staging)
    );
    assert_eq!(
        env(&[]).environment(),
        None,
        "no environment is not `local`"
    );
    assert_eq!(
        env(&[("APP_ENV", " ")]).environment(),
        None,
        "blank names none"
    );
}

#[test]
fn the_config_says_whether_it_behaves_like_production() {
    assert!(env(&[("APP_ENV", "production")]).behaves_like_production());
    assert!(env(&[("APP_APP__ENV", "PRODUCTION")]).behaves_like_production());
    assert!(!env(&[("APP_ENV", "staging")]).behaves_like_production());
    assert!(!env(&[("APP_ENV", "development")]).behaves_like_production());
    assert!(
        !env(&[]).behaves_like_production(),
        "unset is not production"
    );
}

#[test]
fn the_top_level_env_key_is_still_in_the_tree() {
    // The modules that have not moved yet look at `env`.
    let config = env(&[("APP_ENV", "production")]);
    assert_eq!(config.get_string("env").as_deref(), Some("production"));
}

#[test]
fn an_environment_deserializes_from_a_string() {
    #[derive(serde::Deserialize)]
    struct Holder {
        env: Environment,
    }
    let holder: Holder = serde_json::from_str(r#"{"env": "Staging"}"#).expect("json");
    assert_eq!(holder.env, Environment::Staging);
}
