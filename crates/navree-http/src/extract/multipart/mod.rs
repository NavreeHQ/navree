//! [`Multipart`]: a `multipart/form-data` body, one part at a time, read by
//! Navree's own streaming parser within Navree's limits.
//!
//! A form is often read before anyone has been authenticated, so every way
//! of making the parser hold or search bytes has a bound: see
//! [`MultipartLimits`]. A value is handed on as it arrives, never held whole.

use std::fmt;
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};
use std::task::{Context, Poll, ready};

use encoding_rs::{Encoding, UTF_8};
use futures_util::{Stream, StreamExt};
use http::HeaderMap;
use http::header::CONTENT_TYPE;
use http_body_util::{BodyDataStream, BodyExt, LengthLimitError, Limited};
use navree_config::ConfigTable;
use navree_kernel::{AppContext, BoxError};
use serde::Deserialize;

use super::rejection::{MultipartError, MultipartRejection};
use super::{FromRequest, OptionalFromRequest, Request};
use crate::HttpOptions;
use crate::body::{Body, BodyError, BodyLimit, Bytes};
use crate::serve::BodyReadTimeout;

mod disposition;
mod parser;

use parser::{Failure, Head, Next, Parser, Piece};

/// The longest boundary: RFC 2046's 70 characters.
const MAX_BOUNDARY_BYTES: usize = 70;

/// What a form may hold, on top of the route's body limit (configuration
/// section `http.multipart`; every key optional). A handler sets its own with
/// [`Multipart::with_limits`].
///
/// A value past a limit answers 413. The whole body answers to the route's
/// [`BodyLimit`], however it is split, and to the server's pace on request
/// bodies (408), so a file has no limit of its own by default: raising
/// `RouteDef::body_limit` is what makes room for one.
///
/// ```toml
/// [http.multipart]
/// max_parts = 1000
/// max_field_bytes = 1048576
/// max_header_bytes = 16384
/// ```
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, ConfigTable)]
#[serde(default)]
pub struct MultipartLimits {
    /// The most parts in a form (default 1,000), counting parts a handler
    /// skips.
    pub max_parts: usize,
    /// The largest value of a part without a file name, in bytes (default
    /// 1 MiB). [`Field::text`] keeps to it whatever the part calls itself.
    pub max_field_bytes: u64,
    /// The largest value of a part with a file name, in bytes (default
    /// `None`: the route's body limit). A client names the file, so this is
    /// no way to keep text small: that is `max_field_bytes`.
    pub max_file_bytes: Option<u64>,
    /// The largest head of a part, the bytes after its delimiter line up to
    /// and including the blank line (default 16 KiB). Also bounds what comes
    /// before the first delimiter, and the padding after a delimiter.
    pub max_header_bytes: usize,
}

impl Default for MultipartLimits {
    fn default() -> Self {
        Self {
            max_parts: 1000,
            max_field_bytes: 1024 * 1024,
            max_file_bytes: None,
            max_header_bytes: 16 * 1024,
        }
    }
}

impl MultipartLimits {
    /// The problems with these limits, for the boot.
    pub(crate) fn check(&self, errors: &mut Vec<String>) {
        if self.max_parts == 0 {
            errors.push("http.multipart.max_parts must be at least 1".to_owned());
        }
        if self.max_header_bytes == 0 {
            errors.push("http.multipart.max_header_bytes must be at least 1".to_owned());
        }
    }

    /// The limits of a request: the app's, else the defaults.
    async fn of(extensions: &http::Extensions) -> Self {
        let app = extensions.get::<AppContext>().cloned();
        match app {
            Some(app) => app
                .try_get::<HttpOptions>()
                .await
                .map_or_else(Self::default, |options| options.multipart),
            None => Self::default(),
        }
    }
}

/// A `multipart/form-data` body, read part by part. It reads the body, so
/// it is a handler's last argument.
///
/// ```
/// use navree_http::AppError;
/// use navree_http::extract::Multipart;
///
/// async fn upload(mut form: Multipart) -> Result<String, AppError> {
///     let mut total = 0;
///     while let Some(mut field) = form.next_field().await? {
///         while let Some(chunk) = field.chunk().await? {
///             total += chunk.len();
///         }
///     }
///     Ok(format!("{total} bytes"))
/// }
/// ```
///
/// Nothing is read until [`next_field`](Self::next_field) is called. Every
/// limit answers with a [`MultipartError`], and `?` keeps its status. A
/// failure is final: later calls answer the same failure, so a form that was
/// cut short never reads as a shorter form.
///
/// Field names, file names and content types are what the client sent. A
/// file name may hold a path, `..`, or anything else: nothing here reads it
/// as one, and neither should a handler (`store_upload` in navree-storage
/// keeps only its last component, and stores under a random name).
///
/// A part's `name` and `filename` are the parameters of its
/// `Content-Disposition`, read in order (a parameter is never found inside
/// another one's value); `filename*` (RFC 8187, UTF-8 or ISO-8859-1) is
/// preferred to `filename` when it decodes. A part with two
/// `Content-Disposition` or two `Content-Type` headers, or that names a
/// parameter twice, is a 400: whoever is in front of the app may have read
/// the other one. So is what two readers could take differently: a head with
/// a bare line feed or anything after its header lines, something that is not
/// a parameter where one should be, a quote that does not close, and a
/// backslash before a quote where another reader would close the string. A
/// first delimiter counts only at the start of the body or of a line.
pub struct Multipart {
    // Only ever reached through `&mut self`, with `get_mut`: the lock makes
    // the form `Sync` and is never taken.
    body: Mutex<BodyDataStream<Limited<Body>>>,
    parser: Parser,
    limits: MultipartLimits,
    parts: usize,
    failed: Option<MultipartError>,
}

impl fmt::Debug for Multipart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Multipart")
            .field("parser", &self.parser)
            .field("limits", &self.limits)
            .field("parts", &self.parts)
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}

impl Multipart {
    /// Replaces the limits of this form. Set them before reading: what has
    /// been read so far counts.
    #[must_use]
    pub fn with_limits(self, limits: MultipartLimits) -> Self {
        Self { limits, ..self }
    }

    /// The limits of this form.
    pub fn limits(&self) -> MultipartLimits {
        self.limits
    }

    /// The next part, `None` after the last. A part left unread is skipped.
    pub async fn next_field(&mut self) -> Result<Option<Field<'_>>, MultipartError> {
        let limit = self.limits.max_header_bytes;
        let next = poll_fn(|cx| self.poll_read(cx, |parser| parser.next(limit))).await?;
        let Next::Part(head) = next else {
            return Ok(None);
        };
        if self.parts >= self.limits.max_parts {
            return Err(self.fail(MultipartError::TooManyParts {
                limit: self.limits.max_parts,
            }));
        }
        self.parts += 1;
        Ok(Some(Field::new(head, self)))
    }

    /// What `read` finds in the body, reading more of the body while `read`
    /// wants more. A failure stays.
    fn poll_read<T>(
        &mut self,
        cx: &mut Context<'_>,
        mut read: impl FnMut(&mut Parser) -> Result<Option<T>, Failure>,
    ) -> Poll<Result<T, MultipartError>> {
        loop {
            if let Some(error) = &self.failed {
                return Poll::Ready(Err(error.clone()));
            }
            let failure = match read(&mut self.parser) {
                Ok(Some(found)) => return Poll::Ready(Ok(found)),
                Ok(None) => None,
                Err(Failure::HeadTooLarge) => Some(MultipartError::HeadTooLarge {
                    limit: self.limits.max_header_bytes,
                }),
                Err(Failure::Malformed(message)) => Some(MultipartError::Malformed(message)),
            };
            if let Some(failure) = failure {
                return Poll::Ready(Err(self.fail(failure)));
            }
            let body = self.body.get_mut().unwrap_or_else(PoisonError::into_inner);
            match ready!(body.poll_next_unpin(cx)) {
                Some(Ok(chunk)) => self.parser.push(&chunk),
                Some(Err(error)) => {
                    let failure = MultipartError::Body(body_error(&error));
                    return Poll::Ready(Err(self.fail(failure)));
                }
                None => self.parser.end(),
            }
        }
    }

    fn fail(&mut self, error: MultipartError) -> MultipartError {
        self.failed = Some(error.clone());
        error
    }

    async fn start(request: Request, boundary: String) -> Self {
        let limits = MultipartLimits::of(request.extensions()).await;
        let body_limit = BodyLimit::of(request.extensions());
        let body = Limited::new(request.into_body(), body_limit).into_data_stream();
        Self {
            body: Mutex::new(body),
            parser: Parser::new(&boundary),
            limits,
            parts: 0,
            failed: None,
        }
    }
}

/// How a body failed.
fn body_error(error: &BoxError) -> BodyError {
    let chain = || {
        std::iter::successors(
            Some(&**error as &(dyn std::error::Error + 'static)),
            |error| error.source(),
        )
    };
    if chain().any(|error| error.is::<LengthLimitError>()) {
        BodyError::TooLarge
    } else if chain().any(|error| error.is::<BodyReadTimeout>()) {
        BodyError::TimedOut
    } else {
        BodyError::Read(error.to_string())
    }
}

impl FromRequest for Multipart {
    type Rejection = MultipartRejection;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        match form_boundary(request.headers()) {
            FormBoundary::Found(boundary) => Ok(Self::start(request, boundary).await),
            FormBoundary::Absent | FormBoundary::Invalid => Err(MultipartRejection),
        }
    }
}

impl OptionalFromRequest for Multipart {
    type Rejection = MultipartRejection;

    async fn from_request(request: Request) -> Result<Option<Self>, Self::Rejection> {
        match form_boundary(request.headers()) {
            FormBoundary::Found(boundary) => Ok(Some(Self::start(request, boundary).await)),
            FormBoundary::Absent => Ok(None),
            FormBoundary::Invalid => Err(MultipartRejection),
        }
    }
}

/// What a request's `Content-Type` says about a form.
enum FormBoundary {
    Found(String),
    /// No `Content-Type`, or another kind of body.
    Absent,
    /// A form with no usable boundary.
    Invalid,
}

/// The boundary of the form in `headers`. Two `Content-Type` headers, or two
/// `boundary` parameters, name none: whoever is in front of the app may have
/// read the other one.
fn form_boundary(headers: &HeaderMap) -> FormBoundary {
    let mut values = headers.get_all(CONTENT_TYPE).iter();
    let Some(value) = values.next() else {
        return FormBoundary::Absent;
    };
    if values.next().is_some() {
        return FormBoundary::Invalid;
    }
    let Ok(value) = value.to_str() else {
        return FormBoundary::Absent;
    };
    let Ok(media_type) = value.parse::<mime::Mime>() else {
        return FormBoundary::Invalid;
    };
    if media_type.type_() != mime::MULTIPART || media_type.subtype() != mime::FORM_DATA {
        return FormBoundary::Absent;
    }
    let mut boundaries = media_type
        .params()
        .filter(|(name, _)| *name == mime::BOUNDARY);
    match (boundaries.next(), boundaries.next()) {
        (Some((_, boundary)), None)
            if (1..=MAX_BOUNDARY_BYTES).contains(&boundary.as_str().len()) =>
        {
            FormBoundary::Found(boundary.as_str().to_owned())
        }
        _ => FormBoundary::Invalid,
    }
}

/// One part of a [`Multipart`] form. Reading its value counts towards the
/// limit for its kind: `max_file_bytes` when it has a file name, else
/// `max_field_bytes`. It is also a [`Stream`] of its value, which ends after
/// reporting a failure.
#[derive(Debug)]
pub struct Field<'a> {
    head: Head,
    limit: u64,
    read: u64,
    file: bool,
    // The stream has reported the form's failure.
    ended: bool,
    // The value has been read to its end.
    done: bool,
    // The parser reads one part at a time, and this makes that a borrow.
    form: &'a mut Multipart,
}

impl<'a> Field<'a> {
    fn new(head: Head, form: &'a mut Multipart) -> Self {
        let file = head.file_name.is_some();
        let limit = if file {
            form.limits.max_file_bytes.unwrap_or(u64::MAX)
        } else {
            form.limits.max_field_bytes
        };
        Self {
            head,
            limit,
            read: 0,
            file,
            ended: false,
            done: false,
            form,
        }
    }

    /// The `name` of its `Content-Disposition`, as the client sent it.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.head.name.as_deref()
    }

    /// The file name of its `Content-Disposition` (`filename*` when it
    /// decodes, else `filename`), as the client sent it: a label, not a
    /// path. See [`Multipart`].
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.head.file_name.as_deref()
    }

    /// Its `Content-Type`, as the client sent it.
    #[must_use]
    pub fn content_type(&self) -> Option<&str> {
        self.head.content_type.as_ref().map(AsRef::as_ref)
    }

    /// Its headers.
    #[must_use]
    pub fn headers(&self) -> &HeaderMap {
        &self.head.headers
    }

    /// The next piece of its value, `None` after the last. A failure stays:
    /// calling again answers the same failure.
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, MultipartError> {
        if let Some(error) = &self.form.failed {
            return Err(error.clone());
        }
        self.next().await.transpose()
    }

    /// Its whole value.
    pub async fn bytes(mut self) -> Result<Bytes, MultipartError> {
        let mut value = bytes::BytesMut::new();
        while let Some(chunk) = self.chunk().await? {
            value.extend_from_slice(&chunk);
        }
        Ok(value.freeze())
    }

    /// Its whole value as text, in the charset of its `Content-Type` (UTF-8
    /// by default, invalid sequences replaced). This holds to
    /// `max_field_bytes` even for a part with a file name.
    pub async fn text(mut self) -> Result<String, MultipartError> {
        let charset = self
            .head
            .content_type
            .as_ref()
            .and_then(|media_type| media_type.get_param(mime::CHARSET))
            .and_then(|charset| Encoding::for_label(charset.as_str().as_bytes()));
        if self.file && self.limit > self.form.limits.max_field_bytes {
            self.limit = self.form.limits.max_field_bytes;
            self.file = false;
        }
        let bytes = self.bytes().await?;
        Ok(charset.unwrap_or(UTF_8).decode(&bytes).0.into_owned())
    }

    fn too_large(&self) -> MultipartError {
        if self.file {
            MultipartError::FileTooLarge { limit: self.limit }
        } else {
            MultipartError::FieldTooLarge { limit: self.limit }
        }
    }
}

impl Stream for Field<'_> {
    type Item = Result<Bytes, MultipartError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        // One failure, then the end: a stream that fails for ever is a spin
        // for a loop that does not stop at an error.
        if this.ended || this.done {
            return Poll::Ready(None);
        }
        let failure = match ready!(this.form.poll_read(cx, Parser::piece)) {
            Ok(Piece::End) => {
                this.done = true;
                return Poll::Ready(None);
            }
            Ok(Piece::Bytes(chunk)) => {
                this.read = this.read.saturating_add(chunk.len() as u64);
                if this.read <= this.limit {
                    return Poll::Ready(Some(Ok(chunk)));
                }
                let failure = this.too_large();
                this.form.fail(failure)
            }
            Err(error) => error,
        };
        this.ended = true;
        Poll::Ready(Some(Err(failure)))
    }
}
