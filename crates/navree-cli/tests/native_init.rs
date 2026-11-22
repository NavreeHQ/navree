//! `packages/apple/Starter` is what `navree native:init` writes for one app,
//! Dukaan, against the store demo on this machine and the package in its
//! parent folder: the Apple job of `native.yml` builds it on a Mac (without
//! building the CLI there), so the templates are built on every change to the
//! Apple client.
//!
//! After a change to the templates, write the folder again with
//! `NAVREE_WRITE_STARTER=1 cargo test -p navree-cli --test native_init`.

use std::path::PathBuf;

use navree_cli::native_init::{AppleProject, PackageSource, plan};

fn starter() -> AppleProject {
    AppleProject {
        display_name: "Dukaan".to_owned(),
        type_name: "Dukaan".to_owned(),
        scheme: "dukaan".to_owned(),
        origin: "http://127.0.0.1:8095".to_owned(),
        loopback: true,
        domain: String::new(),
        bundle_id: "com.navree.dukaan".to_owned(),
        bundle_id_assumed: false,
        team: String::new(),
        package: PackageSource::Local {
            path: "..".to_owned(),
        },
    }
}

fn folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("packages")
        .join("apple")
        .join("Starter")
}

#[test]
fn the_starter_is_what_native_init_writes() {
    let files = plan(&starter()).expect("the templates render");
    let folder = folder();
    if std::env::var_os("NAVREE_WRITE_STARTER").is_some() {
        for file in files.iter() {
            let path = navree_cli::scaffold::join(&folder, &file.path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("the folder");
            std::fs::write(&path, &file.contents).expect("the file");
        }
    }
    for file in files.iter() {
        let path = navree_cli::scaffold::join(&folder, &file.path);
        let on_disk = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .replace('\r', "");
        assert_eq!(
            on_disk, file.contents,
            "packages/apple/Starter/{} is not what native:init writes: run NAVREE_WRITE_STARTER=1 cargo test -p navree-cli --test native_init",
            file.path
        );
    }
}
