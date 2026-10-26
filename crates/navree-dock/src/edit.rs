//! `navree dock:add` and `navree dock:remove`: edits of `dock.toml` that
//! keep its comments and layout.

use std::path::Path;

use toml_edit::{DocumentMut, Item, Table};

use crate::config::{DOCK_FILE, DockFile, package_name};
use crate::error::DockError;
use crate::service::Service;

/// What [`add_service`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddReport {
    /// The service.
    pub service: Service,
    /// Whether `dock.toml` was created.
    pub created: bool,
    /// Whether the service was already in the file (nothing changed).
    pub already_present: bool,
    /// Lines to print: what the app needs to use the service.
    pub hints: Vec<String>,
}

/// Adds `[services.<name>]` to `<root>/dock.toml`, creating the file when
/// needed. The rest of the file is kept as it is.
pub fn add_service(root: &Path, name: &str) -> Result<AddReport, DockError> {
    let service: Service = name.parse()?;
    let path = root.join(DOCK_FILE);
    let package = package_name(root);
    let (mut document, created) = match std::fs::read_to_string(&path) {
        Ok(text) => (parse_document(&text)?, false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let header = format!("# Local services for {package}. `navree dock up` starts them.\n");
            (parse_document(&header)?, true)
        }
        Err(error) => return Err(DockError::io(path, error)),
    };

    // A file of comments only keeps them as the document's trailing text,
    // which is written after every table: move them above the new one.
    let prefix = if document.as_table().is_empty() {
        let comments = document
            .trailing()
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned();
        document.set_trailing("");
        if comments.is_empty() {
            String::new()
        } else {
            format!("{comments}\n\n")
        }
    } else {
        "\n".to_owned()
    };
    let services = services_table(&mut document)?;
    if services.contains_key(service.name()) {
        return Ok(AddReport {
            service,
            created: false,
            already_present: true,
            hints: Vec::new(),
        });
    }
    let mut table = Table::new();
    table.decor_mut().set_prefix(prefix);
    services.insert(service.name(), Item::Table(table));

    let text = document.to_string();
    // The result must still be a valid dock.toml.
    DockFile::parse(&text, &package)?;
    std::fs::write(&path, &text).map_err(|e| DockError::io(&path, e))?;

    let mut hints = Vec::new();
    if service == Service::Postgres && !uses_postgres_feature(root) {
        hints.extend(service.add_hint().map(str::to_owned));
    }
    Ok(AddReport {
        service,
        created,
        already_present: false,
        hints,
    })
}

/// Removes `[services.<name>]` from `<root>/dock.toml`. Errors when the
/// service is not there.
pub fn remove_service(root: &Path, name: &str) -> Result<Service, DockError> {
    let service: Service = name.parse()?;
    let path = root.join(DOCK_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(DockError::NoDockFile(root.to_path_buf()));
        }
        Err(error) => return Err(DockError::io(path, error)),
    };
    let mut document = parse_document(&text)?;
    let services = services_table(&mut document)?;
    if services.remove(service.name()).is_none() {
        return Err(DockError::NotInDockFile(name.to_owned()));
    }
    // The next table's blank line may now open the file.
    let text = document.to_string();
    let text = text.trim_start_matches(['\r', '\n']);
    std::fs::write(&path, text).map_err(|e| DockError::io(&path, e))?;
    Ok(service)
}

fn parse_document(text: &str) -> Result<DocumentMut, DockError> {
    text.parse()
        .map_err(|e: toml_edit::TomlError| DockError::config(e.to_string().trim().to_owned()))
}

/// The `services` table, created (as an implicit table, so no bare
/// `[services]` header is written) when missing.
fn services_table(document: &mut DocumentMut) -> Result<&mut Table, DockError> {
    let item = document.entry("services").or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(true);
        Item::Table(table)
    });
    item.as_table_mut().ok_or_else(|| {
        DockError::config("`services` must be a table of services, such as [services.postgres]")
    })
}

/// Whether the app's `Cargo.toml` turns on navree's (or navree-db's)
/// `postgres` feature.
fn uses_postgres_feature(root: &Path) -> bool {
    let Some(manifest) = std::fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
    else {
        return false;
    };
    let Some(dependencies) = manifest.get("dependencies").and_then(toml::Value::as_table) else {
        return false;
    };
    ["navree", "navree-db"].iter().any(|krate| {
        dependencies
            .get(*krate)
            .and_then(|dep| dep.get("features"))
            .and_then(toml::Value::as_array)
            .is_some_and(|features| features.iter().any(|f| f.as_str() == Some("postgres")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"crm-api\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { path = \"../navree\" }\n",
        )
        .unwrap();
        dir
    }

    fn read(dir: &tempfile::TempDir) -> String {
        std::fs::read_to_string(dir.path().join(DOCK_FILE)).unwrap()
    }

    #[test]
    fn adding_creates_the_file_then_appends() {
        let dir = app();
        let report = add_service(dir.path(), "postgres").unwrap();
        assert!(report.created && !report.already_present);
        assert_eq!(
            report.hints,
            ["Your app needs navree's `postgres` feature."]
        );
        assert_eq!(
            read(&dir),
            "# Local services for crm-api. `navree dock up` starts them.\n\n[services.postgres]\n"
        );
        let report = add_service(dir.path(), "mailpit").unwrap();
        assert!(!report.created && report.hints.is_empty());
        assert_eq!(
            read(&dir),
            "# Local services for crm-api. `navree dock up` starts them.\n\n[services.postgres]\n\n[services.mailpit]\n"
        );
        let again = add_service(dir.path(), "postgres").unwrap();
        assert!(again.already_present);
        let file = DockFile::load(dir.path()).unwrap();
        assert_eq!(file.services.len(), 2);
    }

    #[test]
    fn adding_keeps_comments_and_settings() {
        let dir = app();
        let original = "# Our services.\nproject = \"crm\" # short names\n\n[services.postgres]\nversion = \"16\" # matches production\n";
        std::fs::write(dir.path().join(DOCK_FILE), original).unwrap();
        add_service(dir.path(), "redis").unwrap();
        assert_eq!(read(&dir), format!("{original}\n[services.redis]\n"));
    }

    #[test]
    fn a_file_of_comments_keeps_them_on_top() {
        let dir = app();
        std::fs::write(
            dir.path().join(DOCK_FILE),
            "# Team services.\n# Ask before adding.\n",
        )
        .unwrap();
        add_service(dir.path(), "nats").unwrap();
        assert_eq!(
            read(&dir),
            "# Team services.\n# Ask before adding.\n\n[services.nats]\n"
        );
        let empty = app();
        std::fs::write(empty.path().join(DOCK_FILE), "").unwrap();
        add_service(empty.path(), "nats").unwrap();
        assert_eq!(read(&empty), "[services.nats]\n");
    }

    #[test]
    fn the_postgres_hint_is_skipped_when_the_feature_is_on() {
        let dir = app();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"crm-api\"\n\n[dependencies]\nnavree = { path = \"../navree\", features = [\"postgres\"] }\n",
        )
        .unwrap();
        assert!(
            add_service(dir.path(), "postgres")
                .unwrap()
                .hints
                .is_empty()
        );
    }

    #[test]
    fn unknown_services_and_broken_files_are_errors() {
        let dir = app();
        assert!(matches!(
            add_service(dir.path(), "mysql"),
            Err(DockError::UnknownService(_))
        ));
        std::fs::write(dir.path().join(DOCK_FILE), "services = \"none\"\n").unwrap();
        let error = add_service(dir.path(), "redis").unwrap_err();
        assert!(
            error.to_string().contains("`services` must be a table"),
            "{error}"
        );
        std::fs::write(dir.path().join(DOCK_FILE), "[services.redis]\nbucket = 1\n").unwrap();
        let error = add_service(dir.path(), "nats").unwrap_err();
        assert!(
            error.to_string().contains("unknown key `bucket`"),
            "{error}"
        );
        assert_eq!(read(&dir), "[services.redis]\nbucket = 1\n");
    }

    #[test]
    fn removing_drops_the_table_and_keeps_the_rest() {
        let dir = app();
        std::fs::write(
            dir.path().join(DOCK_FILE),
            "# Services.\n\n[services.postgres]\nversion = \"17\"\n\n[services.redis] # cache\nmaxmemory_mb = 64\n",
        )
        .unwrap();
        assert_eq!(remove_service(dir.path(), "redis").unwrap(), Service::Redis);
        assert_eq!(
            read(&dir),
            "# Services.\n\n[services.postgres]\nversion = \"17\"\n"
        );
        let error = remove_service(dir.path(), "redis").unwrap_err();
        assert_eq!(
            error.to_string(),
            "redis is not in dock.toml; add it with `navree dock:add redis`"
        );
        let empty = tempfile::tempdir().unwrap();
        assert!(matches!(
            remove_service(empty.path(), "redis"),
            Err(DockError::NoDockFile(_))
        ));
        std::fs::write(
            dir.path().join(DOCK_FILE),
            "[services.redis]\n\n[services.nats]\n",
        )
        .unwrap();
        remove_service(dir.path(), "redis").unwrap();
        assert_eq!(read(&dir), "[services.nats]\n");
        remove_service(dir.path(), "nats").unwrap();
        assert_eq!(read(&dir), "");
    }
}
