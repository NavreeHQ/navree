//! `navree browser:install` against a local server serving a small
//! Chrome for Testing archive.

use std::io::Write;
use std::path::Path;

use navree_cli::browser::{InstallOptions, binary_in, install};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A zip shaped like `chrome-<platform>.zip`.
fn archive(platform: &str, with_binary: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);
    let root = format!("chrome-{platform}");
    zip.add_directory(format!("{root}/"), options).unwrap();
    if with_binary {
        let binary = binary_in(Path::new(""), platform);
        let name = binary.to_string_lossy().replace('\\', "/");
        zip.start_file(name, options).unwrap();
        zip.write_all(b"#!/bin/sh\necho chrome\n").unwrap();
    }
    zip.start_file(format!("{root}/resources.pak"), options).unwrap();
    zip.write_all(&[7u8; 4096]).unwrap();
    zip.finish().unwrap().into_inner()
}

fn options(server: &MockServer, cache: &Path, version: &str, force: bool) -> InstallOptions {
    InstallOptions {
        version: version.to_owned(),
        platform: "linux64".to_owned(),
        cache: cache.to_path_buf(),
        base_url: server.uri(),
        force,
    }
}

#[tokio::test]
async fn it_downloads_and_unpacks_into_the_cache_once() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/156.0.8078.47/linux64/chrome-linux64.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive("linux64", true)))
        .expect(2)
        .mount(&server)
        .await;
    let cache = tempfile::tempdir().unwrap();
    let opts = options(&server, cache.path(), "156.0.8078.47", false);

    let mut out = Vec::new();
    let binary = install(&opts, &mut out).await.unwrap();
    assert_eq!(
        binary,
        cache
            .path()
            .join("chrome/156.0.8078.47/chrome-linux64/chrome")
    );
    assert!(binary.is_file());
    assert!(cache.path().join("chrome/156.0.8078.47/chrome-linux64/resources.pak").is_file());
    let printed = String::from_utf8(out).unwrap();
    assert!(
        printed.contains("Downloaded Chrome for Testing 156.0.8078.47 (linux64) to "),
        "{printed}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&binary).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "the browser is executable");
    }
    // No download files are left behind.
    let leftovers: Vec<_> = std::fs::read_dir(cache.path().join("chrome"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["156.0.8078.47"]);

    // Installed: nothing to download.
    let mut out = Vec::new();
    install(&opts, &mut out).await.unwrap();
    assert!(String::from_utf8(out).unwrap().contains("is already installed"));

    // --force downloads again.
    let forced = options(&server, cache.path(), "156.0.8078.47", true);
    let mut out = Vec::new();
    install(&forced, &mut out).await.unwrap();
    assert!(String::from_utf8(out).unwrap().contains("Downloaded"));
}

#[tokio::test]
async fn a_version_the_server_does_not_have_fails_with_the_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let cache = tempfile::tempdir().unwrap();
    let opts = options(&server, cache.path(), "1.2.3.4", false);
    let error = install(&opts, &mut Vec::new()).await.unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("/1.2.3.4/linux64/chrome-linux64.zip"), "{message}");
    assert!(message.contains("404"), "{message}");
    assert!(!cache.path().join("chrome/1.2.3.4").exists());
}

#[tokio::test]
async fn a_broken_archive_leaves_nothing_behind() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/9.9.9.9/linux64/chrome-linux64.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"not a zip".to_vec()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/8.8.8.8/linux64/chrome-linux64.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive("linux64", false)))
        .mount(&server)
        .await;
    let cache = tempfile::tempdir().unwrap();

    let error = install(&options(&server, cache.path(), "9.9.9.9", false), &mut Vec::new())
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("is not a valid archive"), "{error:#}");

    let error = install(&options(&server, cache.path(), "8.8.8.8", false), &mut Vec::new())
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("has no chrome-linux64/ browser"), "{error:#}");

    let left: Vec<_> = std::fs::read_dir(cache.path().join("chrome"))
        .unwrap()
        .flatten()
        .collect();
    assert!(left.is_empty(), "{left:?}");
}
