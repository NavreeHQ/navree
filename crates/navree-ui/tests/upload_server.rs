//! The server side of an upload: size, count and type limits enforced while the bytes
//! stream in, hostile names, and files sent in chunks that are put together only once
//! they pass the same checks as a plain post. Runs with the `storage` feature.
#![cfg(feature = "storage")]

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http::{HeaderMap, Method, Request, StatusCode};
use http_body_util::BodyExt;
use navree_http::body::{Body, BodyLimit};
use navree_http::extract::Multipart;
use navree_http::response::{IntoResponse, Response};
use navree_http::routing::Router;
use navree_storage::{Disk, DiskConfig, Storage, StorageOptions};
use navree_ui::{ChunkStart, Received, Refused, UploadCode, UploadLabels, UploadLimits, Uploads};
use serde_json::Value;
use tower::ServiceExt;
use tower::util::MapRequestLayer;

const MB: u64 = 1024 * 1024;
const KB: usize = 1024;
/// The smallest chunk the server allows.
const PART: usize = 64 * KB;

/// The first bytes of a PNG file.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
/// The first bytes of a Windows executable.
const EXE: &[u8] = b"MZ\x90\0\x03\0\0\0\x04\0\0\0\xff\xff\0\0";

fn with_head(head: &[u8], len: usize) -> Vec<u8> {
    let mut bytes = head.to_vec();
    bytes.resize(len.max(head.len()), 7);
    bytes
}

fn png(len: usize) -> Vec<u8> {
    with_head(PNG, len)
}

fn memory_disk() -> Disk {
    let mut options = StorageOptions {
        default: "scratch".into(),
        ..StorageOptions::default()
    };
    options.disks.clear();
    options.disks.insert("scratch".into(), DiskConfig::Memory);
    Storage::new(&options, b"signing key")
        .expect("a memory storage")
        .default_disk()
}

fn images(max: u64, files: usize) -> UploadLimits {
    UploadLimits::new(max)
        .max_files(files)
        .accept(["image/png", "image/jpeg"])
}

fn uploads(limits: UploadLimits) -> (Arc<Uploads>, Disk) {
    let disk = memory_disk();
    (Arc::new(Uploads::new(disk.clone(), "photos", limits)), disk)
}

// ---- A router around `receive` ---------------------------------------------------------------

async fn upload_route(uploads: Arc<Uploads>, room: Option<usize>, mut form: Multipart) -> Response {
    let received = match room {
        Some(room) => uploads.receive_into(&mut form, room).await,
        None => uploads.receive(&mut form).await,
    };
    match received {
        Ok(received) => received.into_json_response(),
        Err(error) => error.into_response(),
    }
}

fn router(uploads: Arc<Uploads>, body_limit: usize) -> Router {
    router_with_room(uploads, body_limit, None)
}

fn router_with_room(uploads: Arc<Uploads>, body_limit: usize, room: Option<usize>) -> Router {
    Router::new()
        .route(Method::POST, "/upload", move |form: Multipart| {
            upload_route(uploads.clone(), room, form)
        })
        .route_layer(MapRequestLayer::new(move |mut request: Request<Body>| {
            request.extensions_mut().insert(BodyLimit(body_limit));
            request
        }))
}

const BOUNDARY: &str = "navree-upload-boundary";

/// A form of parts: `(field name, file name, content type, bytes)`; no file name makes a text part.
fn form(parts: &[(&str, Option<&str>, &str, &[u8])]) -> Request<Body> {
    let mut body = Vec::new();
    for (field, file, content_type, contents) in parts {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field}\"").as_bytes(),
        );
        if let Some(file) = file {
            body.extend_from_slice(
                format!("; filename=\"{file}\"\r\nContent-Type: {content_type}").as_bytes(),
            );
        }
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(contents);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    // Sent in 1 KiB chunks that arrive one at a time, like a network upload.
    let chunks: Vec<Bytes> = body.chunks(KB).map(Bytes::copy_from_slice).collect();
    let slow = futures_util::stream::iter(chunks)
        .enumerate()
        .then(|(at, chunk)| async move {
            if at > 0 {
                tokio::task::yield_now().await;
            }
            Ok::<_, std::io::Error>(chunk)
        });
    Request::post("/upload")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from_stream(slow))
        .unwrap()
}

async fn send(router: &Router, request: Request<Body>) -> (u16, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&body)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&body)));
    (status, json)
}

async fn stored_files(disk: &Disk) -> Vec<String> {
    disk.list("photos").await.unwrap()
}

// ---- A plain post ------------------------------------------------------------------------------

// UI-GENUI-021: a file inside the limits is stored under a name of the server's own.
#[tokio::test]
async fn a_good_file_is_stored_under_a_random_name_with_the_type_it_has() {
    let (uploads, disk) = uploads(images(MB, 2));
    let router = router(uploads, 2 * MB as usize);
    let contents = png(300 * KB);
    // The client claims a JPEG; the content is a PNG.
    let (status, body) = send(
        &router,
        form(&[(
            "photo",
            Some("C:\\fakepath\\Harleen Kaur.jpg"),
            "image/jpeg",
            &contents,
        )]),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["refused"], serde_json::json!([]));
    assert_eq!(body["stored"][0]["name"], "Harleen Kaur.jpg");
    assert_eq!(body["stored"][0]["mime"], "image/png");
    assert_eq!(body["stored"][0]["size"], 300 * KB);
    let files = stored_files(&disk).await;
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(
        files[0].starts_with("photos/") && files[0].ends_with(".png"),
        "{files:?}"
    );
    assert!(
        !files[0].contains("Harleen"),
        "the client's name is not the file's: {files:?}"
    );
    assert_eq!(
        disk.get(&files[0]).await.unwrap().as_ref(),
        contents.as_slice()
    );
}

// UI-GENUI-021: the type is read from the bytes, never from the name or the header.
#[tokio::test]
async fn a_renamed_program_and_a_script_dressed_as_an_image_are_refused() {
    let (uploads, disk) = uploads(images(MB, 3));
    let router = router(uploads, 2 * MB as usize);
    let (status, body) = send(
        &router,
        form(&[
            (
                "photo",
                Some("photo.png"),
                "image/png",
                &with_head(EXE, 4 * KB),
            ),
            (
                "photo",
                Some("logo.png"),
                "image/png",
                b"<svg onload=alert(1)></svg>",
            ),
        ]),
    )
    .await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["stored"], serde_json::json!([]));
    let refused = body["refused"].as_array().unwrap();
    assert_eq!(refused.len(), 2, "{body}");
    for (item, name) in refused.iter().zip(["photo.png", "logo.png"]) {
        assert_eq!(item["code"], "wrong_type", "{body}");
        assert_eq!(item["name"], name);
        let message = item["message"].as_str().unwrap();
        assert!(
            message.contains(name) && message.contains("PNG or JPG"),
            "{message}"
        );
    }
    assert!(stored_files(&disk).await.is_empty(), "nothing was written");
}

// UI-GENUI-021: the size is counted while the bytes stream in, and a partial file is thrown away.
#[tokio::test]
async fn a_file_over_the_limit_is_refused_and_leaves_nothing_behind() {
    let (uploads, disk) = uploads(images(MB, 2));
    let router = router(uploads, 4 * MB as usize);
    let (status, body) = send(
        &router,
        form(&[(
            "photo",
            Some("huge.png"),
            "image/png",
            &png(2 * MB as usize),
        )]),
    )
    .await;
    assert_eq!(status, 413, "{body}");
    assert_eq!(body["refused"][0]["code"], "too_large");
    assert_eq!(
        body["refused"][0]["message"],
        "huge.png is larger than 1 MB."
    );
    assert!(stored_files(&disk).await.is_empty());
}

// UI-GENUI-021: an empty file is an empty file, not a stored zero.
#[tokio::test]
async fn an_empty_file_is_refused_in_words() {
    let (uploads, disk) = uploads(images(MB, 1));
    let router = router(uploads, MB as usize);
    let (status, body) = send(
        &router,
        form(&[("photo", Some("blank.png"), "image/png", b"")]),
    )
    .await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["refused"][0]["code"], "empty");
    assert_eq!(body["refused"][0]["message"], "blank.png is empty.");
    assert!(stored_files(&disk).await.is_empty());
}

// UI-GENUI-021: the count limit holds for one request, and the app can say how many places are left.
#[tokio::test]
async fn files_past_the_count_are_refused_one_by_one() {
    let (uploads, disk) = uploads(images(MB, 2));
    let router = router(uploads.clone(), 4 * MB as usize);
    let one = png(KB);
    let (status, body) = send(
        &router,
        form(&[
            ("photo", Some("a.png"), "image/png", &one),
            ("photo", Some("b.png"), "image/png", &one),
            ("photo", Some("c.png"), "image/png", &one),
        ]),
    )
    .await;
    // Two were kept: the request is not a failure.
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["stored"].as_array().unwrap().len(), 2);
    assert_eq!(body["refused"][0]["code"], "too_many");
    assert_eq!(body["refused"][0]["name"], "c.png");
    assert_eq!(body["refused"][0]["message"], "You can send up to 2 files.");
    assert_eq!(stored_files(&disk).await.len(), 2);

    // The app counts the files a person already has and passes the room that is left.
    let app = router_with_room(uploads, 4 * MB as usize, Some(0));
    let (status, body) = send(&app, form(&[("photo", Some("d.png"), "image/png", &one)])).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["refused"][0]["code"], "too_many");
}

// UI-GENUI-021: a name is for display only.
#[tokio::test]
async fn hostile_file_names_are_cut_to_a_plain_name_and_never_become_a_path() {
    let (uploads, disk) = uploads(UploadLimits::new(MB).max_files(10));
    let router = router(uploads, 4 * MB as usize);
    let long = format!("{}.txt", "a".repeat(3000));
    let text = b"plain words\n";
    let names = [
        "../../etc/passwd",
        "..\\..\\windows\\win.ini",
        "con.txt",
        ".htaccess",
        "ਗੁਰਪ੍ਰੀਤ ਸਿੰਘ.txt",
        long.as_str(),
        "x/.txt",
    ];
    let parts: Vec<(&str, Option<&str>, &str, &[u8])> = names
        .iter()
        .map(|name| ("photo", Some(*name), "text/plain", &text[..]))
        .collect();
    let (_, body) = send(&router, form(&parts)).await;
    let refused = body["refused"].as_array().unwrap();
    let stored = body["stored"].as_array().unwrap();
    assert_eq!(stored.len() + refused.len(), names.len(), "{body}");
    for item in stored {
        let name = item["name"].as_str().unwrap();
        assert!(!name.contains('/') && !name.contains('\\'), "{name}");
        assert!(!name.chars().any(char::is_control), "{name:?}");
    }
    for path in stored_files(&disk).await {
        let file = path.strip_prefix("photos/").expect("under the directory");
        assert!(!file.contains('/') && file.len() <= 60, "{path}");
        assert!(
            file.chars().all(|c| c.is_ascii_alphanumeric() || c == '.'),
            "{path}"
        );
    }
}

// UI-GENUI-021: two files with one name are two files.
#[tokio::test]
async fn two_files_with_one_name_are_stored_apart() {
    let (uploads, disk) = uploads(images(MB, 2));
    let router = router(uploads, 4 * MB as usize);
    let (status, body) = send(
        &router,
        form(&[
            ("photo", Some("same.png"), "image/png", &png(KB)),
            ("photo", Some("same.png"), "image/png", &png(2 * KB)),
        ]),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let files = stored_files(&disk).await;
    assert_eq!(files.len(), 2, "{files:?}");
}

// UI-GENUI-021: text parts are not files, and an empty choice is nothing.
#[tokio::test]
async fn text_parts_and_an_empty_choice_are_not_files() {
    let (uploads, disk) = uploads(images(MB, 2));
    let router = router(uploads, MB as usize);
    let (status, body) = send(
        &router,
        form(&[
            ("note", None, "", b"hello"),
            // A browser sends this part when no file was chosen.
            ("photo", Some(""), "application/octet-stream", b""),
        ]),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["stored"], serde_json::json!([]));
    assert_eq!(body["refused"], serde_json::json!([]));
    assert!(stored_files(&disk).await.is_empty());
}

// UI-GENUI-021: when the whole body is refused, what the request had stored is taken back.
#[tokio::test]
async fn a_body_over_the_route_limit_removes_what_the_request_stored() {
    let (uploads, disk) = uploads(images(MB, 3));
    let router = router(uploads, 8 * KB);
    let (status, _) = send(
        &router,
        form(&[
            ("photo", Some("small.png"), "image/png", &png(KB)),
            ("photo", Some("big.png"), "image/png", &png(100 * KB)),
        ]),
    )
    .await;
    assert_eq!(status, 413);
    assert!(
        stored_files(&disk).await.is_empty(),
        "{:?}",
        stored_files(&disk).await
    );
}

// UI-GENUI-021: the route's own limit follows the upload's.
#[test]
fn the_route_limits_follow_the_upload_limits() {
    let (uploads, _) = uploads(images(2 * MB, 1));
    assert!(uploads.body_limit() >= 2 * MB as usize && uploads.body_limit() < 3 * MB as usize);
    assert_eq!(uploads.chunk_body_limit(), 4 * MB as usize);
}

// ---- Words -------------------------------------------------------------------------------------

// UI-GENUI-021: every refusal is in the app's words.
#[tokio::test]
async fn refusals_use_the_apps_labels() {
    let disk = memory_disk();
    let labels = UploadLabels {
        too_big: "{name} बहुत बड़ी है (सीमा {max}).".into(),
        ..UploadLabels::default()
    };
    let uploads = Arc::new(Uploads::new(disk, "photos", images(KB as u64, 1)).labels(labels));
    let router = router(uploads, MB as usize);
    let (_, body) = send(
        &router,
        form(&[("photo", Some("फोटो.png"), "image/png", &png(4 * KB))]),
    )
    .await;
    assert_eq!(
        body["refused"][0]["message"],
        "फोटो.png बहुत बड़ी है (सीमा 1 KB)."
    );
}

// ---- What the page gets back ---------------------------------------------------------------------

// UI-GENUI-021: the module asks for JSON; a browser with no script gets a redirect and a toast.
#[test]
fn the_answer_is_json_for_the_module_and_a_redirect_for_a_plain_post() {
    let received = Received {
        stored: vec![],
        refused: vec![Refused {
            name: "a.exe".into(),
            code: UploadCode::WrongType,
            message: "a.exe is not an accepted kind of file. Accepted: PNG or JPG.".into(),
        }],
    };
    assert_eq!(received.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let mut json = HeaderMap::new();
    json.insert("accept", "application/json".parse().unwrap());
    let response = received.clone().respond(&json, "/profile");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );

    let plain = received.respond(&HeaderMap::new(), "/profile");
    assert_eq!(plain.status(), StatusCode::SEE_OTHER);
    assert_eq!(plain.headers()["location"], "/profile");
    assert!(
        plain.headers().get("set-cookie").is_some(),
        "the refusal is carried to the next page as a toast"
    );
}

// UI-GENUI-021: a toast says what happened.
#[test]
fn the_summary_says_what_was_kept_and_what_was_not() {
    let labels = UploadLabels::default();
    let none = Received::default();
    assert_eq!(none.summary(&labels).1, "Choose a file to send.");
    let sent = Received {
        stored: vec![navree_storage::StoredFile {
            disk: "scratch".into(),
            path: "photos/x.png".into(),
            size: 10,
            mime: "image/png".into(),
            original_name: Some("x.png".into()),
        }],
        refused: vec![],
    };
    assert_eq!(sent.summary(&labels).1, "1 file sent.");
}

// ---- Chunks ----------------------------------------------------------------------------------------

fn start(name: &str, size: usize) -> ChunkStart {
    ChunkStart {
        name: name.to_owned(),
        size: size as u64,
    }
}

async fn put_all(
    uploads: &Uploads,
    owner: &str,
    id: &str,
    data: &[u8],
    order: &[u32],
    step: usize,
) {
    for index in order {
        let from = *index as usize * step;
        let to = (from + step).min(data.len());
        uploads
            .put_chunk(owner, id, *index, Bytes::copy_from_slice(&data[from..to]))
            .await
            .unwrap_or_else(|e| panic!("chunk {index}: {}", e.message));
    }
}

// UI-GENUI-021: the server names the part and the number of parts; the id cannot be guessed.
#[tokio::test]
async fn a_session_names_the_part_size_and_an_id_that_cannot_be_guessed() {
    let (uploads, _) = uploads(UploadLimits::new(20 * MB));
    let ticket = uploads
        .begin("user-7", start("report.pdf", 10 * MB as usize))
        .await
        .unwrap();
    assert_eq!(ticket.chunks, 3);
    assert_eq!(ticket.chunk_bytes, 4 * MB, "the default part");
    assert!(
        ticket.id.len() == 32
            && ticket
                .id
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "{}",
        ticket.id
    );
    let other = uploads
        .begin("user-7", start("report.pdf", 10 * MB as usize))
        .await
        .unwrap();
    assert_ne!(ticket.id, other.id);
}

// UI-GENUI-021: the real flow, with a small part so a test file needs only a few.
#[tokio::test]
async fn a_file_sent_in_parts_is_put_together_and_the_parts_are_removed() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk.clone(), "photos", images(MB, 1)).chunk_bytes(PART as u64);
    let data = png(2 * PART + 1000);
    let ticket = uploads
        .begin("user-7", start("मेरी फोटो.png", data.len()))
        .await
        .unwrap();
    assert_eq!((ticket.chunks, ticket.chunk_bytes), (3, PART as u64));
    put_all(&uploads, "user-7", &ticket.id, &data, &[2, 0, 1, 1], PART).await;
    let stored = uploads.finish("user-7", &ticket.id).await.unwrap();
    assert_eq!(stored.mime, "image/png");
    assert_eq!(stored.size, data.len() as u64);
    assert_eq!(stored.original_name.as_deref(), Some("मेरी फोटो.png"));
    assert!(
        stored.path.starts_with("photos/") && stored.path.ends_with(".png"),
        "{}",
        stored.path
    );
    assert_eq!(
        disk.get(&stored.path).await.unwrap().as_ref(),
        data.as_slice()
    );
    let left = disk.list("photos").await.unwrap();
    assert_eq!(
        left,
        vec![stored.path.clone()],
        "the parts and the note about them are gone"
    );
    // The same session cannot be finished twice.
    let again = uploads.finish("user-7", &ticket.id).await.unwrap_err();
    assert_eq!(again.code, UploadCode::Unknown);
}

// UI-GENUI-021: somebody else's session is not found, whatever they send.
#[tokio::test]
async fn another_users_session_and_made_up_ids_are_not_found() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk, "photos", images(MB, 1)).chunk_bytes(PART as u64);
    let data = png(PART + 10);
    let ticket = uploads
        .begin("user-7", start("a.png", data.len()))
        .await
        .unwrap();
    let body = Bytes::copy_from_slice(&data[..PART]);
    assert_eq!(
        uploads
            .put_chunk("user-8", &ticket.id, 0, body.clone())
            .await
            .unwrap_err()
            .code,
        UploadCode::Unknown
    );
    assert_eq!(
        uploads.finish("user-8", &ticket.id).await.unwrap_err().code,
        UploadCode::Unknown
    );
    assert_eq!(
        uploads.abort("user-8", &ticket.id).await.unwrap_err().code,
        UploadCode::Unknown
    );
    let long = "a".repeat(33);
    for id in [
        "",
        "../../etc",
        "ABCDEF0123456789ABCDEF0123456789",
        "short",
        long.as_str(),
        "0123456789abcdef0123456789abcdef",
    ] {
        let err = uploads
            .put_chunk("user-7", id, 0, body.clone())
            .await
            .unwrap_err();
        assert_eq!(err.code, UploadCode::Unknown, "{id:?}");
        assert_eq!(err.code.status(), StatusCode::NOT_FOUND);
    }
    // The owner's session is untouched by any of it.
    uploads
        .put_chunk("user-7", &ticket.id, 0, body)
        .await
        .unwrap();
}

// UI-GENUI-021: the size of every part is the one the server named.
#[tokio::test]
async fn parts_of_the_wrong_size_or_number_are_refused() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk, "photos", images(MB, 1)).chunk_bytes(PART as u64);
    let data = png(2 * PART + 5);
    let ticket = uploads
        .begin("u", start("a.png", data.len()))
        .await
        .unwrap();
    let part = |n: usize| Bytes::copy_from_slice(&data[..n]);
    // Too short for a middle part, too long for any, and the last one has its own size.
    assert_eq!(
        uploads
            .put_chunk("u", &ticket.id, 0, part(PART - 1))
            .await
            .unwrap_err()
            .code,
        UploadCode::BadChunk
    );
    assert_eq!(
        uploads
            .put_chunk("u", &ticket.id, 0, part(PART + 1))
            .await
            .unwrap_err()
            .code,
        UploadCode::TooLarge
    );
    assert_eq!(
        uploads
            .put_chunk("u", &ticket.id, 2, part(PART))
            .await
            .unwrap_err()
            .code,
        UploadCode::BadChunk
    );
    assert_eq!(
        uploads
            .put_chunk("u", &ticket.id, 3, part(5))
            .await
            .unwrap_err()
            .code,
        UploadCode::BadChunk
    );
    assert_eq!(
        uploads
            .put_chunk("u", &ticket.id, u32::MAX, part(5))
            .await
            .unwrap_err()
            .code,
        UploadCode::BadChunk
    );
    uploads
        .put_chunk("u", &ticket.id, 2, part(5))
        .await
        .unwrap();
}

// UI-GENUI-021: a file with a part missing is not finished, and can still be.
#[tokio::test]
async fn a_missing_part_stops_the_finish_and_a_retry_completes_it() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk.clone(), "photos", images(MB, 1)).chunk_bytes(PART as u64);
    let data = png(2 * PART);
    let ticket = uploads
        .begin("u", start("a.png", data.len()))
        .await
        .unwrap();
    put_all(&uploads, "u", &ticket.id, &data, &[0], PART).await;
    let err = uploads.finish("u", &ticket.id).await.unwrap_err();
    assert_eq!(err.code, UploadCode::BadChunk);
    put_all(&uploads, "u", &ticket.id, &data, &[1], PART).await;
    let stored = uploads.finish("u", &ticket.id).await.unwrap();
    assert_eq!(stored.size, data.len() as u64);
}

// UI-GENUI-021: the totals are held at the start, before a byte is sent.
#[tokio::test]
async fn a_file_over_the_limit_or_empty_is_refused_when_it_begins() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk.clone(), "photos", images(MB, 1)).chunk_bytes(PART as u64);
    let big = uploads
        .begin("u", start("big.png", MB as usize + 1))
        .await
        .unwrap_err();
    assert_eq!(big.code, UploadCode::TooLarge);
    assert_eq!(big.message, "big.png is larger than 1 MB.");
    assert_eq!(big.code.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let empty = uploads.begin("u", start("none.png", 0)).await.unwrap_err();
    assert_eq!(empty.code, UploadCode::Empty);
    assert!(
        disk.list("photos").await.unwrap().is_empty(),
        "a refusal writes nothing"
    );
}

// UI-GENUI-021: the type is checked on the assembled bytes, with the plain post's sniffer.
#[tokio::test]
async fn parts_that_make_a_program_are_refused_and_removed() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk.clone(), "photos", images(MB, 1)).chunk_bytes(PART as u64);
    let data = with_head(EXE, PART + 20);
    let ticket = uploads
        .begin("u", start("photo.png", data.len()))
        .await
        .unwrap();
    put_all(&uploads, "u", &ticket.id, &data, &[0, 1], PART).await;
    let err = uploads.finish("u", &ticket.id).await.unwrap_err();
    assert_eq!(err.code, UploadCode::WrongType);
    assert!(
        err.message.contains("photo.png") && err.message.contains("PNG or JPG"),
        "{}",
        err.message
    );
    assert!(
        disk.list("photos").await.unwrap().is_empty(),
        "nothing is left"
    );
}

// UI-GENUI-021: a person can take back a file they were sending.
#[tokio::test]
async fn aborting_removes_the_parts() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk.clone(), "photos", images(MB, 1)).chunk_bytes(PART as u64);
    let data = png(PART + 1);
    let ticket = uploads
        .begin("u", start("a.png", data.len()))
        .await
        .unwrap();
    put_all(&uploads, "u", &ticket.id, &data, &[0], PART).await;
    assert!(!disk.list("photos").await.unwrap().is_empty());
    uploads.abort("u", &ticket.id).await.unwrap();
    assert!(disk.list("photos").await.unwrap().is_empty());
}

// UI-GENUI-021: a session that is not finished in time is gone, and sweeping finds it.
#[tokio::test]
async fn an_old_session_expires_and_a_sweep_removes_it() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk.clone(), "photos", images(MB, 1))
        .chunk_bytes(PART as u64)
        .chunk_ttl(Duration::ZERO);
    let data = png(PART + 1);
    let ticket = uploads
        .begin("u", start("a.png", data.len()))
        .await
        .unwrap();
    let body = Bytes::copy_from_slice(&data[..PART]);
    assert_eq!(
        uploads
            .put_chunk("u", &ticket.id, 0, body)
            .await
            .unwrap_err()
            .code,
        UploadCode::Unknown
    );
    assert!(uploads.sweep().await >= 1);
    assert!(disk.list("photos").await.unwrap().is_empty());
}

// UI-GENUI-021: a name from the client is cut to a plain name when a session begins.
#[tokio::test]
async fn the_name_of_a_chunked_file_is_a_plain_name() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk, "photos", UploadLimits::new(MB)).chunk_bytes(PART as u64);
    let data = b"plain text\n".to_vec();
    let ticket = uploads
        .begin("u", start("..\\..\\etc\\notes\0.txt", data.len()))
        .await
        .unwrap();
    uploads
        .put_chunk("u", &ticket.id, 0, Bytes::from(data))
        .await
        .unwrap();
    let stored = uploads.finish("u", &ticket.id).await.unwrap();
    let name = stored.original_name.unwrap();
    assert!(
        !name.contains('\\') && !name.contains('/') && !name.contains('\0'),
        "{name:?}"
    );
}

// UI-GENUI-021: the two ways in cannot disagree: the same name and bytes get the same answer.
#[tokio::test]
async fn a_plain_post_and_a_chunked_one_accept_and_refuse_the_same_files() {
    let cases: Vec<(&str, Vec<u8>, bool)> = vec![
        ("photo.png", png(5 * KB), true),
        ("photo.png", with_head(EXE, 5 * KB), false),
        (
            "logo.png",
            b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec(),
            false,
        ),
        (
            "table.csv",
            b"naam,sheher\nSimran Kaur,Ludhiana\n".to_vec(),
            true,
        ),
        ("table.csv", with_head(EXE, 100), false),
        ("note.txt", b"just words".to_vec(), false),
    ];
    for (name, bytes, allowed) in cases {
        let limits = UploadLimits::new(MB).accept(["image/png", "text/csv"]);
        // The plain post.
        let (uploads_plain, disk_plain) = uploads(limits.clone());
        let router = router(uploads_plain, 2 * MB as usize);
        let (_, body) = send(
            &router,
            form(&[("f", Some(name), "application/octet-stream", &bytes)]),
        )
        .await;
        let by_post = body["stored"].as_array().unwrap().len() == 1;
        // The chunked one.
        let disk = memory_disk();
        let chunked = Uploads::new(disk.clone(), "photos", limits).chunk_bytes(PART as u64);
        let ticket = chunked.begin("u", start(name, bytes.len())).await.unwrap();
        for (index, part) in bytes.chunks(PART).enumerate() {
            chunked
                .put_chunk("u", &ticket.id, index as u32, Bytes::copy_from_slice(part))
                .await
                .unwrap();
        }
        let by_chunks = chunked.finish("u", &ticket.id).await;
        assert_eq!(by_post, allowed, "{name}: post {body}");
        assert_eq!(by_chunks.is_ok(), allowed, "{name}: chunks");
        if let Ok(stored) = by_chunks {
            assert_eq!(stored.mime, body["stored"][0]["mime"], "{name}");
        }
        assert_eq!(stored_files(&disk_plain).await.len(), usize::from(allowed));
    }
}

// UI-GENUI-021: the answer to a failed step is JSON the module can read.
#[tokio::test]
async fn an_error_is_json_with_a_message() {
    let (uploads, _) = uploads(images(MB, 1));
    let err = uploads
        .finish("u", "0123456789abcdef0123456789abcdef")
        .await
        .unwrap_err();
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["refused"][0]["code"], "unknown");
    assert!(
        json["message"].as_str().unwrap().contains("expired"),
        "{json}"
    );
}

// ---- Chunked uploads under load ------------------------------------------------------------

/// A local disk in a directory of its own, removed when the test ends. Its reads and writes
/// wait on the file system, so uploads that run at the same time really do.
struct LocalDisk {
    disk: Disk,
    root: std::path::PathBuf,
}

impl Drop for LocalDisk {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn local_disk() -> LocalDisk {
    let root = std::env::temp_dir().join(format!(
        "navree-ui-uploads-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&root).expect("a directory for the disk");
    let mut options = StorageOptions {
        default: "scratch".into(),
        ..StorageOptions::default()
    };
    options.disks.clear();
    options
        .disks
        .insert("scratch".into(), DiskConfig::local(root.clone()));
    let disk = Storage::new(&options, b"signing key")
        .expect("a local storage")
        .default_disk();
    LocalDisk { disk, root }
}

/// A retry that overtakes the first `finish`, or a page that sends it twice: the parts are put
/// together and stored once, and every other call finds the upload finished.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finishes_at_the_same_moment_store_the_file_once() {
    let local = local_disk();
    let uploads = Arc::new(
        Uploads::new(local.disk.clone(), "photos", images(MB, 1)).chunk_bytes(PART as u64),
    );
    let data = png(3 * PART + 7);
    let ticket = uploads
        .begin("u", start("a.png", data.len()))
        .await
        .unwrap();
    put_all(&uploads, "u", &ticket.id, &data, &[0, 1, 2, 3], PART).await;

    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let finishes: Vec<_> = (0..8)
        .map(|_| {
            let (uploads, barrier, id) = (uploads.clone(), barrier.clone(), ticket.id.clone());
            tokio::spawn(async move {
                barrier.wait().await;
                uploads.finish("u", &id).await
            })
        })
        .collect();
    let mut stored = Vec::new();
    for finish in finishes {
        match finish.await.unwrap() {
            Ok(file) => stored.push(file),
            Err(error) => assert_eq!(error.code, UploadCode::Unknown, "{}", error.message),
        }
    }
    assert_eq!(stored.len(), 1, "{stored:?}");
    assert_eq!(
        local.disk.list("photos").await.unwrap(),
        vec![stored[0].path.clone()],
        "one file, and no parts or notes left"
    );
    assert_eq!(
        local.disk.get(&stored[0].path).await.unwrap().as_ref(),
        data.as_slice()
    );
}

/// Nobody fills the disk with the parts of files that never end: an owner has so many
/// unfinished uploads at once, and finishing or stopping one makes room.
#[tokio::test]
async fn an_owner_has_only_so_many_unfinished_uploads() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk, "photos", images(MB, 1))
        .chunk_bytes(PART as u64)
        .chunk_sessions(2);
    let first = uploads.begin("u", start("a.png", PART)).await.unwrap();
    let _second = uploads.begin("u", start("b.png", PART)).await.unwrap();
    let refused = uploads.begin("u", start("c.png", PART)).await.unwrap_err();
    assert_eq!(refused.code, UploadCode::TooMany);
    assert_eq!(refused.code.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.message, "You can send up to 2 files.");

    uploads
        .begin("v", start("a.png", PART))
        .await
        .expect("another owner has room of its own");

    uploads.abort("u", &first.id).await.unwrap();
    let third = uploads.begin("u", start("c.png", PART)).await.unwrap();
    put_all(&uploads, "u", &third.id, &png(PART), &[0], PART).await;
    uploads.finish("u", &third.id).await.unwrap();
    uploads
        .begin("u", start("d.png", PART))
        .await
        .expect("a finished upload is not counted");
}

/// An upload that ran out of time counts for nothing.
#[tokio::test]
async fn an_expired_upload_leaves_its_owner_room() {
    let disk = memory_disk();
    let uploads = Uploads::new(disk.clone(), "photos", images(MB, 1))
        .chunk_bytes(PART as u64)
        .chunk_ttl(Duration::ZERO)
        .chunk_sessions(1);
    uploads.begin("u", start("a.png", PART)).await.unwrap();
    uploads.begin("u", start("b.png", PART)).await.unwrap();
    assert_eq!(
        disk.list("photos").await.unwrap().len(),
        1,
        "the expired one was removed"
    );
}

/// The begins of one owner take turns: a burst of them opens no more than the limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_burst_of_begins_opens_no_more_than_the_limit() {
    let local = local_disk();
    let uploads = Arc::new(
        Uploads::new(local.disk.clone(), "photos", images(MB, 1))
            .chunk_bytes(PART as u64)
            .chunk_sessions(3),
    );
    let barrier = Arc::new(tokio::sync::Barrier::new(10));
    let begins: Vec<_> = (0..10)
        .map(|n| {
            let (uploads, barrier) = (uploads.clone(), barrier.clone());
            tokio::spawn(async move {
                barrier.wait().await;
                uploads.begin("u", start(&format!("{n}.png"), PART)).await
            })
        })
        .collect();
    let mut opened = 0;
    for begin in begins {
        match begin.await.unwrap() {
            Ok(_) => opened += 1,
            Err(error) => assert_eq!(error.code, UploadCode::TooMany, "{}", error.message),
        }
    }
    assert_eq!(opened, 3);
}
