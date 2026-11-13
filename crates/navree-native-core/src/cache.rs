//! The last tree of each address: shown at once while a fresh one is fetched.
//!
//! The core keeps a bounded number of answers in memory, as they came. When
//! the app's build says so, it also hands them to the client to keep between
//! launches, under keys the client need not understand: `boot` (the shell and
//! the list of pages kept) and `page:<address>`. A kept tree is only ever a
//! head start: it is marked as kept, nothing is sent from it, and the fresh
//! answer takes its place. A write, a new build, a change of person and a
//! sign-out empty it.

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

    /// Keeps the answer a page gave.
    pub(crate) fn remember(&mut self, url: &str, status: u16, body: &str) {
        let limit = self.config.cache.pages;
        if limit == 0 || status >= 400 {
            return;
        }
        self.inner.cache.put(url, status, body.to_owned(), limit);
        if !self.config.cache.persist {
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
        } else if self.config.cache.persist && self.inner.cache.disk.iter().any(|kept| kept == url)
        {
            self.effects.push(Effect::CacheRead {
                key: format!("page:{url}"),
            });
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
            self.inner.cache.disk = boot.pages;
            // The shell that came over the network is the newer one.
            if !self.inner.has_shell {
                self.shell_from_cache(&boot.shell);
            }
        } else if let Some(url) = key.strip_prefix("page:") {
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
