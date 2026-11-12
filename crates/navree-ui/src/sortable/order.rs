//! The order a [`SortableGrid`](crate::SortableGrid) posts, and how it is put
//! together with the tiles as the server has them now.

use http::HeaderValue;
use http::StatusCode;
use http::header::ALLOW;
use http::request::Parts;
use navree_http::body::{BodyLimit, read_body};
use navree_http::extract::{FromRequest, Request, urlencoded};
use navree_http::response::{IntoResponse, Response};

use crate::html::LocalHref;
use crate::op::{Envelope, Op, Reply, ToastTone, UI_HEADER};

/// The longest key an order may name, in bytes.
const MAX_KEY: usize = 256;

/// The most name-and-value pairs read from one order's body.
const MAX_PAIRS: usize = 1_000;

/// The order of a [`SortableGrid`](crate::SortableGrid)'s tiles, as the grid posts it:
/// every tile's key in the order the page shows them, the tiles taken away, and, from
/// a page with JavaScript off, one tile to move a place earlier or later.
///
/// It reads a urlencoded form and answers forgery as [`Submit`](crate::Submit) does: a
/// body from another site gets 403 (unless its `Origin` is in
/// `http.cors.allowed_origins`), a body that isn't a form 415, and `GET` 405. A body
/// that names a key twice in its order, a key longer than 256 bytes, or more than
/// 1,000 fields gets 400 (413 for the size).
///
/// Nothing in it is trusted. [`arrange`](Self::arrange) puts it together with the
/// tiles as they are now: keys the server doesn't hold are passed over, tiles the
/// page didn't know of keep their place at the end, and only what was asked is
/// taken away. Save the result in one write:
///
/// ```ignore
/// async fn tiles(db: Db, order: GridOrder) -> Result<Reply> {
///     let now = tile_keys(&db).await?; // in their saved order
///     let arranged = order.keep(["health"]).arrange(&now);
///     if arranged.changed {
///         save_tiles(&db, &arranged.order).await?;
///     }
///     Ok(order.done("/widgets/7").op(Op::replace("preview", &preview(&arranged.order))))
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct GridOrder {
    /// The keys in the order the page shows them.
    pub order: Vec<String>,
    /// The keys of the tiles taken away.
    pub remove: Vec<String>,
    /// A tile to move one place earlier: the button a page with JavaScript off posts.
    pub up: Option<String>,
    /// A tile to move one place later.
    pub down: Option<String>,
    /// Keys that are never taken away, whatever the post says: see [`keep`](Self::keep).
    pub kept: Vec<String>,
}

/// An order put together with the tiles the server holds: what to save.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Arranged {
    /// Every tile's key in its new order.
    pub order: Vec<String>,
    /// The tiles taken away, each once, in the order they were named.
    pub removed: Vec<String>,
    /// Whether anything differs from the tiles as they were.
    pub changed: bool,
}

impl GridOrder {
    /// An order of `keys`, with nothing taken away.
    pub fn new<I, K>(keys: I) -> Self
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        Self {
            order: keys.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// Takes `key` away too.
    #[must_use]
    pub fn remove(mut self, key: impl Into<String>) -> Self {
        self.remove.push(key.into());
        self
    }

    /// Moves `key` one place earlier, as the button of a page with JavaScript off does.
    #[must_use]
    pub fn up(mut self, key: impl Into<String>) -> Self {
        self.up = Some(key.into());
        self
    }

    /// Moves `key` one place later.
    #[must_use]
    pub fn down(mut self, key: impl Into<String>) -> Self {
        self.down = Some(key.into());
        self
    }

    /// Never takes these keys away, whatever the post asks: the tiles drawn with
    /// [`GridTile::fixed`](crate::GridTile::fixed).
    #[must_use]
    pub fn keep<I, K>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        self.kept.extend(keys.into_iter().map(Into::into));
        self
    }

    /// Reads an order from a urlencoded body: every `order` field in turn, every
    /// `remove`, and the first non-empty `up` and `down`. Other fields are passed over.
    ///
    /// # Errors
    ///
    /// When a key is longer than 256 bytes, the order names a key twice, or the body
    /// holds more than 1,000 pairs.
    pub fn from_form(body: &[u8]) -> Result<Self, OrderError> {
        let mut out = Self::default();
        for (count, (name, value)) in urlencoded::parse(body).enumerate() {
            if count >= MAX_PAIRS {
                return Err(OrderError::TooLarge);
            }
            let field = match name.as_ref() {
                "order" => "order",
                "remove" => "remove",
                "up" => "up",
                "down" => "down",
                _ => continue,
            };
            if value.len() > MAX_KEY {
                return Err(OrderError::Field(field));
            }
            if value.is_empty() {
                continue;
            }
            let value = value.into_owned();
            match field {
                "order" => {
                    if out.order.contains(&value) {
                        return Err(OrderError::Repeated);
                    }
                    out.order.push(value);
                }
                "remove" => out.remove.push(value),
                "up" => {
                    out.up.get_or_insert(value);
                }
                _ => {
                    out.down.get_or_insert(value);
                }
            }
        }
        Ok(out)
    }

    /// Puts the order together with `current`, the keys of the tiles the server holds
    /// now, in their saved order.
    ///
    /// The tiles come in the order posted; a key the server doesn't hold (a tile taken
    /// away in another window) is passed over, and a tile the page didn't know of (one
    /// added in another window) keeps its place after the rest, so a page drawn earlier
    /// never loses one. Then the tiles asked for are taken away, except the
    /// [kept](Self::keep) ones, and the one tile to move earlier or later swaps with its
    /// neighbour.
    ///
    /// ```
    /// use navree_ui::GridOrder;
    ///
    /// let order = GridOrder::new(["sleep", "recovery", "stress"]).remove("stress");
    /// let arranged = order.arrange(&["recovery", "sleep", "stress", "steps"]);
    /// assert_eq!(arranged.order, ["sleep", "recovery", "steps"]);
    /// assert_eq!(arranged.removed, ["stress"]);
    /// assert!(arranged.changed);
    /// ```
    pub fn arrange<K: AsRef<str>>(&self, current: &[K]) -> Arranged {
        let held = |key: &str| current.iter().any(|k| k.as_ref() == key);
        let mut order: Vec<String> = Vec::with_capacity(current.len());
        for key in &self.order {
            if held(key) && !order.contains(key) {
                order.push(key.clone());
            }
        }
        for key in current {
            let key = key.as_ref();
            if !order.iter().any(|k| k == key) {
                order.push(key.to_owned());
            }
        }
        let mut removed: Vec<String> = Vec::new();
        for key in &self.remove {
            if self.kept.contains(key) || removed.contains(key) {
                continue;
            }
            if let Some(at) = order.iter().position(|k| k == key) {
                order.remove(at);
                removed.push(key.clone());
            }
        }
        if let Some(at) = self
            .up
            .as_ref()
            .and_then(|key| order.iter().position(|k| k == key))
            && at > 0
        {
            order.swap(at, at - 1);
        }
        if let Some(at) = self
            .down
            .as_ref()
            .and_then(|key| order.iter().position(|k| k == key))
            && at + 1 < order.len()
        {
            order.swap(at, at + 1);
        }
        let changed =
            order.len() != current.len() || order.iter().zip(current).any(|(a, b)| a != b.as_ref());
        Arranged {
            order,
            removed,
            changed,
        }
    }

    /// The answer to an order that was saved. The page shows the order already, so it
    /// gets no ops of its own (add a preview drawn again, say); with JavaScript off the
    /// browser goes back to `page`.
    pub fn done(&self, page: impl Into<LocalHref>) -> Reply {
        Reply::ops_or_redirect(page.into().into_href())
    }
}

/// Why a body isn't an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OrderError {
    /// A key is longer than 256 bytes.
    Field(&'static str),
    /// The order names a key twice.
    Repeated,
    /// The body holds more pairs than an order may.
    TooLarge,
}

impl std::fmt::Display for OrderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Field(name) => write!(f, "the field `{name}` is too long"),
            Self::Repeated => f.write_str("the order names a key twice"),
            Self::TooLarge => f.write_str("the body holds too many fields"),
        }
    }
}

impl std::error::Error for OrderError {}

/// A request that isn't an order: what [`GridOrder`] answers in its place.
#[derive(Debug)]
pub struct OrderRejected {
    status: StatusCode,
    from_runtime: bool,
}

impl OrderRejected {
    /// The status it answers with.
    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl IntoResponse for OrderRejected {
    fn into_response(self) -> Response {
        if self.status == StatusCode::BAD_REQUEST && self.from_runtime {
            let ops = vec![Op::toast(
                ToastTone::Danger,
                "That order couldn't be read. Reload the page and try again.",
            )];
            return (self.status, Envelope::new(ops)).into_response();
        }
        let mut response = self.status.into_response();
        if self.status == StatusCode::METHOD_NOT_ALLOWED {
            response
                .headers_mut()
                .insert(ALLOW, HeaderValue::from_static("POST"));
        }
        response
    }
}

impl FromRequest for GridOrder {
    type Rejection = OrderRejected;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        let (parts, body): (Parts, _) = request.into_parts();
        let from_runtime = parts.headers.contains_key(UI_HEADER);
        let refuse = |status| OrderRejected {
            status,
            from_runtime,
        };
        if let Some(status) = crate::form::refusal(&parts).await {
            return Err(refuse(status));
        }
        let limit = BodyLimit::of(&parts.extensions);
        let Ok(bytes) = read_body(body, limit).await else {
            return Err(refuse(StatusCode::PAYLOAD_TOO_LARGE));
        };
        match Self::from_form(&bytes) {
            Ok(order) => Ok(order),
            Err(OrderError::TooLarge) => Err(refuse(StatusCode::PAYLOAD_TOO_LARGE)),
            Err(_) => Err(refuse(StatusCode::BAD_REQUEST)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_reads_every_order_field_in_turn_and_the_removes() {
        let order =
            GridOrder::from_form(b"_csrf=t&order=b&order=a&order=c&remove=c&remove=d&x=1").unwrap();
        assert_eq!(order.order, ["b", "a", "c"]);
        assert_eq!(order.remove, ["c", "d"]);
        assert_eq!(order.up, None);
        let order = GridOrder::from_form(b"order=a&order=b&up=b&up=a&down=").unwrap();
        assert_eq!(order.up.as_deref(), Some("b"));
        assert_eq!(order.down, None);
        // Keys are read as the form wrote them, spaces and symbols too.
        let order = GridOrder::from_form(b"order=a%20b&order=%E2%82%B9&order=").unwrap();
        assert_eq!(order.order, ["a b", "\u{20b9}"]);
    }

    #[test]
    fn a_form_that_repeats_a_key_or_is_too_long_is_refused() {
        assert_eq!(
            GridOrder::from_form(b"order=a&order=b&order=a"),
            Err(OrderError::Repeated)
        );
        let long = format!("order={}", "k".repeat(257));
        assert_eq!(
            GridOrder::from_form(long.as_bytes()),
            Err(OrderError::Field("order"))
        );
        let many: String = (0..1_001).map(|i| format!("order={i}&")).collect();
        assert_eq!(
            GridOrder::from_form(many.as_bytes()),
            Err(OrderError::TooLarge)
        );
        assert!(GridOrder::from_form(b"").unwrap().order.is_empty());
    }

    #[test]
    fn an_order_is_put_together_with_the_tiles_held_now() {
        let now = ["a", "b", "c", "d"];
        let same = GridOrder::new(now).arrange(&now);
        assert_eq!(same.order, now);
        assert!(!same.changed && same.removed.is_empty());

        let moved = GridOrder::new(["c", "a", "b", "d"]).arrange(&now);
        assert_eq!(moved.order, ["c", "a", "b", "d"]);
        assert!(moved.changed);

        // A key the server doesn't hold is passed over; one the page didn't know of
        // keeps its place at the end.
        let stale = GridOrder::new(["b", "gone", "a"]).arrange(&["a", "b", "new"]);
        assert_eq!(stale.order, ["b", "a", "new"]);

        // Only what was asked goes: a tile missing from the post stays.
        let partial = GridOrder::new(["b"]).remove("a").arrange(&now);
        assert_eq!(partial.order, ["b", "c", "d"]);
        assert_eq!(partial.removed, ["a"]);

        // Removing what isn't there, or twice, is nothing.
        let twice = GridOrder::new(now)
            .remove("x")
            .remove("b")
            .remove("b")
            .arrange(&now);
        assert_eq!(twice.removed, ["b"]);
        assert_eq!(twice.order, ["a", "c", "d"]);

        // A kept tile is never taken away.
        let kept = GridOrder::new(now)
            .remove("a")
            .remove("c")
            .keep(["a"])
            .arrange(&now);
        assert_eq!(kept.order, ["a", "b", "d"]);
        assert_eq!(kept.removed, ["c"]);
    }

    #[test]
    fn up_and_down_swap_with_a_neighbour_and_stop_at_the_ends() {
        let now = ["a", "b", "c"];
        assert_eq!(
            GridOrder::new(now).up("b").arrange(&now).order,
            ["b", "a", "c"]
        );
        assert_eq!(
            GridOrder::new(now).down("b").arrange(&now).order,
            ["a", "c", "b"]
        );
        let first = GridOrder::new(now).up("a").arrange(&now);
        assert_eq!(first.order, now);
        assert!(!first.changed);
        assert_eq!(GridOrder::new(now).down("c").arrange(&now).order, now);
        assert_eq!(GridOrder::new(now).up("zz").arrange(&now).order, now);
        // A move after a removal counts places among what is left.
        assert_eq!(
            GridOrder::new(now).remove("b").up("c").arrange(&now).order,
            ["c", "a"]
        );
    }

    #[test]
    fn nothing_held_and_nothing_posted_is_nothing() {
        let none: [&str; 0] = [];
        let arranged = GridOrder::default().arrange(&none);
        assert!(arranged.order.is_empty() && !arranged.changed);
        let all_gone = GridOrder::new(["a"]).remove("a").arrange(&["a"]);
        assert!(all_gone.order.is_empty() && all_gone.changed);
    }
}
