//! Where the person is: a stack of screens for each top-level destination,
//! and sheets and covers over them.
//!
//! - A link to a destination selects it instead of stacking a copy of it.
//! - How a link is presented is the link's to say; when it says nothing, the
//!   shell's routes say, and without a route it is pushed.
//! - Back closes what is open on the screen in front, then pops, then takes a
//!   sheet or cover away, then (where the system has such a back) leads to the
//!   first destination, then leaves the app to the system.
//! - Where a post leads: out of a presented screen and back to what is under
//!   it; back to a screen already in the stack; or in place of the form.
//! - An address from outside opens only when the shell knows it.

use navree_native::action::Present;

use crate::config::DeepLinks;
use crate::effect::{Change, Effect};
use crate::origin::{Target, path_of, route_matches};
use crate::reduce::Shown;
use crate::state::{Content, Modal, ScreenId};
use crate::Core;

impl Core {
    /// Follows `href` as found on the screen `from`. Returns the screen now
    /// in front of that stack.
    pub(crate) fn navigate(&mut self, from: Option<ScreenId>, href: &str, present: Present) -> Option<ScreenId> {
        let target = match from {
            Some(from) => self.resolve(href, from),
            None => self.inner.origin.resolve(href, "/"),
        };
        match target {
            Target::Refused => from,
            Target::Outside(url) => {
                self.effects.push(Effect::OpenUrl { url });
                from
            }
            Target::Inside(url) => self.go(from, &url, present),
        }
    }

    /// Takes every sheet and cover away.
    fn dismiss_modals(&mut self) {
        let modals = std::mem::take(&mut self.view.modals);
        for id in modals.into_iter().flat_map(|modal| modal.stack) {
            self.close(id);
        }
    }

    /// The destination whose own address is `url`'s.
    fn destination_at(&self, url: &str) -> Option<usize> {
        let path = path_of(url);
        self.view
            .destinations
            .iter()
            .position(|destination| path_of(&self.destination_url(&destination.href)) == path)
    }

    /// Goes to `url`, an address inside, from the screen `from`.
    pub(crate) fn go(&mut self, from: Option<ScreenId>, url: &str, present: Present) -> Option<ScreenId> {
        // A sign-in with another site runs in the system's own session, never in a fetch.
        if self.config.sign_in.paths.iter().any(|pattern| route_matches(pattern, url)) {
            self.inner.signing_in = true;
            self.effects.push(Effect::SignIn {
                url: self.inner.origin.absolute(url),
                callback: self.config.sign_in.callback.clone(),
            });
            return from;
        }
        let how = match present {
            Present::Push => self.route(url).and_then(|route| route.present).unwrap_or(Present::Push),
            said => said,
        };
        if how == Present::External {
            let url = self.inner.origin.absolute(url);
            self.effects.push(Effect::OpenUrl { url });
            return from;
        }
        if let Some(from) = from {
            self.close_overlays(from);
        }
        if let (Some(tab), Present::Push | Present::Root) = (self.destination_at(url), how) {
            // A link to a top-level destination selects it.
            self.dismiss_modals();
            let stack = std::mem::take(&mut self.view.destinations[tab].stack);
            let root = stack.first().copied().filter(|root| self.view.screens.get(root).is_some_and(|view| view.url == url));
            for id in stack {
                if Some(id) != root {
                    self.close(id);
                }
            }
            let root = root.unwrap_or_else(|| self.open(url));
            self.view.destinations[tab].stack = vec![root];
            self.view.selected = tab;
            self.touch(Change::Nav);
            return Some(root);
        }
        let anchor = from.filter(|from| self.view.holds(*from)).or(self.front());
        match how {
            Present::Replace => {
                let id = anchor?;
                self.replace(id, url);
                Some(id)
            }
            Present::Root => {
                self.dismiss_modals();
                let selected = self.view.selected;
                let stack = std::mem::take(&mut self.view.destinations.get_mut(selected)?.stack);
                for id in stack {
                    self.close(id);
                }
                let id = self.open(url);
                self.view.destinations[selected].stack = vec![id];
                self.touch(Change::Nav);
                Some(id)
            }
            Present::Sheet | Present::Full => {
                let id = self.open(url);
                self.view.modals.push(Modal {
                    present: how,
                    locked: false,
                    stack: vec![id],
                });
                self.touch(Change::Nav);
                Some(id)
            }
            _ => {
                // A second tap on a link whose screen is still on its way opens it once.
                let top = anchor.and_then(|anchor| self.view.stack_of(anchor)).and_then(|stack| stack.last().copied());
                if let Some(top) = top.filter(|top| Some(*top) != anchor) {
                    let waiting = self.view.screens.get(&top).is_some_and(|view| view.url == url && view.loading);
                    if waiting {
                        return Some(top);
                    }
                }
                let id = self.open(url);
                let selected = self.view.selected;
                match anchor.and_then(|anchor| self.view.stack_of(anchor)) {
                    Some(stack) => stack.push(id),
                    None => self.view.destinations.get_mut(selected)?.stack.push(id),
                }
                self.touch(Change::Nav);
                Some(id)
            }
        }
    }

    /// The screen stays and takes another address's answer in its place. What
    /// it shows stays until the answer is here.
    pub(crate) fn replace(&mut self, id: ScreenId, url: &str) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        if path_of(&view.url) != path_of(url) {
            // Another page: what was typed and opened here belongs to the page being left.
            inner.dirty.clear();
            if matches!(view.content, Content::Blank) {
                view.chrome = self.inner.routes.iter().find(|route| route_matches(&route.matches, url)).is_none_or(|route| route.chrome);
            }
        }
        view.url = url.to_owned();
        self.fetch_page(id, false);
    }

    pub(crate) fn select(&mut self, index: usize) {
        if !self.view.modals.is_empty() || index >= self.view.destinations.len() {
            return;
        }
        if index == self.view.selected {
            // The destination in front, chosen again: back to its first screen.
            let stack = &mut self.view.destinations[index].stack;
            let rest: Vec<ScreenId> = stack.drain(1.min(stack.len())..).collect();
            for id in rest {
                self.close(id);
            }
        } else {
            self.view.selected = index;
        }
        self.touch(Change::Nav);
    }

    /// The system's back.
    pub(crate) fn back(&mut self) {
        if let Some(front) = self.front() {
            let open = self.view.screens.get(&front).and_then(|view| view.overlays.last().cloned());
            if let Some(open) = open {
                return self.set_open(front, &open, false);
            }
        }
        if let Some(modal) = self.view.modals.last_mut() {
            // The sign-in a session's end put up can't be backed out of.
            if modal.locked && modal.stack.len() == 1 {
                return self.effects.push(Effect::Exit);
            }
            let gone = if modal.stack.len() > 1 {
                modal.stack.pop().into_iter().collect()
            } else {
                self.view.modals.pop().map(|modal| modal.stack).unwrap_or_default()
            };
            for id in gone {
                self.close(id);
            }
            return self.touch(Change::Nav);
        }
        let selected = self.view.selected;
        let Some(destination) = self.view.destinations.get_mut(selected) else {
            return self.effects.push(Effect::Exit);
        };
        if destination.stack.len() > 1 {
            if let Some(id) = destination.stack.pop() {
                self.close(id);
            }
        } else if self.config.back_to_first && selected != 0 {
            // From any other destination, back leads to the first one before it leaves the app.
            self.view.selected = 0;
            self.touch(Change::Nav);
        } else {
            self.effects.push(Effect::Exit);
        }
    }

    /// The person went back to a screen further down its stack.
    pub(crate) fn pop_to(&mut self, id: ScreenId) {
        let Some(stack) = self.view.stack_of(id) else {
            return;
        };
        let Some(at) = stack.iter().position(|other| *other == id) else {
            return;
        };
        let gone: Vec<ScreenId> = stack.drain(at + 1..).collect();
        for id in gone {
            self.close(id);
        }
    }

    /// The person took a sheet or cover away.
    pub(crate) fn dismissed(&mut self, id: ScreenId) {
        let Some(at) = self.view.modals.iter().position(|modal| modal.stack.contains(&id)) else {
            return;
        };
        self.take_modals_from(at);
    }

    /// Takes away the sheet or cover at `at` and whatever is presented over it.
    pub(crate) fn take_modals_from(&mut self, at: usize) {
        let gone: Vec<Modal> = self.view.modals.drain(at..).collect();
        for id in gone.into_iter().flat_map(|modal| modal.stack) {
            self.close(id);
        }
    }

    /// A post was answered with a whole screen for `url`. Returns the screen
    /// that shows it.
    pub(crate) fn arrive(&mut self, id: ScreenId, url: &str, shown: Shown) -> ScreenId {
        let Some(view) = self.view.screens.get(&id) else {
            return id;
        };
        if view.url == url {
            // The same page again, as the server now has it: its values are the ones to show.
            if let Some(inner) = self.inner.screens.get_mut(&id) {
                inner.dirty.clear();
            }
            self.show(id, url, shown);
            return id;
        }
        self.land(id, url, Some(shown))
    }

    fn settle_on(&mut self, id: ScreenId, url: &str, shown: Option<Shown>) {
        match shown {
            Some(shown) => self.show(id, url, shown),
            None => self.fetch_page(id, false),
        }
    }

    fn arrived(&mut self, url: &str, shown: Option<Shown>) -> ScreenId {
        match shown {
            Some(shown) => {
                let id = self.new_screen(url);
                self.show(id, url, shown);
                id
            }
            None => self.open(url),
        }
    }

    /// Where a post leads when it leads to another address. `shown` is what
    /// the answer already brought for it; without it the address is fetched.
    pub(crate) fn land(&mut self, id: ScreenId, url: &str, shown: Option<Shown>) -> ScreenId {
        if !self.view.holds(id) {
            return id;
        }
        if self.view.in_modal(id) {
            // The presented screen has done its work: it goes, and what is under it carries on.
            self.dismissed(id);
            let under = self.front();
            if let Some(under) = under.filter(|under| self.view.screens.get(under).is_some_and(|view| view.url == url)) {
                self.settle_on(under, url, shown);
                return under;
            }
            // A link to a destination selects it; anything else is stacked on what was under.
            if self.destination_at(url).is_some() {
                return self.go(under, url, Present::Push).unwrap_or(id);
            }
            let new = self.arrived(url, shown);
            let selected = self.view.selected;
            match under.and_then(|under| self.view.stack_of(under)) {
                Some(stack) => stack.push(new),
                None => {
                    if let Some(destination) = self.view.destinations.get_mut(selected) {
                        destination.stack.push(new);
                    }
                }
            }
            self.touch(Change::Nav);
            return new;
        }
        let stack: Vec<ScreenId> = self.view.stack_of(id).cloned().unwrap_or_default();
        let at = stack.iter().position(|other| *other == id).unwrap_or(0);
        let below = stack[..at].iter().rposition(|other| self.view.screens.get(other).is_some_and(|view| view.url == url));
        if let Some(below) = below {
            // That screen is already under this one: go back to it rather than stack a copy.
            let target = stack[below];
            self.pop_to(target);
            self.settle_on(target, url, shown);
            self.touch(Change::Nav);
            return target;
        }
        // Otherwise the new screen takes the place of the one that posted.
        let new = self.arrived(url, shown);
        if let Some(stack) = self.view.stack_of(id) {
            for place in stack.iter_mut().filter(|place| **place == id) {
                *place = new;
            }
        }
        self.close(id);
        new
    }

    /// The app's own scheme stands for its origin: `shop://orders/7` is `/orders/7`.
    fn own_scheme<'a>(&self, address: &'a str) -> Option<&'a str> {
        let scheme = self.config.sign_in.callback.as_deref()?;
        let rest = address.strip_prefix(scheme)?.strip_prefix("://")?;
        Some(rest)
    }

    /// An address from outside the app: a deep link, a notification, the end
    /// of a sign-in. It is only ever fetched as a page, and only when the
    /// shell knows it: a destination, or an address one of its routes names.
    pub(crate) fn opened(&mut self, address: &str) {
        let own = self.own_scheme(address).map(|rest| format!("/{}", rest.trim_start_matches('/')));
        let Target::Inside(url) = self.inner.origin.resolve(own.as_deref().unwrap_or(address), "/") else {
            return;
        };
        if !self.inner.has_shell {
            // Asked before the shell is here: it opens when the shell does.
            self.inner.launch = Some(url);
            return;
        }
        let destination = self.destination_at(&url);
        let route = self.route(&url).map(|route| route.present);
        if destination.is_none() && route.is_none() && self.config.deep_links == DeepLinks::Routes {
            return;
        }
        if destination.is_some() {
            self.go(None, &url, Present::Push);
            return;
        }
        if let Some(Some(present @ (Present::Sheet | Present::Full))) = route {
            self.go(None, &url, present);
            return;
        }
        // It is stacked on the destination it belongs to: the one whose address its own starts with.
        self.dismiss_modals();
        let path = path_of(&url);
        let owner = self
            .view
            .destinations
            .iter()
            .enumerate()
            .filter_map(|(at, destination)| {
                let root = path_of(&self.destination_url(&destination.href));
                let under = root != "/" && path.strip_prefix(root.as_str()).is_some_and(|rest| rest.starts_with('/'));
                under.then_some((root.len(), at))
            })
            .max()
            .map(|(_, at)| at);
        if let Some(owner) = owner {
            self.view.selected = owner;
        }
        let selected = self.view.selected;
        if self.view.destinations.get(selected).is_some_and(|destination| destination.stack.is_empty()) {
            let root = self.destination_url(&self.view.destinations[selected].href);
            let root = self.open(&root);
            self.view.destinations[selected].stack.push(root);
        }
        let id = self.open(&url);
        if let Some(destination) = self.view.destinations.get_mut(selected) {
            destination.stack.push(id);
        }
        self.touch(Change::Nav);
    }

    /// A link was tapped in the web view.
    pub(crate) fn web_link(&mut self, id: ScreenId, url: &str) {
        match self.resolve(url, id) {
            // The page the web view already shows is not a place to go.
            Target::Inside(inside) if self.view.screens.get(&id).is_some_and(|view| view.url == inside) => {}
            Target::Inside(inside) => {
                self.go(Some(id), &inside, Present::Push);
            }
            Target::Outside(url) => self.effects.push(Effect::OpenUrl { url }),
            Target::Refused => {}
        }
    }

    /// A write happened: nothing kept can be trusted, and every other screen
    /// may show what is no longer so. Each is fetched again when it comes to
    /// the front.
    pub(crate) fn wrote(&mut self, except: ScreenId) {
        self.inner.epoch += 1;
        self.forget_pages();
        for (id, inner) in &mut self.inner.screens {
            if *id != except {
                inner.stale = true;
            }
        }
    }
}
