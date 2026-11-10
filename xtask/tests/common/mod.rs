//! A workspace described in a few lines, as `cargo metadata` would print it.

#![allow(dead_code)]

use serde_json::{Value, json};
use xtask::metadata::Workspace;

/// One member: its name, whether it is published, its version, and its
/// dependencies as (name, table, requirement). A dependency on a name that
/// is not a member is a registry dependency; on a member, a path one.
pub struct Member<'a> {
    pub name: &'a str,
    pub published: bool,
    pub version: &'a str,
    pub deps: &'a [(&'a str, &'a str, &'a str)],
}

pub fn member<'a>(name: &'a str, deps: &'a [(&'a str, &'a str, &'a str)]) -> Member<'a> {
    Member {
        name,
        published: true,
        version: "1.0.0",
        deps,
    }
}

pub fn workspace(members: &[Member<'_>]) -> Workspace {
    let names: Vec<&str> = members.iter().map(|m| m.name).collect();
    let packages: Vec<Value> = members
        .iter()
        .map(|m| {
            let deps: Vec<Value> = m
                .deps
                .iter()
                .map(|(name, kind, req)| {
                    let kind = match *kind {
                        "dev" => json!("dev"),
                        "build" => json!("build"),
                        _ => Value::Null,
                    };
                    let mut dep = json!({
                        "name": name,
                        "req": req,
                        "kind": kind,
                        "optional": false,
                    });
                    if names.contains(name) {
                        dep["path"] = json!(format!("/ws/crates/{name}"));
                    }
                    dep
                })
                .collect();
            json!({
                "name": m.name,
                "version": m.version,
                "id": format!("path+file:///ws/crates/{}#{}", m.name, m.version),
                "manifest_path": format!("/ws/crates/{}/Cargo.toml", m.name),
                "publish": if m.published { Value::Null } else { json!([]) },
                "dependencies": deps,
            })
        })
        .collect();
    let ids: Vec<Value> = packages.iter().map(|p| p["id"].clone()).collect();
    let metadata = json!({
        "packages": packages,
        "workspace_members": ids,
        "workspace_root": "/ws",
    });
    Workspace::from_json(&metadata.to_string()).expect("fixture metadata")
}

pub fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}
