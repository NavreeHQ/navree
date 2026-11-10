//! The AWS credentials Bedrock signs with when the configuration names no
//! role: the explicit keys, the environment or the shared credentials file.
//! Requests are signed by `navree_crypto::sigv4`, the signer SES and SNS
//! use too.

use std::path::{Path, PathBuf};

use navree_crypto::sigv4::Credentials;

/// The credentials from the explicit keys, else `AWS_ACCESS_KEY_ID` and
/// `AWS_SECRET_ACCESS_KEY` (and `AWS_SESSION_TOKEN`), else the shared
/// credentials file's `profile` (default `AWS_PROFILE`, else `default`).
/// `env` reads an environment variable and `read` a file.
pub(crate) fn resolve_credentials(
    explicit: (Option<&str>, Option<&str>, Option<&str>),
    profile: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
    read: impl Fn(&Path) -> Option<String>,
) -> Option<Credentials> {
    let non_empty = |value: Option<String>| value.filter(|v| !v.is_empty());
    if let (Some(id), Some(secret)) = (explicit.0, explicit.1)
        && !id.is_empty()
        && !secret.is_empty()
    {
        return Some(
            Credentials::new(id, secret).with_session_token(explicit.2.map(str::to_owned)),
        );
    }
    if let (Some(id), Some(secret)) = (
        non_empty(env("AWS_ACCESS_KEY_ID")),
        non_empty(env("AWS_SECRET_ACCESS_KEY")),
    ) {
        return Some(Credentials::new(id, secret).with_session_token(env("AWS_SESSION_TOKEN")));
    }
    let path = match non_empty(env("AWS_SHARED_CREDENTIALS_FILE")) {
        Some(path) => PathBuf::from(path),
        None => {
            let home = non_empty(env("HOME")).or_else(|| non_empty(env("USERPROFILE")))?;
            Path::new(&home).join(".aws").join("credentials")
        }
    };
    let profile = profile
        .map(str::to_owned)
        .or_else(|| non_empty(env("AWS_PROFILE")))
        .unwrap_or_else(|| "default".to_owned());
    parse_profile(&read(&path)?, &profile)
}

/// The keys of `[profile]` in an INI credentials file.
fn parse_profile(file: &str, profile: &str) -> Option<Credentials> {
    let mut current = None;
    let (mut id, mut secret, mut token) = (None, None, None);
    for line in file.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = Some(section.trim().trim_start_matches("profile ").to_owned());
            continue;
        }
        if current.as_deref() != Some(profile) {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().to_owned();
            match key.trim() {
                "aws_access_key_id" => id = Some(value),
                "aws_secret_access_key" => secret = Some(value),
                "aws_session_token" => token = Some(value),
                _ => {}
            }
        }
    }
    Some(Credentials::new(id?, secret?).with_session_token(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_resolve_in_order() {
        let none = |_: &str| None::<String>;
        let no_file = |_: &Path| None::<String>;
        let explicit =
            resolve_credentials((Some("A"), Some("S"), None), None, none, no_file).unwrap();
        assert_eq!(explicit.access_key_id, "A");

        let env = |name: &str| match name {
            "AWS_ACCESS_KEY_ID" => Some("ENVID".to_owned()),
            "AWS_SECRET_ACCESS_KEY" => Some("ENVSECRET".to_owned()),
            "AWS_SESSION_TOKEN" => Some("TOKEN".to_owned()),
            _ => None,
        };
        let from_env = resolve_credentials((None, None, None), None, env, no_file).unwrap();
        assert_eq!(from_env.session_token.as_deref(), Some("TOKEN"));

        let home = |name: &str| (name == "HOME").then(|| "/home/me".to_owned());
        let file = |path: &Path| {
            assert!(path.ends_with(Path::new(".aws").join("credentials")));
            Some(
                "[default]\naws_access_key_id = D\naws_secret_access_key = DS\n\n\
                 [profile work]\naws_access_key_id=W\naws_secret_access_key=WS\n"
                    .to_owned(),
            )
        };
        let default = resolve_credentials((None, None, None), None, home, file).unwrap();
        assert_eq!(default.access_key_id, "D");
        let work = resolve_credentials((None, None, None), Some("work"), home, file).unwrap();
        assert_eq!(work.secret_access_key, "WS");
        assert!(resolve_credentials((None, None, None), Some("ghost"), home, file).is_none());
        assert!(resolve_credentials((None, None, None), None, none, no_file).is_none());
        assert!(!format!("{default:?}").contains('D'));
    }
}
