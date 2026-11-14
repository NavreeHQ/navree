//! The last tree of each address: shown at once while a fresh one is fetched.
//!
//! The core keeps a bounded number of answers in memory, as they came. When
//! the app's build says so, it also hands the client to keep between launches,
//! under keys the client need not understand, the shell and the list of pages
//! kept (`boot`) and each page whose answer said it may be kept
//! ([`navree_native::KEEP_HEADER`], sent when the page's `Cache-Control` does
//! not say `no-store`) under `page:<address>`. An answer that does not say so
//! removes what was kept for its address.
//!
//! A kept tree is only ever a head start: it is marked as kept, nothing is
//! sent from it, and the fresh answer takes its place. A page kept on the
//! device is drawn only once this launch's shell has answered over the network
//! and named the person the kept shell named; until then it is not even read.
//! A write, a new build, a change of person and a sign-out empty what is kept.

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::effect::Effect;
use crate::state::{Content, ScreenId};

/// The answers kept in memory, and what the client keeps.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    /// The last answer of each address, the newest last: address, status, body.
    pages: Vec<(String, u16, String)>,
    /// The addresses the client keeps a page for, the newest last.
    pub disk: Vec<String>,
    /// The shell as it came, for the client to keep.
    pub shell: Option<String>,
    /// This launch's shell answered over the network, and what is kept on
    /// the device is the person's it named: a kept page may be drawn.
    pub confirmed: bool,
}

#[derive(Serialize, Deserialize)]
struct Boot {
    shell: String,
    #[serde(default)]
    pages: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Page {
    status: u16,
    body: String,
}

impl Cache {
    fn get(&self, url: &str) -> Option<(u16, &str)> {
        self.pages
            .iter()
            .find(|(kept, _, _)| kept == url)
            .map(|(_, status, body)| (*status, body.as_str()))
    }

    fn put(&mut self, url: &str, status: u16, body: String, limit: usize) {
        self.pages.retain(|(kept, _, _)| kept != url);
        self.pages.push((url.to_owned(), status, body));
        if self.pages.len() > limit {
            self.pages.remove(0);
        }
    }
}

/// The person a kept shell names, as the core compares people.
fn person_of(shell: &str) -> Option<(String, Option<String>)> {
    let shell = serde_json::from_str::<navree_native::shell::Shell>(shell).ok()?;
    shell.user.map(|user| (user.name, user.email))
}

impl Core {
    fn boot(&mut self) {
        if !self.config.cache.persist {
            return;
        }
        let Some(shell) = self.inner.cache.shell.clone() else {
            return;
        };
        let boot = Boot {
            shell,
            pages: self.inner.cache.disk.clone(),
        };
        self.effects.push(Effect::CacheWrite {
            key: "boot".to_owned(),
            value: serde_json::to_string(&boot).ok(),
        });
    }

    /// Keeps the shell for the next launch.
    pub(crate) fn remember_shell(&mut self, body: String) {
        if self.inner.cache.shell.as_deref() != Some(&body) {
            self.inner.cache.shell = Some(body);
            self.boot();
        }
    }

    /// Keeps the answer a page gave: in memory, and on the device when the
    /// answer said it may be kept (`keep`). An answer that did not say so
    /// takes away what the device kept for its address.
    pub(crate) fn remember(&mut self, url: &str, status: u16, body: &str, keep: bool) {
        let limit = self.config.cache.pages;
        if limit == 0 || status >= 400 {
            return;
        }
        self.inner.cache.put(url, status, body.to_owned(), limit);
        if !self.config.cache.persist {
            return;
        }
        if !keep {
            let disk = &mut self.inner.cache.disk;
            if disk.iter().any(|kept| kept == url) {
                disk.retain(|kept| kept != url);
                self.effects.push(Effect::CacheWrite {
                    key: format!("page:{url}"),
                    value: None,
                });
                self.boot();
            }
            return;
        }
        let page = Page {
            status,
            body: body.to_owned(),
        };
        self.effects.push(Effect::CacheWrite {
            key: format!("page:{url}"),
            value: serde_json::to_string(&page).ok(),
        });
        let disk = &mut self.inner.cache.disk;
        let listed = disk.last().is_some_and(|last| last == url);
        disk.retain(|kept| kept != url);
        disk.push(url.to_owned());
        let dropped = (disk.len() > limit).then(|| disk.remove(0));
        if let Some(dropped) = dropped {
            self.effects.push(Effect::CacheWrite {
                key: format!("page:{dropped}"),
                value: None,
            });
        }
        if !listed {
            self.boot();
        }
    }

    /// Empties the pages kept: something was written, or the build changed.
    pub(crate) fn forget_pages(&mut self) {
        let had = !self.inner.cache.disk.is_empty();
        self.inner.cache.pages.clear();
        self.inner.cache.disk.clear();
        if had && self.config.cache.persist {
            self.effects.push(Effect::CacheClear);
            self.boot();
        }
    }

    /// Empties everything kept: the person changed, or signed out.
    pub(crate) fn forget_all(&mut self) {
        self.inner.cache = Cache::default();
        if self.config.cache.persist {
            self.effects.push(Effect::CacheClear);
        }
    }

    /// Gives a new screen the kept tree of its address, when there is one, or
    /// asks the client for it.
    pub(crate) fn head_start(&mut self, id: ScreenId, url: &str) {
        if let Some((status, body)) = self.inner.cache.get(url) {
            let body = body.to_owned();
            self.show_kept(id, status, &body);
        } else if self.inner.cache.confirmed {
            self.read_kept(url);
        }
    }

    /// Asks the client for the page it kept for `url`, when it keeps one.
    fn read_kept(&mut self, url: &str) {
        if self.config.cache.persist && self.inner.cache.disk.iter().any(|kept| kept == url) {
            self.effects.push(Effect::CacheRead {
                key: format!("page:{url}"),
            });
        }
    }

    /// This launch's shell answered over the network and the person it names
    /// is the one what is kept belongs to (another, or nobody, emptied it
    /// first): the screens still waiting for their first tree ask for theirs.
    pub(crate) fn shell_confirmed(&mut self) {
        if std::mem::replace(&mut self.inner.cache.confirmed, true) {
            return;
        }
        let waiting: Vec<String> = self
            .view
            .screens
            .values()
            .filter(|screen| screen.loading && matches!(screen.content, Content::Blank))
            .map(|screen| screen.url.clone())
            .collect();
        for url in waiting {
            self.read_kept(&url);
        }
    }

    fn show_kept(&mut self, id: ScreenId, status: u16, body: &str) {
        let Ok(read) = crate::decode::envelope(body, &self.config) else {
            return;
        };
        let (Some(screen), true) = (read.envelope.screen, read.missing.is_empty()) else {
            return;
        };
        let Some(view) = self.view.screens.get(&id) else {
            return;
        };
        if !matches!(view.content, Content::Blank) {
            return;
        }
        let url = view.url.clone();
        self.show_native(id, &url, screen, status, true);
    }

    /// What the client kept under `key` has been read.
    pub(crate) fn cache_loaded(&mut self, key: &str, value: Option<String>) {
        let Some(value) = value else {
            return;
        };
        if key == "boot" {
            let Ok(boot) = serde_json::from_str::<Boot>(&value) else {
                return;
            };
            if !self.inner.has_shell {
                self.inner.cache.disk = boot.pages;
                self.shell_from_cache(&boot.shell);
            } else if !self.inner.cache.confirmed {
                // A shell that is no person's word (an answer with none) is
                // not a reason to draw what is kept, nor to throw it away.
                self.inner.cache.disk = boot.pages;
            } else if person_of(&boot.shell) == self.inner.user {
                // The shell that came over the network is the newer one, and
                // it names the person the pages kept are for.
                let limit = self.config.cache.pages.max(1);
                let mut pages = boot.pages;
                for url in std::mem::take(&mut self.inner.cache.disk) {
                    pages.retain(|kept| *kept != url);
                    pages.push(url);
                }
                let over = pages.len().saturating_sub(limit);
                for dropped in pages.drain(..over) {
                    self.effects.push(Effect::CacheWrite {
                        key: format!("page:{dropped}"),
                        value: None,
                    });
                }
                self.inner.cache.disk = pages;
                self.boot();
                self.inner.cache.confirmed = false;
                self.shell_confirmed();
            } else {
                // What was kept is another person's, or from before a sign-out.
                // Emptying the store takes this launch's pages with it.
                self.inner.cache.disk.clear();
                self.effects.push(Effect::CacheClear);
                self.boot();
            }
        } else if let Some(url) = key.strip_prefix("page:") {
            // Nothing kept is drawn before this launch's shell has named the person.
            if !self.inner.cache.confirmed {
                return;
            }
            let Ok(page) = serde_json::from_str::<Page>(&value) else {
                return;
            };
            // Only a screen that still shows nothing takes it: the fresh answer may have won.
            let waiting: Vec<ScreenId> = self
                .view
                .screens
                .values()
                .filter(|screen| {
                    screen.url == url && screen.loading && matches!(screen.content, Content::Blank)
                })
                .map(|screen| screen.id)
                .collect();
            for id in waiting {
                self.show_kept(id, page.status, &page.body);
            }
        }
    }
}
