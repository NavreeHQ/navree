//! The kit's static assets, embedded in the binary: one stylesheet, one
//! script, the favicon and the fonts (Geist and Monaspace Neon, SIL OFL).
//!
//! They are served under a versioned path, `{mount}/{version}/{file}`, with
//! `Cache-Control: public, max-age=31536000, immutable`. The version is a
//! hash of the files, so a new build of the kit gets new URLs.

use std::path::Path;
use std::sync::LazyLock;

/// Where [`UiModule`](crate::UiModule) serves the assets unless configured
/// otherwise (`ui.mount`).
pub const DEFAULT_MOUNT: &str = "/_navree/ui";

/// The stylesheet's file name.
pub const STYLESHEET: &str = "navree-ui.css";

/// The script's file name.
pub const SCRIPT: &str = "navree-ui.js";

/// One embedded file.
#[derive(Debug)]
pub struct Asset {
    /// The path under the versioned directory (`"fonts/geist.woff2"`).
    pub path: &'static str,
    /// The `Content-Type`.
    pub content_type: &'static str,
    /// The file's bytes.
    pub bytes: &'static [u8],
}

/// Every embedded file.
pub static ASSETS: &[Asset] = &[
    Asset {
        path: STYLESHEET,
        content_type: "text/css; charset=utf-8",
        bytes: include_bytes!("../assets/navree-ui.css"),
    },
    Asset {
        path: SCRIPT,
        content_type: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../assets/navree-ui.js"),
    },
    Asset {
        path: "favicon.svg",
        content_type: "image/svg+xml",
        bytes: include_bytes!("../assets/favicon.svg"),
    },
    Asset {
        path: "fonts/geist.woff2",
        content_type: "font/woff2",
        bytes: include_bytes!("../assets/fonts/geist.woff2"),
    },
    Asset {
        path: "fonts/monaspace-neon.woff2",
        content_type: "font/woff2",
        bytes: include_bytes!("../assets/fonts/monaspace-neon.woff2"),
    },
    Asset {
        path: "fonts/LICENSE-geist.txt",
        content_type: "text/plain; charset=utf-8",
        bytes: include_bytes!("../assets/fonts/LICENSE-geist.txt"),
    },
    Asset {
        path: "fonts/LICENSE-monaspace-neon.txt",
        content_type: "text/plain; charset=utf-8",
        bytes: include_bytes!("../assets/fonts/LICENSE-monaspace-neon.txt"),
    },
];

/// The embedded file at `path`, if any.
pub fn get(path: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.path == path)
}

/// The assets' version: the first 12 hex digits of a 64-bit FNV-1a hash of
/// every file's path and bytes.
pub fn version() -> &'static str {
    static VERSION: LazyLock<String> = LazyLock::new(|| {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for asset in ASSETS {
            for byte in asset
                .path
                .bytes()
                .chain([0])
                .chain(asset.bytes.iter().copied())
            {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
        format!("{hash:016x}")[..12].to_owned()
    });
    &VERSION
}

/// The versioned asset URL for `mount` (`"/_navree/ui"` gives
/// `"/_navree/ui/<version>"`): what pages put in `shell.assets`.
pub fn url(mount: &str) -> String {
    format!("{}/{}", mount.trim_end_matches('/'), version())
}

/// Writes every asset under `dir` (`dir/navree-ui.css`, `dir/fonts/...`),
/// for static HTML (the gallery's files) or a CDN upload.
pub fn write_all(dir: &Path) -> std::io::Result<()> {
    for asset in ASSETS {
        let path = dir.join(asset.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, asset.bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_stable_and_short() {
        assert_eq!(version().len(), 12);
        assert_eq!(version(), version());
        assert!(version().bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(url("/_navree/ui/"), format!("/_navree/ui/{}", version()));
    }

    #[test]
    fn every_asset_is_found_by_path() {
        for asset in ASSETS {
            assert!(std::ptr::eq(get(asset.path).unwrap(), asset));
            assert!(!asset.bytes.is_empty(), "{}", asset.path);
        }
        assert!(get("missing.css").is_none());
        assert!(get("../Cargo.toml").is_none());
    }
}
