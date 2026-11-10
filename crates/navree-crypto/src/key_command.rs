//! The `key:generate` command: print a new application key, or write it to
//! the `.env` file of the app.

use std::path::Path;

use navree_config::set_dotenv_value;

use crate::generate_key;

/// The entry of the `.env` file that holds the application key.
const ENTRY: &str = "APP_KEY";

const USAGE: &str = "usage: key:generate [--write [--force]]";

/// What `key:generate` was asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyArgs {
    /// Write the key to `.env` instead of printing it.
    write: bool,
    /// With `write`: replace a key that is already set.
    force: bool,
}

impl KeyArgs {
    /// Reads the arguments after `key:generate`.
    pub(crate) fn parse(args: &[String]) -> Result<Self, String> {
        let mut parsed = KeyArgs::default();
        for arg in args {
            match arg.as_str() {
                "--write" => parsed.write = true,
                "--force" => parsed.force = true,
                other => return Err(format!("unknown argument `{other}`; {USAGE}")),
            }
        }
        if parsed.force && !parsed.write {
            return Err(format!("`--force` goes with `--write`; {USAGE}"));
        }
        Ok(parsed)
    }
}

/// Runs the command against the `.env` file at `dotenv`, and returns what to
/// print: the new key, or a line saying where it was written. The key is
/// never printed when it is written.
pub(crate) fn run(args: &[String], dotenv: &Path) -> Result<String, String> {
    let args = KeyArgs::parse(args)?;
    let key = generate_key();
    if !args.write {
        return Ok(key);
    }
    let existing = match std::fs::read_to_string(dotenv) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("cannot read {}: {error}", dotenv.display())),
    };
    let updated = set_dotenv_value(&existing, ENTRY, &key, args.force).map_err(|_| {
        format!(
            "{ENTRY} is already set in {}. A new key cannot read what the old one encrypted: \
             move the old key to `app.previous_keys` (APP_PREVIOUS_KEYS) first, then run \
             `key:generate --write --force`.",
            dotenv.display()
        )
    })?;
    std::fs::write(dotenv, updated)
        .map_err(|error| format!("cannot write {}: {error}", dotenv.display()))?;
    Ok(format!("{ENTRY} written to {}.", dotenv.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    /// A path in the temporary directory that no other test uses.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("navree-key-command-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temporary directory");
        dir.join(".env")
    }

    #[test]
    fn without_write_the_key_is_printed_and_nothing_is_written() {
        let path = scratch("print");
        let _ = std::fs::remove_file(&path);
        let printed = run(&[], &path).expect("runs");
        assert!(printed.starts_with("base64:"), "{printed}");
        assert!(!path.exists());
    }

    #[test]
    fn write_fills_an_empty_entry_and_does_not_print_the_key() {
        let path = scratch("fill");
        std::fs::write(&path, "# keys\nAPP_KEY=\nAPP_NAME=agra\n").expect("write");
        let printed = run(&args(&["--write"]), &path).expect("runs");
        assert!(!printed.contains("base64:"), "{printed}");
        let text = std::fs::read_to_string(&path).expect("read");
        let entries = navree_config::parse_dotenv(&text).expect("valid .env");
        let key = entries
            .iter()
            .find(|(name, _)| name == "APP_KEY")
            .map(|(_, value)| value.as_str())
            .expect("APP_KEY");
        crate::AppKey::parse(key).expect("a valid key");
        assert!(text.starts_with("# keys\n"), "{text}");
        assert!(text.ends_with("APP_NAME=agra\n"), "{text}");
    }

    #[test]
    fn write_creates_the_file_when_there_is_none() {
        let path = scratch("create");
        let _ = std::fs::remove_file(&path);
        run(&args(&["--write"]), &path).expect("runs");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.starts_with("APP_KEY=base64:"), "{text}");
    }

    #[test]
    fn write_refuses_to_replace_a_key_without_force() {
        let path = scratch("refuse");
        std::fs::write(&path, "APP_KEY=base64:old\n").expect("write");
        let error = run(&args(&["--write"]), &path).expect_err("already set");
        assert!(error.contains("--force"), "{error}");
        assert!(error.contains("previous_keys"), "{error}");
        assert!(!error.contains("base64:old"), "{error}");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "APP_KEY=base64:old\n"
        );
        run(&args(&["--write", "--force"]), &path).expect("force replaces");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.starts_with("APP_KEY=base64:"), "{text}");
        assert!(!text.contains("base64:old"), "{text}");
    }

    #[test]
    fn bad_arguments_are_reported() {
        let path = scratch("bad");
        assert!(
            run(&args(&["--force"]), &path)
                .expect_err("force alone")
                .contains("--write")
        );
        assert!(
            run(&args(&["--wite"]), &path)
                .expect_err("typo")
                .contains("unknown argument `--wite`")
        );
    }
}
