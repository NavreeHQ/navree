//! `navree browser:install`: downloads a pinned Chrome for Testing build
//! into Navree's cache, where navree-browser finds it.
//!
//! The cache is `NAVREE_CACHE_DIR`, else `%LOCALAPPDATA%\navree` (Windows),
//! `~/Library/Caches/navree` (macOS) or `$XDG_CACHE_HOME/navree`,
//! `~/.cache/navree` (Linux); the browser goes in `chrome/<version>/`, as
//! unpacked from Chrome for Testing's `chrome-<platform>.zip`. navree-browser
//! looks there with the same rules.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

/// The Chrome for Testing version `navree browser:install` downloads.
pub const CHROME_VERSION: &str = "156.0.8078.47";

/// Where Chrome for Testing builds are downloaded from; the environment
/// variable `NAVREE_BROWSER_DOWNLOAD_URL` replaces it (a mirror).
pub const DOWNLOAD_URL: &str = "https://storage.googleapis.com/chrome-for-testing-public";

/// What to install and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallOptions {
    /// The Chrome for Testing version.
    pub version: String,
    /// `linux64`, `mac-arm64`, `mac-x64`, `win64` or `win32`.
    pub platform: String,
    /// Navree's cache directory.
    pub cache: PathBuf,
    /// The download base URL.
    pub base_url: String,
    /// Download even if the version is installed.
    pub force: bool,
}

impl InstallOptions {
    /// The options for this machine: the pinned (or given) version, this
    /// platform, the cache directory and download URL from the environment.
    pub fn for_this_machine(version: Option<&str>, force: bool) -> anyhow::Result<InstallOptions> {
        let env = |name: &str| std::env::var_os(name);
        let platform =
            platform(std::env::consts::OS, std::env::consts::ARCH).with_context(|| {
                format!(
                    "Chrome for Testing has no build for {} {}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                )
            })?;
        let cache = cache_dir(std::env::consts::OS, &env).context(
            "cannot find a cache directory: set NAVREE_CACHE_DIR (or HOME / LOCALAPPDATA)",
        )?;
        let base_url = std::env::var("NAVREE_BROWSER_DOWNLOAD_URL")
            .ok()
            .filter(|url| !url.trim().is_empty())
            .unwrap_or_else(|| DOWNLOAD_URL.to_owned());
        Ok(InstallOptions {
            version: version.unwrap_or(CHROME_VERSION).to_owned(),
            platform: platform.to_owned(),
            cache,
            base_url,
            force,
        })
    }

    /// `<cache>/chrome/<version>`.
    pub fn dir(&self) -> PathBuf {
        self.cache.join("chrome").join(&self.version)
    }

    /// The archive's URL: `<base>/<version>/<platform>/chrome-<platform>.zip`.
    pub fn url(&self) -> String {
        format!(
            "{}/{}/{}/chrome-{}.zip",
            self.base_url.trim_end_matches('/'),
            self.version,
            self.platform,
            self.platform
        )
    }

    /// The browser binary inside [`dir`](Self::dir).
    pub fn binary(&self) -> PathBuf {
        binary_in(&self.dir(), &self.platform)
    }
}

/// Chrome for Testing's platform name for an OS and architecture
/// (`std::env::consts`).
pub fn platform(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some("linux64"),
        ("macos", "aarch64") => Some("mac-arm64"),
        ("macos", "x86_64") => Some("mac-x64"),
        ("windows", "x86_64") => Some("win64"),
        ("windows", "x86") => Some("win32"),
        _ => None,
    }
}

/// Navree's cache directory for `os` (`std::env::consts::OS`).
pub fn cache_dir(os: &str, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |name: &str| env(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("NAVREE_CACHE_DIR") {
        return Some(dir);
    }
    match os {
        "windows" => var("LOCALAPPDATA").map(|d| d.join("navree")),
        "macos" => var("HOME").map(|h| h.join("Library/Caches/navree")),
        _ => var("XDG_CACHE_HOME")
            .or_else(|| var("HOME").map(|h| h.join(".cache")))
            .map(|d| d.join("navree")),
    }
}

/// The browser binary of an unpacked `chrome-<platform>.zip` in `dir`.
pub fn binary_in(dir: &Path, platform: &str) -> PathBuf {
    let unpacked = dir.join(format!("chrome-{platform}"));
    if platform.starts_with("win") {
        unpacked.join("chrome.exe")
    } else if platform.starts_with("mac") {
        unpacked.join("Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing")
    } else {
        unpacked.join("chrome")
    }
}

/// Downloads and unpacks Chrome for Testing unless it is installed (or
/// `force`); returns the browser binary.
pub async fn install(options: &InstallOptions, out: &mut dyn Write) -> anyhow::Result<PathBuf> {
    let dir = options.dir();
    let binary = options.binary();
    let label = format!(
        "Chrome for Testing {} ({})",
        options.version, options.platform
    );
    if binary.is_file() && !options.force {
        writeln!(out, "{label} is already installed in {}", shown(&dir))?;
        return Ok(binary);
    }
    writeln!(out, "Downloading {label} from {}", options.url())?;
    let root = options.cache.join("chrome");
    std::fs::create_dir_all(&root).with_context(|| format!("cannot create {}", root.display()))?;
    let archive = root.join(format!(".{}-{}.zip", options.version, std::process::id()));
    let downloaded = download(&options.url(), &archive).await;
    let partial = root.join(format!(
        ".{}.partial-{}",
        options.version,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&partial);
    let unpacked = downloaded.and_then(|()| {
        unpack(&archive, &partial)
            .map_err(|error| error.context(format!("{} is not a valid archive", options.url())))
    });
    let _ = std::fs::remove_file(&archive);
    if let Err(error) = unpacked {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(error);
    }
    if !binary_in(&partial, &options.platform).is_file() {
        let _ = std::fs::remove_dir_all(&partial);
        bail!(
            "the archive from {} has no chrome-{}/ browser",
            options.url(),
            options.platform
        );
    }
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("cannot replace {}", dir.display()))?;
    }
    std::fs::rename(&partial, &dir)
        .with_context(|| format!("cannot move the browser to {}", dir.display()))?;
    writeln!(out, "Downloaded {label} to {}", shown(&dir))?;
    Ok(binary)
}

/// Streams `url` into the file `to`.
async fn download(url: &str, to: &Path) -> anyhow::Result<()> {
    // reqwest is built without a default crypto provider: use ring. An
    // error only means a process-wide provider is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .build()
        .context("cannot build the HTTP client")?;
    let mut response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("cannot download {url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("cannot download {url}: the server answered {status}");
    }
    let mut file =
        std::fs::File::create(to).with_context(|| format!("cannot write {}", to.display()))?;
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("the download of {url} was cut off"))?
    {
        file.write_all(&chunk)
            .with_context(|| format!("cannot write {}", to.display()))?;
    }
    file.flush()?;
    Ok(())
}

/// Unpacks the zip archive `archive` into `dir`, keeping Unix permissions.
fn unpack(archive: &Path, dir: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    zip.extract(dir)?;
    Ok(())
}

/// `path` with the home directory shown as `~`.
fn shown(path: &Path) -> String {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match home.map(PathBuf::from) {
        Some(home) if !home.as_os_str().is_empty() && path.starts_with(&home) => {
            let rest = path.strip_prefix(&home).unwrap_or(path);
            format!("~/{}", rest.to_string_lossy().replace('\\', "/"))
        }
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn platforms_follow_chrome_for_testing() {
        assert_eq!(platform("linux", "x86_64"), Some("linux64"));
        assert_eq!(platform("macos", "aarch64"), Some("mac-arm64"));
        assert_eq!(platform("macos", "x86_64"), Some("mac-x64"));
        assert_eq!(platform("windows", "x86_64"), Some("win64"));
        assert_eq!(platform("windows", "x86"), Some("win32"));
        assert_eq!(platform("linux", "aarch64"), None);
    }

    #[test]
    fn the_cache_dir_matches_navree_browser() {
        let env = env_of(&[("HOME", "/home/ada")]);
        assert_eq!(
            cache_dir("linux", &env),
            Some(PathBuf::from("/home/ada/.cache/navree"))
        );
        let env = env_of(&[("HOME", "/home/ada"), ("XDG_CACHE_HOME", "/xdg")]);
        assert_eq!(cache_dir("linux", &env), Some(PathBuf::from("/xdg/navree")));
        assert_eq!(
            cache_dir("macos", &env),
            Some(PathBuf::from("/home/ada/Library/Caches/navree"))
        );
        let env = env_of(&[("LOCALAPPDATA", r"C:\Users\ada\AppData\Local")]);
        assert_eq!(
            cache_dir("windows", &env),
            Some(PathBuf::from(r"C:\Users\ada\AppData\Local").join("navree"))
        );
        let env = env_of(&[("NAVREE_CACHE_DIR", "/ci/cache"), ("HOME", "/home/ada")]);
        assert_eq!(cache_dir("linux", &env), Some(PathBuf::from("/ci/cache")));
        assert_eq!(cache_dir("linux", &env_of(&[])), None);
    }

    #[test]
    fn urls_and_binaries_follow_the_archive_layout() {
        let options = InstallOptions {
            version: CHROME_VERSION.to_owned(),
            platform: "linux64".to_owned(),
            cache: PathBuf::from("/c"),
            base_url: format!("{DOWNLOAD_URL}/"),
            force: false,
        };
        assert_eq!(
            options.url(),
            format!("{DOWNLOAD_URL}/{CHROME_VERSION}/linux64/chrome-linux64.zip")
        );
        assert_eq!(
            options.binary(),
            PathBuf::from("/c/chrome")
                .join(CHROME_VERSION)
                .join("chrome-linux64/chrome")
        );
        assert!(binary_in(Path::new("/d"), "win64").ends_with("chrome-win64/chrome.exe"));
        assert!(binary_in(Path::new("/d"), "mac-arm64").ends_with(
            "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
        ));
    }
}
