//! Where things live on a server: paths, systemd unit names and the two
//! web slots.

use std::fmt;

use crate::config::HarborConfig;

/// One of the two web slots. A new release starts in the idle slot next to
/// the live one, and traffic switches once it is healthy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// The first slot (`deploy.ports[0]`, 8081).
    Blue,
    /// The second slot (`deploy.ports[1]`, 8082).
    Green,
}

impl Slot {
    /// Both slots.
    pub const BOTH: [Slot; 2] = [Slot::Blue, Slot::Green];

    /// `blue` or `green`.
    pub fn as_str(self) -> &'static str {
        match self {
            Slot::Blue => "blue",
            Slot::Green => "green",
        }
    }

    /// The other slot.
    pub fn other(self) -> Slot {
        match self {
            Slot::Blue => Slot::Green,
            Slot::Green => Slot::Blue,
        }
    }

    /// 0 for blue, 1 for green: the index into `deploy.ports`.
    pub fn index(self) -> usize {
        match self {
            Slot::Blue => 0,
            Slot::Green => 1,
        }
    }

    /// Parses `blue` or `green`.
    pub fn parse(text: &str) -> Option<Slot> {
        match text.trim() {
            "blue" => Some(Slot::Blue),
            "green" => Some(Slot::Green),
            _ => None,
        }
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The paths and unit names of an app on its servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The app (package) name, which is also the binary's name.
    pub app: String,
    /// `/srv/<app>`: releases, links and `shared/`.
    pub base: String,
    /// `/etc/<app>`: the environment files.
    pub etc: String,
    /// The deploy user, who owns the app and runs its processes.
    pub user: String,
}

impl Layout {
    /// The layout of `config`'s app.
    pub fn new(config: &HarborConfig) -> Self {
        Self {
            app: config.app.clone(),
            base: config.deploy_path(),
            etc: format!("/etc/{}", config.app),
            user: config.ssh.user.clone(),
        }
    }

    /// `/srv/<app>/releases`.
    pub fn releases(&self) -> String {
        format!("{}/releases", self.base)
    }

    /// `/srv/<app>/releases/<id>`.
    pub fn release_dir(&self, id: &str) -> String {
        format!("{}/releases/{id}", self.base)
    }

    /// `/srv/<app>/current`.
    pub fn current(&self) -> String {
        format!("{}/current", self.base)
    }

    /// `/srv/<app>/shared`.
    pub fn shared(&self) -> String {
        format!("{}/shared", self.base)
    }

    /// `/srv/<app>/.harbor-live`: the live slot's name.
    pub fn live_marker(&self) -> String {
        format!("{}/.harbor-live", self.base)
    }

    /// `/etc/<app>/env`: the environment of every process.
    pub fn env_file(&self) -> String {
        format!("{}/env", self.etc)
    }

    /// `/etc/<app>/<slot>.env`: the web slot's address and release.
    pub fn slot_env(&self, slot: Slot) -> String {
        format!("{}/{slot}.env", self.etc)
    }

    /// `/etc/<app>/frontend-<slot>.env`: the front end slot's port and API.
    pub fn frontend_env(&self, slot: Slot) -> String {
        format!("{}/frontend-{slot}.env", self.etc)
    }

    /// `/etc/caddy/sites/<app>.caddy`: the app's site, rewritten on switch.
    pub fn caddy_site(&self) -> String {
        format!("/etc/caddy/sites/{}.caddy", self.app)
    }

    /// `/var/backups/postgresql/<app>`.
    pub fn backup_dir(&self) -> String {
        format!("/var/backups/postgresql/{}", self.app)
    }

    /// `<app>-web@<slot>`.
    pub fn web_unit(&self, slot: Slot) -> String {
        format!("{}-web@{slot}", self.app)
    }

    /// `<app>-frontend@<slot>`.
    pub fn frontend_unit(&self, slot: Slot) -> String {
        format!("{}-frontend@{slot}", self.app)
    }

    /// `<app>-worker@<n>`.
    pub fn worker_unit(&self, instance: u32) -> String {
        format!("{}-worker@{instance}", self.app)
    }

    /// `<app>-scheduler`.
    pub fn scheduler_unit(&self) -> String {
        format!("{}-scheduler", self.app)
    }

    /// `<app>-backup` (a oneshot service with a daily timer).
    pub fn backup_unit(&self) -> String {
        format!("{}-backup", self.app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_alternate() {
        assert_eq!(Slot::Blue.other(), Slot::Green);
        assert_eq!(Slot::Green.other(), Slot::Blue);
        assert_eq!(Slot::parse("green\n"), Some(Slot::Green));
        assert_eq!(Slot::parse(""), None);
        assert_eq!(Slot::Green.index(), 1);
        assert_eq!(Slot::Blue.to_string(), "blue");
    }

    #[test]
    fn names_follow_the_app() {
        let config = HarborConfig::parse(
            "app = \"crm-api\"\n[servers.a]\nhost = \"x\"\n",
            std::path::Path::new("harbor.toml"),
        )
        .unwrap();
        let layout = Layout::new(&config);
        assert_eq!(layout.base, "/srv/crm-api");
        assert_eq!(layout.release_dir("r1"), "/srv/crm-api/releases/r1");
        assert_eq!(layout.current(), "/srv/crm-api/current");
        assert_eq!(layout.env_file(), "/etc/crm-api/env");
        assert_eq!(layout.slot_env(Slot::Green), "/etc/crm-api/green.env");
        assert_eq!(layout.caddy_site(), "/etc/caddy/sites/crm-api.caddy");
        assert_eq!(layout.web_unit(Slot::Blue), "crm-api-web@blue");
        assert_eq!(layout.worker_unit(2), "crm-api-worker@2");
        assert_eq!(layout.scheduler_unit(), "crm-api-scheduler");
        assert_eq!(layout.backup_unit(), "crm-api-backup");
        assert_eq!(layout.user, "navree");
    }
}
