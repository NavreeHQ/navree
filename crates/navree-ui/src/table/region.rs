//! [`Region`]: a named part of a page that updates by itself.

use std::time::Duration;

use crate::components::{Skeleton, SkeletonShape};
use crate::html::Href;
use crate::node::{AttrValue, Element, IntoNodes, Node, Uses, el};

/// A named part of a page that updates by itself: fetched after paint, polled,
/// or refetched when an op invalidates it. Its answer morphs in.
///
/// ```
/// use navree_ui::{Region, render};
///
/// let html = render(&Region::new("kpis").src("/fragments/kpis").lazy().into());
/// assert!(html.contains(r#"data-ui-src="/fragments/kpis""#));
/// assert!(html.contains("data-ui-lazy"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Region {
    el: Element,
    placeholder: bool,
}

impl Region {
    /// A region named `id`.
    pub fn new(id: impl Into<AttrValue>) -> Self {
        Self { el: el("div").slot("region").attr("id", id).flag("data-ui-region", true).uses(Uses::CORE), placeholder: false }
    }

    /// Sets where its content comes from: a page holding an element with this
    /// id, or a fragment.
    pub fn src(mut self, src: impl Into<Href>) -> Self {
        self.el = self.el.attr("data-ui-src", src.into());
        self
    }

    /// Fetches it after paint, showing a skeleton until then.
    pub fn lazy(mut self) -> Self {
        self.el = self.el.flag("data-ui-lazy", true).attr("aria-busy", "true");
        self.placeholder = true;
        self
    }

    /// Refetches it this often, while the page is visible.
    pub fn poll(mut self, every: Duration) -> Self {
        self.el = self.el.attr("data-ui-poll", i64::try_from(every.as_millis()).unwrap_or(i64::MAX));
        self
    }

    /// Makes it a feed: each poll asks the source for what is newer than a cursor
    /// and adds it at the start of the element `into` inside the region, instead
    /// of fetching the whole region again.
    ///
    /// The source is the region's [`src`](Self::src) with `after=<cursor>` added.
    /// It answers the new rows as HTML, nothing when there are none, and the
    /// newest cursor in the `X-UI-Cursor` header. `empty`, if given, names an
    /// element removed once the first row arrives (an "nothing yet" message).
    pub fn feed(mut self, into: impl Into<AttrValue>, after: impl Into<AttrValue>, empty: Option<&str>) -> Self {
        self.el = self.el.attr("data-ui-feed", into).attr("data-ui-after", after).attr_opt("data-ui-feed-empty", empty.map(str::to_owned));
        self
    }

    /// Adds its content.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self.placeholder = false;
        self
    }

    fn into_node(self) -> Node {
        if self.placeholder {
            return self.el.children((Skeleton::new(SkeletonShape::Line), Skeleton::new(SkeletonShape::Short))).into();
        }
        self.el.into()
    }
}

impl From<Region> for Node {
    fn from(region: Region) -> Self {
        region.into_node()
    }
}
