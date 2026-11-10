//! The application key that `navree new` writes into the new app's `.env`.
//!
//! `.env.example` keeps `APP_KEY=` empty (it is committed); `.env` is this
//! machine's copy, so it gets a key of its own and the app signs and
//! encrypts from the first run.

use std::path::Path;

use anyhow::Context as _;

/// `env` with its empty `APP_KEY=` line holding `key`. The text is returned
/// as it is when it has no such line (a key that is already there stays).
pub(crate) fn with_app_key(env: &str, key: &str) -> String {
    // The pad lets the first line match like any other.
    let padded = format!("\n{env}");
    let filled = padded.replacen("\nAPP_KEY=\n", &format!("\nAPP_KEY={key}\n"), 1);
    filled[1..].to_owned()
}

/// Puts a new application key in the `.env` of the app at `root`. Returns
/// whether it did: `false` when the app has no `.env` or its `APP_KEY` is
/// not empty.
pub(crate) fn write_app_key(root: &Path) -> anyhow::Result<bool> {
    let path = root.join(".env");
    let Ok(env) = std::fs::read_to_string(&path) else {
        return Ok(false);
    };
    let key = navree_harbor::SecretKey::generate().encode();
    let filled = with_app_key(&env, &key);
    if filled == env {
        return Ok(false);
    }
    std::fs::write(&path, filled).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_line_gets_the_key_and_nothing_else_changes() {
        let env = "# comment\nAPP_KEY=\nAPP_MAIL__FROM=\"a <b@c.d>\"\n";
        assert_eq!(
            with_app_key(env, "base64:k"),
            "# comment\nAPP_KEY=base64:k\nAPP_MAIL__FROM=\"a <b@c.d>\"\n"
        );
        assert_eq!(with_app_key("APP_KEY=\n", "base64:k"), "APP_KEY=base64:k\n");
    }

    #[test]
    fn a_key_that_is_there_stays() {
        let env = "APP_KEY=base64:old\n";
        assert_eq!(with_app_key(env, "base64:new"), env);
        let commented = "# APP_KEY=\nAPP_NAME=x\n";
        assert_eq!(with_app_key(commented, "base64:new"), commented);
    }

    #[test]
    fn the_written_file_holds_a_valid_key_and_the_example_stays_empty() {
        let dir = tempfile::tempdir().expect("temporary directory");
        std::fs::write(dir.path().join(".env"), "APP_KEY=\nAPP_NAME=x\n").expect("write");
        std::fs::write(dir.path().join(".env.example"), "APP_KEY=\nAPP_NAME=x\n").expect("write");
        assert!(write_app_key(dir.path()).expect("written"));
        let env = std::fs::read_to_string(dir.path().join(".env")).expect("read");
        let key = env
            .lines()
            .find_map(|line| line.strip_prefix("APP_KEY="))
            .expect("APP_KEY");
        let encoded = key.strip_prefix("base64:").expect("base64: prefix");
        assert_eq!(encoded.len(), 44, "32 bytes in base64: {key}");
        assert!(env.ends_with("APP_NAME=x\n"));
        let example = std::fs::read_to_string(dir.path().join(".env.example")).expect("read");
        assert_eq!(example, "APP_KEY=\nAPP_NAME=x\n");
        // A second run finds the key set and leaves it alone.
        assert!(!write_app_key(dir.path()).expect("not written"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".env")).expect("read"),
            env
        );
    }

    #[test]
    fn an_app_without_dotenv_is_left_alone() {
        let dir = tempfile::tempdir().expect("temporary directory");
        assert!(!write_app_key(dir.path()).expect("nothing to write"));
    }
}
