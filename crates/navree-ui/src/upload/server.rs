//! The server side of an upload: files received through navree-storage and held to the
//! limits while they stream in, and large files put together from chunks.
//!
//! Nothing the browser sends is believed. The size is counted as bytes arrive, the kind is read
//! from the first bytes of the file, the name is text to show and never a path, and the two
//! ways in, a plain post and chunks, run the same checks on the same sniffer.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use futures_util::stream;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use navree_http::AppError;
use navree_http::extract::Multipart;
use navree_http::response::{IntoResponse, Response};
use navree_storage::{
    Disk, SNIFF_BYTES, StorageError, StoredFile, sniff_type, store_upload, upload_path,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::OwnedMutexGuard;
use uuid::Uuid;

use super::labels::{UploadLabels, fill};
use super::limits::UploadLimits;
use crate::html::LocalHref;
use crate::op::{Reply, ToastTone};

/// A part of a chunked upload, unless an app says otherwise: 4 MB.
const DEFAULT_CHUNK: u64 = 4 * 1024 * 1024;
/// The smallest part: 64 KB.
const MIN_CHUNK: u64 = 64 * 1024;
/// The largest part: 16 MB.
const MAX_CHUNK: u64 = 16 * 1024 * 1024;
/// The most parts one file may have.
const MAX_CHUNKS: u64 = 4096;
/// How long an unfinished chunked upload is kept: a day.
const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Where the parts of unfinished uploads are kept, under the upload's directory.
const PARTS: &str = "_parts";
/// How many unfinished uploads one sweep looks at.
const SWEEP_AT_MOST: usize = 200;
/// The longest file name kept, in characters: the tail, so the extension stays.
const NAME_AT_MOST: usize = 255;
/// How many chunked uploads one owner may have unfinished at once, unless an app says
/// otherwise.
const DEFAULT_OPEN: usize = 16;

/// Why a file was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UploadCode {
    /// The file has nothing in it.
    Empty,
    /// The file is larger than the limit.
    TooLarge,
    /// The file is not a kind that is accepted.
    WrongType,
    /// More files than the limit.
    TooMany,
    /// A part of a chunked upload that is the wrong size, or in the wrong place, or missing.
    BadChunk,
    /// A chunked upload that does not exist, is not this user's, or has expired.
    Unknown,
    /// The storage failed.
    Failed,
}

impl UploadCode {
    /// The code as the browser module and the JSON name it: `wrong_type`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooLarge => "too_large",
            Self::WrongType => "wrong_type",
            Self::TooMany => "too_many",
            Self::BadChunk => "bad_chunk",
            Self::Unknown => "unknown",
            Self::Failed => "failed",
        }
    }

    /// The HTTP status a refusal for this reason answers with.
    pub const fn status(self) -> StatusCode {
        match self {
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Unknown => StatusCode::NOT_FOUND,
            Self::Failed => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Empty | Self::WrongType | Self::TooMany | Self::BadChunk => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
        }
    }
}

/// A file that was not kept, and why, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    /// The file's name, cut to a plain name. Text to show.
    pub name: String,
    /// The reason.
    pub code: UploadCode,
    /// The reason in the app's words.
    pub message: String,
}

/// What a request left: the files that were stored and the files that were not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Received {
    /// The files that were stored, each under a name of the server's own.
    pub stored: Vec<StoredFile>,
    /// The files that were refused.
    pub refused: Vec<Refused>,
}

impl Received {
    /// 200 when anything was stored or nothing was refused; otherwise the status of the first
    /// refusal.
    pub fn status(&self) -> StatusCode {
        match (self.stored.is_empty(), self.refused.first()) {
            (true, Some(first)) => first.code.status(),
            _ => StatusCode::OK,
        }
    }

    /// The names, sizes and kinds of what was stored and the reasons for what was not. The
    /// storage paths stay on the server: add your own ids to what you send back.
    pub fn json(&self) -> Value {
        let stored: Vec<Value> = self
            .stored
            .iter()
            .map(|file| {
                json!({
                    "name": file.original_name.clone().unwrap_or_default(),
                    "size": file.size,
                    "mime": file.mime,
                })
            })
            .collect();
        let refused: Vec<Value> = self
            .refused
            .iter()
            .map(|file| {
                json!({
                    "name": file.name,
                    "code": file.code.as_str(),
                    "message": file.message,
                })
            })
            .collect();
        json!({ "stored": stored, "refused": refused })
    }

    /// [`json`](Self::json) as a response, with [`status`](Self::status). This is what the
    /// browser module reads.
    pub fn into_json_response(self) -> Response {
        json_response(self.status(), &self.json())
    }

    /// A toast's tone and text for a person who sent the form with no script: how many files
    /// were kept, and why the others were not.
    pub fn summary(&self, labels: &UploadLabels) -> (ToastTone, String) {
        if self.stored.is_empty() && self.refused.is_empty() {
            return (ToastTone::Warning, labels.nothing.clone());
        }
        let mut parts: Vec<String> = Vec::new();
        match self.stored.len() {
            0 => {}
            1 => parts.push(labels.sent_one.clone()),
            count => {
                let count = count.to_string();
                parts.push(fill(&labels.sent_many, &[("count", count.as_str())]));
            }
        }
        parts.extend(self.refused.iter().take(3).map(|file| file.message.clone()));
        let tone = match (self.stored.is_empty(), self.refused.is_empty()) {
            (_, true) => ToastTone::Success,
            (true, false) => ToastTone::Danger,
            (false, false) => ToastTone::Warning,
        };
        (tone, parts.join(" "))
    }

    /// The answer to a request: JSON for the browser module, which asks for it, and for
    /// everything else a redirect to `back` with a toast, in English. To use your own words,
    /// call [`Uploads::respond`].
    pub fn respond(self, headers: &HeaderMap, back: impl Into<LocalHref>) -> Response {
        self.respond_in(headers, back, &UploadLabels::default())
    }

    fn respond_in(
        self,
        headers: &HeaderMap,
        back: impl Into<LocalHref>,
        labels: &UploadLabels,
    ) -> Response {
        let wants_json = headers
            .get_all(header::ACCEPT)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .any(|value| value.contains("application/json"));
        if wants_json {
            return self.into_json_response();
        }
        let (tone, text) = self.summary(labels);
        Reply::redirect(back).toast(tone, text).into_response()
    }
}

/// A failed step of a chunked upload, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadError {
    /// The reason.
    pub code: UploadCode,
    /// The reason in the app's words.
    pub message: String,
}

impl UploadError {
    /// The HTTP status.
    pub fn status(&self) -> StatusCode {
        self.code.status()
    }

    /// The error in Navree's error shape.
    pub fn to_app_error(&self) -> AppError {
        AppError::new(self.status(), self.code.as_str(), self.message.clone())
            .with_detail(self.message.clone())
    }
}

impl IntoResponse for UploadError {
    fn into_response(self) -> Response {
        let body = json!({
            "statusCode": self.status().as_u16(),
            "message": self.message,
            "refused": [{ "name": "", "code": self.code.as_str(), "message": self.message }],
        });
        json_response(self.status(), &body)
    }
}

/// What a chunked upload starts with: the file's name and its size in bytes.
#[derive(Clone, Debug, Deserialize)]
pub struct ChunkStart {
    /// The file's name. Text to show.
    pub name: String,
    /// The file's size, in bytes.
    pub size: u64,
}

/// What a started chunked upload tells the browser.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChunkTicket {
    /// The upload's id: 32 hexadecimal digits that cannot be guessed.
    pub id: String,
    /// The size of every part but the last, in bytes. The browser cuts the file to it.
    pub chunk_bytes: u64,
    /// How many parts there are.
    pub chunks: u32,
}

/// The note kept beside the parts.
#[derive(Debug, Deserialize, Serialize)]
struct Session {
    owner: String,
    name: String,
    size: u64,
    chunk: u64,
    at: u64,
}

/// Receives the files an [`Upload`](crate::Upload) sends, into a directory of a navree-storage
/// disk, holding every file to its [`UploadLimits`].
///
/// There are two ways in, and they run the same checks:
/// - [`receive`](Self::receive) reads a `multipart/form-data` post (a plain form, or the module
///   sending one small file) and streams each file to the disk, counting its size as it goes and
///   reading its kind from its first bytes.
/// - [`begin`](Self::begin), [`put_chunk`](Self::put_chunk) and [`finish`](Self::finish) take a
///   large file in parts, so a slow connection or a closed tab loses one part and not the file.
///   The parts are put together only when every one is here, and the kind is read from the
///   whole file's first bytes before anything is stored under its final name.
///
/// ```no_run
/// use navree_http::extract::Multipart;
/// use navree_http::response::Response;
/// use navree_ui::{UploadLimits, Uploads};
/// # async fn demo(disk: navree_storage::Disk, headers: http::HeaderMap, mut form: Multipart) -> Result<Response, navree_http::AppError> {
/// let uploads = Uploads::new(
///     disk,
///     "photos",
///     UploadLimits::new(2 * 1024 * 1024).accept(["image/png", "image/jpeg"]),
/// );
/// let received = uploads.receive(&mut form).await?;
/// Ok(received.respond(&headers, "/profile"))
/// # }
/// ```
///
/// Chunked uploads are held in check too: one owner has at most
/// [`chunk_sessions`](Self::chunk_sessions) unfinished at once, and an upload is put together
/// once, however many `finish` calls arrive for it at the same moment.
#[derive(Clone)]
pub struct Uploads {
    disk: Disk,
    dir: String,
    limits: UploadLimits,
    labels: UploadLabels,
    chunk_bytes: u64,
    ttl: Duration,
    open: usize,
}

impl Uploads {
    /// Files go to `dir` on `disk`, under random names, and are held to `limits`.
    pub fn new(disk: Disk, dir: impl Into<String>, limits: UploadLimits) -> Self {
        let mut uploads = Self {
            disk,
            dir: dir.into(),
            limits,
            labels: UploadLabels::default(),
            chunk_bytes: DEFAULT_CHUNK,
            ttl: DEFAULT_TTL,
            open: DEFAULT_OPEN,
        };
        uploads.chunk_bytes = uploads.fit_chunk(DEFAULT_CHUNK);
        uploads
    }

    /// Sets the words of the refusals.
    pub fn labels(mut self, labels: UploadLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Sets the size of a part: between 64 KB and 16 MB, and large enough that the largest file
    /// has at most 4,096 parts.
    pub fn chunk_bytes(mut self, bytes: u64) -> Self {
        self.chunk_bytes = self.fit_chunk(bytes);
        self
    }

    /// Sets how long an unfinished chunked upload is kept (a day by default).
    pub fn chunk_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sets how many chunked uploads one owner may have unfinished at once: at least one, 16
    /// by default. [`begin`](Self::begin) refuses another (`too_many`) until one is finished,
    /// aborted or has expired, so nobody fills the disk with parts of files that never end.
    pub fn chunk_sessions(mut self, open: usize) -> Self {
        self.open = open.max(1);
        self
    }

    /// The limits.
    pub fn limits(&self) -> &UploadLimits {
        &self.limits
    }

    /// The body limit for the route that [`receive`](Self::receive)s: the largest file and a
    /// little more for the rest of the form.
    pub fn body_limit(&self) -> usize {
        usize::try_from(self.limits.bytes())
            .unwrap_or(usize::MAX)
            .saturating_add(64 * 1024)
    }

    /// The body limit for the route that takes a part: the size of a part.
    pub fn chunk_body_limit(&self) -> usize {
        usize::try_from(self.chunk_bytes).unwrap_or(usize::MAX)
    }

    fn fit_chunk(&self, bytes: u64) -> u64 {
        bytes
            .max(self.limits.bytes().div_ceil(MAX_CHUNKS))
            .clamp(MIN_CHUNK, MAX_CHUNK)
    }

    /// `tail` under the upload's directory.
    fn at(&self, tail: &str) -> String {
        let dir = self.dir.trim_matches('/');
        if dir.is_empty() {
            tail.to_owned()
        } else {
            format!("{dir}/{tail}")
        }
    }

    /// A file of the session `id` of the owner `key` ([`owner_key`]).
    fn part(&self, key: &str, id: &str, file: &str) -> String {
        self.at(&format!("{PARTS}/{key}/{id}/{file}"))
    }

    /// The turn of `what` of this upload directory, in this process.
    fn turn_key(&self, what: &str) -> String {
        format!("{}\0{}\0{what}", self.disk.name(), self.dir)
    }

    // ---- A plain post --------------------------------------------------------------------

    /// Reads the files of `form` and stores the ones inside the limits. A file that breaks a
    /// limit is refused by name and the rest go on; a form that cannot be read at all, or that
    /// is larger than the route allows, is an error, and what the request stored is taken back.
    ///
    /// At most [`UploadLimits::files`] files are kept. To count the files a person already has,
    /// use [`receive_into`](Self::receive_into).
    ///
    /// # Errors
    /// The form is malformed or too large, or the storage fails.
    pub async fn receive(&self, form: &mut Multipart) -> Result<Received, AppError> {
        self.receive_into(form, self.limits.files()).await
    }

    /// Like [`receive`](Self::receive), keeping at most `room` files: for an app that counts
    /// the files a person has stored and allows more only while there is room.
    ///
    /// # Errors
    /// The form is malformed or too large, or the storage fails.
    pub async fn receive_into(
        &self,
        form: &mut Multipart,
        room: usize,
    ) -> Result<Received, AppError> {
        let mut received = Received::default();
        let mut seen = 0_usize;
        loop {
            let field = match form.next_field().await {
                Ok(Some(field)) => field,
                Ok(None) => break,
                Err(error) => return Err(self.undo(received, error.to_app_error()).await),
            };
            // A part with no file name is text, and a file name of nothing is a browser
            // saying no file was chosen.
            let Some(raw) = field.file_name().map(str::to_owned) else {
                continue;
            };
            let name = clean_name(&raw);
            if name.is_empty() {
                continue;
            }
            seen += 1;
            if seen > room {
                received
                    .refused
                    .push(self.refuse(UploadCode::TooMany, &name));
                continue;
            }
            match store_upload(field, &self.disk, &self.dir, self.limits.rules()).await {
                Ok(stored) => received.stored.push(StoredFile {
                    original_name: Some(name),
                    ..stored
                }),
                Err(error) => match self.refusal(&name, error) {
                    Ok(refused) => received.refused.push(refused),
                    Err(error) => return Err(self.undo(received, error).await),
                },
            }
        }
        Ok(received)
    }

    /// The answer to a request: JSON for the browser module, and for a post with no script a
    /// redirect to `back` with a toast, in this upload's words.
    pub fn respond(
        &self,
        received: Received,
        headers: &HeaderMap,
        back: impl Into<LocalHref>,
    ) -> Response {
        received.respond_in(headers, back, &self.labels)
    }

    /// The refusal `error` of one file, or the error itself when it is not a refusal.
    fn refusal(&self, name: &str, error: AppError) -> Result<Refused, AppError> {
        if error.status == StatusCode::PAYLOAD_TOO_LARGE {
            return Ok(self.refuse(UploadCode::TooLarge, name));
        }
        if let Some(errors) = &error.errors {
            let text = errors
                .values()
                .flatten()
                .cloned()
                .collect::<Vec<String>>()
                .join(" ");
            let code = if text.contains("should not be empty") {
                UploadCode::Empty
            } else {
                UploadCode::WrongType
            };
            return Ok(self.refuse(code, name));
        }
        Err(error)
    }

    fn refuse(&self, code: UploadCode, name: &str) -> Refused {
        Refused {
            name: name.to_owned(),
            code,
            message: self.message(code, name),
        }
    }

    fn error(&self, code: UploadCode, name: &str) -> UploadError {
        UploadError {
            code,
            message: self.message(code, name),
        }
    }

    /// The words for a refusal.
    fn message(&self, code: UploadCode, name: &str) -> String {
        let labels = &self.labels;
        let max = UploadLimits::size_text(self.limits.bytes());
        let types = self.limits.types_text(labels);
        let count = self.limits.files().to_string();
        match code {
            UploadCode::TooLarge => fill(&labels.too_big, &[("name", name), ("max", max.as_str())]),
            UploadCode::WrongType => fill(
                &labels.wrong_type,
                &[("name", name), ("types", types.as_str())],
            ),
            UploadCode::Empty => fill(&labels.empty, &[("name", name)]),
            UploadCode::TooMany => fill(&labels.too_many, &[("count", count.as_str())]),
            UploadCode::Unknown => labels.expired.clone(),
            UploadCode::BadChunk | UploadCode::Failed => fill(&labels.failed, &[("name", name)]),
        }
    }

    /// Takes back what a failed request stored.
    async fn undo(&self, received: Received, error: AppError) -> AppError {
        for file in &received.stored {
            // A file that cannot be removed now is found by whatever sweeps the disk.
            let _ = self.disk.delete(&file.path).await;
        }
        error
    }

    // ---- Chunks ----------------------------------------------------------------------------

    /// Starts a chunked upload for `owner`, who is whoever the app says is sending: a user's id.
    /// Only that owner can send its parts and finish it. The file's size is held to the limit
    /// here, before a byte is sent, and the owner to its
    /// [`chunk_sessions`](Self::chunk_sessions): the begins of one owner take turns, so a burst
    /// of them cannot open more.
    ///
    /// # Errors
    /// The file is empty or larger than the limit, the owner has as many unfinished uploads as
    /// it may, or the storage fails.
    pub async fn begin(&self, owner: &str, start: ChunkStart) -> Result<ChunkTicket, UploadError> {
        let name = clean_name(&start.name);
        let shown = if name.is_empty() {
            "file".to_owned()
        } else {
            name.clone()
        };
        if start.size == 0 {
            return Err(self.error(UploadCode::Empty, &shown));
        }
        let chunks = start.size.div_ceil(self.chunk_bytes);
        if start.size > self.limits.bytes() || chunks > MAX_CHUNKS {
            return Err(self.error(UploadCode::TooLarge, &shown));
        }
        self.sweep().await;
        let key = owner_key(owner);
        let _turn = turn(self.turn_key(&format!("owner\0{key}"))).await;
        let failed = || self.error(UploadCode::Failed, &shown);
        if self.unfinished(owner, &key).await.map_err(|_| failed())? >= self.open {
            let open = self.open.to_string();
            return Err(UploadError {
                code: UploadCode::TooMany,
                message: fill(&self.labels.too_many, &[("count", open.as_str())]),
            });
        }
        let id = Uuid::new_v4().simple().to_string();
        let session = Session {
            owner: owner.to_owned(),
            name,
            size: start.size,
            chunk: self.chunk_bytes,
            at: now(),
        };
        let note = serde_json::to_vec(&session).map_err(|_| failed())?;
        self.disk
            .put(&self.part(&key, &id, "note"), note)
            .await
            .map_err(|_| failed())?;
        Ok(ChunkTicket {
            id,
            chunk_bytes: self.chunk_bytes,
            chunks: u32::try_from(chunks).unwrap_or(u32::MAX),
        })
    }

    /// Keeps part number `index` (from 0) of the upload `id`. Parts may come in any order and a
    /// part may come twice: the last one sent stays. Every part but the last is exactly the
    /// size [`begin`](Self::begin) named; the last is what is left.
    ///
    /// # Errors
    /// The upload is not this owner's or is gone, the part is out of place, of the wrong size,
    /// or larger than a part may be, or the storage fails.
    pub async fn put_chunk(
        &self,
        owner: &str,
        id: &str,
        index: u32,
        body: Bytes,
    ) -> Result<(), UploadError> {
        let session = self.session(owner, id).await?;
        let chunks = session.size.div_ceil(session.chunk);
        if u64::from(index) >= chunks {
            return Err(self.error(UploadCode::BadChunk, &session.name));
        }
        let length = body.len() as u64;
        if length > session.chunk {
            return Err(self.error(UploadCode::TooLarge, &session.name));
        }
        if length != part_size(&session, u64::from(index)) {
            return Err(self.error(UploadCode::BadChunk, &session.name));
        }
        self.disk
            .put(
                &self.part(&owner_key(owner), id, &format!("{index:08}")),
                body,
            )
            .await
            .map_err(|_| self.error(UploadCode::Failed, &session.name))
    }

    /// Puts the parts of the upload `id` together, once every one is here, and stores the file.
    /// The kind is read from the file's first bytes with the sniffer a plain post uses, and a
    /// file of a kind that is not accepted is thrown away with its parts.
    ///
    /// The finishes of one upload take turns: one sent at the same moment as another waits for
    /// it, then finds the upload finished, so the file is put together and stored once.
    ///
    /// # Errors
    /// The upload is not this owner's or is gone, a part is missing, the kind is not accepted,
    /// or the storage fails. After a missing part, the upload stays and can be finished once the
    /// part is sent.
    pub async fn finish(&self, owner: &str, id: &str) -> Result<StoredFile, UploadError> {
        if !valid_id(id) {
            return Err(self.error(UploadCode::Unknown, ""));
        }
        let _turn = turn(self.turn_key(id)).await;
        let session = self.session(owner, id).await?;
        let key = owner_key(owner);
        let failed = || self.error(UploadCode::Failed, &session.name);
        let bad = || self.error(UploadCode::BadChunk, &session.name);
        let chunks = session.size.div_ceil(session.chunk);
        let mut paths: Vec<String> = Vec::new();
        for index in 0..chunks {
            let path = self.part(&key, id, &format!("{index:08}"));
            match self.disk.size(&path).await {
                Ok(size) if size == part_size(&session, index) => paths.push(path),
                Ok(_) | Err(StorageError::NotFound { .. }) => return Err(bad()),
                Err(_) => return Err(failed()),
            }
        }
        let first = paths.first().ok_or_else(bad)?;
        let first = self.disk.get(first).await.map_err(|_| failed())?;
        let head = &first[..first.len().min(SNIFF_BYTES)];
        let sniffed = sniff_type(head, Some(session.name.as_str()));
        if !self.limits.allows(&sniffed.mime) {
            self.discard(&key, id).await;
            return Err(self.error(UploadCode::WrongType, &session.name));
        }
        let path = upload_path(&self.dir, &sniffed.mime);
        let disk = self.disk.clone();
        let parts = Arc::new(paths);
        let joined = stream::try_unfold(0_usize, move |at| {
            let disk = disk.clone();
            let parts = Arc::clone(&parts);
            async move {
                match parts.get(at) {
                    Some(path) => {
                        let bytes = disk.get(path).await?;
                        Ok::<_, StorageError>(Some((bytes, at + 1)))
                    }
                    None => Ok(None),
                }
            }
        });
        let written = self
            .disk
            .put_stream(&path, joined)
            .await
            .map_err(|_| failed())?;
        if written != session.size {
            let _ = self.disk.delete(&path).await;
            return Err(bad());
        }
        self.discard(&key, id).await;
        Ok(StoredFile {
            disk: self.disk.name().to_owned(),
            path,
            size: written,
            mime: sniffed.mime,
            original_name: Some(session.name),
        })
    }

    /// Stops the upload `id` and removes its parts. It waits for a `finish` of the same upload
    /// that is under way.
    ///
    /// # Errors
    /// The upload is not this owner's or is gone.
    pub async fn abort(&self, owner: &str, id: &str) -> Result<(), UploadError> {
        if !valid_id(id) {
            return Err(self.error(UploadCode::Unknown, ""));
        }
        let _turn = turn(self.turn_key(id)).await;
        self.session(owner, id).await?;
        self.discard(&owner_key(owner), id).await;
        Ok(())
    }

    /// Removes the parts of uploads that were not finished in time, and returns how many
    /// uploads that was. [`begin`](Self::begin) does this, a few hundred at a time; an app with
    /// many uploads can also run it from a schedule.
    pub async fn sweep(&self) -> usize {
        let root = self.at(PARTS);
        let Ok(files) = self.disk.list(&root).await else {
            return 0;
        };
        let lead = format!("{root}/");
        let mut ids: BTreeSet<(String, String)> = BTreeSet::new();
        for file in files {
            let mut steps = file.strip_prefix(&lead).unwrap_or_default().split('/');
            if let (Some(key), Some(id)) = (steps.next(), steps.next())
                && valid_id(key)
                && valid_id(id)
            {
                ids.insert((key.to_owned(), id.to_owned()));
            }
        }
        let mut removed = 0;
        for (key, id) in ids.into_iter().take(SWEEP_AT_MOST) {
            if self.stale(&key, &id).await {
                self.discard(&key, &id).await;
                removed += 1;
            }
        }
        removed
    }

    /// How many uploads `owner` (whose key is `key`) has unfinished: the ones that expired are
    /// removed instead of counted.
    async fn unfinished(&self, owner: &str, key: &str) -> Result<usize, StorageError> {
        let root = self.at(&format!("{PARTS}/{key}"));
        let lead = format!("{root}/");
        let ids: BTreeSet<String> = self
            .disk
            .list(&root)
            .await?
            .iter()
            .filter_map(|file| file.strip_prefix(&lead)?.split('/').next())
            .filter(|id| valid_id(id))
            .map(str::to_owned)
            .collect();
        let mut open = 0;
        for id in ids {
            match self.note(key, &id).await {
                Some(session) if !self.expired(session.at) => {
                    open += usize::from(session.owner == owner);
                }
                _ => self.discard(key, &id).await,
            }
        }
        Ok(open)
    }

    /// Whether the session `id` of the owner `key` is past its time, or has no note to say.
    async fn stale(&self, key: &str, id: &str) -> bool {
        self.note(key, id)
            .await
            .is_none_or(|session| self.expired(session.at))
    }

    /// The note of the session `id` of the owner `key`, if it can be read.
    async fn note(&self, key: &str, id: &str) -> Option<Session> {
        let bytes = self.disk.get(&self.part(key, id, "note")).await.ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// The session `id` of `owner`: not found for a made-up id, another owner's, an expired
    /// one or a note that cannot be read, so nobody learns which of these it was.
    async fn session(&self, owner: &str, id: &str) -> Result<Session, UploadError> {
        let unknown = || self.error(UploadCode::Unknown, "");
        if !valid_id(id) {
            return Err(unknown());
        }
        let bytes = match self
            .disk
            .get(&self.part(&owner_key(owner), id, "note"))
            .await
        {
            Ok(bytes) => bytes,
            Err(StorageError::NotFound { .. }) => return Err(unknown()),
            Err(_) => return Err(self.error(UploadCode::Failed, "")),
        };
        let session: Session = serde_json::from_slice(&bytes).map_err(|_| unknown())?;
        if session.owner != owner || session.chunk == 0 || self.expired(session.at) {
            return Err(unknown());
        }
        Ok(session)
    }

    fn expired(&self, at: u64) -> bool {
        now().saturating_sub(at) >= self.ttl.as_secs()
    }

    /// Removes every file of the session `id` of the owner `key`.
    async fn discard(&self, key: &str, id: &str) {
        let prefix = self.at(&format!("{PARTS}/{key}/{id}"));
        if let Ok(files) = self.disk.list(&prefix).await {
            for file in files {
                let _ = self.disk.delete(&file).await;
            }
        }
    }
}

/// The size of part `index` of `session`: a whole part, or what is left for the last.
fn part_size(session: &Session, index: u64) -> u64 {
    let chunks = session.size.div_ceil(session.chunk);
    if index + 1 == chunks {
        session.size - session.chunk * (chunks - 1)
    } else {
        session.chunk
    }
}

/// Where an owner's sessions are kept: 32 hexadecimal digits of the SHA-256 of `owner`, so
/// an owner's uploads are counted by listing one directory, and no owner names a path.
fn owner_key(owner: &str) -> String {
    Sha256::digest(owner.as_bytes())[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Locks by name, held while an upload is begun for an owner or finished: one at a time each,
/// in this process. An entry lives as long as somebody holds or waits for its lock.
static TURNS: LazyLock<Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(Mutex::default);

/// Waits for the turn of `key`, held until the guard is dropped.
async fn turn(key: String) -> OwnedMutexGuard<()> {
    let lock = {
        let mut turns = TURNS.lock().unwrap_or_else(PoisonError::into_inner);
        turns.retain(|_, lock| lock.strong_count() > 0);
        match turns.get(&key).and_then(Weak::upgrade) {
            Some(lock) => lock,
            None => {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                turns.insert(key, Arc::downgrade(&lock));
                lock
            }
        }
    };
    lock.lock_owned().await
}

/// Whether `id` is one this module makes: 32 lower-case hexadecimal digits.
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Seconds since 1970.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// `raw` as a plain name to show: what follows the last `/` or `\`, with control characters
/// taken out and at most 255 characters, the last ones.
fn clean_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned: String = base.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    let count = cleaned.chars().count();
    if count > NAME_AT_MOST {
        cleaned.chars().skip(count - NAME_AT_MOST).collect()
    } else {
        cleaned.to_owned()
    }
}

/// `body` as JSON with `status`.
fn json_response(status: StatusCode, body: &Value) -> Response {
    let mut response = (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        body.to_string(),
    )
        .into_response();
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cut_to_a_plain_name() {
        assert_eq!(
            clean_name("C:\\fakepath\\Meera Joshi.pdf"),
            "Meera Joshi.pdf"
        );
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("a\u{0}b\n.txt"), "ab.txt");
        assert_eq!(clean_name("dir/"), "");
        assert_eq!(clean_name("  "), "");
        let long = format!("{}.csv", "x".repeat(400));
        let cut = clean_name(&long);
        assert_eq!(cut.chars().count(), 255);
        assert!(cut.ends_with(".csv"));
    }

    #[test]
    fn ids_are_32_lower_case_hex_digits() {
        assert!(valid_id("0123456789abcdef0123456789abcdef"));
        for bad in [
            "",
            "0123456789ABCDEF0123456789ABCDEF",
            "../../x",
            "0123456789abcdef0123456789abcde",
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn the_last_part_is_what_is_left() {
        let session = Session {
            owner: String::new(),
            name: String::new(),
            size: 2 * 100 + 5,
            chunk: 100,
            at: 0,
        };
        assert_eq!(
            [0, 1, 2].map(|index| part_size(&session, index)),
            [100, 100, 5]
        );
        let exact = Session {
            size: 200,
            ..session
        };
        assert_eq!([0, 1].map(|index| part_size(&exact, index)), [100, 100]);
    }

    #[test]
    fn every_code_has_a_status_and_a_name() {
        assert_eq!(UploadCode::TooLarge.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(UploadCode::Unknown.status(), StatusCode::NOT_FOUND);
        assert_eq!(UploadCode::WrongType.as_str(), "wrong_type");
    }
}
