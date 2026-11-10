//! Bodies as real clients write them: Python `requests`, curl and Node's
//! `FormData`, kept as they came off the wire (`clients/NAME.http`: the
//! content type, a line feed, the body).

use std::path::PathBuf;

use crate::support::*;

fn clients() -> Vec<(String, String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/multipart/clients");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            let line = bytes.iter().position(|byte| *byte == b'\n').unwrap();
            let content_type = String::from_utf8(bytes[..line].to_vec()).unwrap();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (name, content_type, bytes[line + 1..].to_vec())
        })
        .collect()
}

/// A part as `(name, file name, value)`.
type Seen = (Option<String>, Option<String>, Vec<u8>);

async fn parts(content_type: &str, chunks: Vec<Vec<u8>>) -> Result<Vec<Seen>, String> {
    let parts = read(request_of(content_type, chunks), wide())
        .await
        .map_err(|error| error.to_string())?;
    Ok(parts
        .into_iter()
        .map(|part| (part.name, part.file_name, part.bytes))
        .collect())
}

/// The names a client wrote with a backslash last. A quoted string that ends
/// in one is a string some readers close and others don't, so a form that
/// holds one is refused.
const REFUSED: [&str; 5] = [
    "curl_backslash_end",
    "node_backslash_end",
    "node_f_back",
    "requests_backslash_end",
    "requests_f_back",
];

#[tokio::test]
async fn bodies_written_by_real_clients_are_read_however_they_arrive() {
    let clients = clients();
    assert_eq!(clients.len(), 50);
    let mut refused = Vec::new();
    for (name, content_type, body) in &clients {
        let whole = parts(content_type, vec![body.clone()]).await;
        let dripped = parts(content_type, body.iter().map(|byte| vec![*byte]).collect()).await;
        assert_eq!(whole, dripped, "{name}: split byte by byte");
        let seven = body.chunks(7).map(<[u8]>::to_vec).collect();
        assert_eq!(
            whole,
            parts(content_type, seven).await,
            "{name}: chunks of 7"
        );
        match whole {
            Ok(parts) => assert!(
                REFUSED.iter().all(|refused| refused != name),
                "{name} was read: {parts:?}"
            ),
            Err(error) => {
                assert!(error.contains("Content-Disposition"), "{name}: {error}");
                refused.push(name.as_str());
            }
        }
    }
    assert_eq!(refused, REFUSED);
}

/// What a client meant is what comes out: the field names and file names.
#[tokio::test]
async fn real_clients_names_come_back_as_they_meant_them() {
    let near_miss = b"\r\n--x\r\nfile with a near miss\r\n--\r\n".to_vec();
    let clients = clients();
    let read = |name: &str| {
        let (_, content_type, body) = clients.iter().find(|(n, ..)| n == name).unwrap();
        let (content_type, body) = (content_type.clone(), body.clone());
        async move { parts(&content_type, vec![body]).await.unwrap() }
    };
    for client in ["requests", "node"] {
        for (case, file_name) in [
            ("plain", "plain.txt"),
            ("space", "two words.txt"),
            ("unicode", "r\u{e9}sum\u{e9} \u{65e5}\u{672c}\u{8a9e}.txt"),
        ] {
            let seen = read(&format!("{client}_{case}")).await;
            assert_eq!(
                seen,
                vec![
                    (Some("a".into()), None, b"1".to_vec()),
                    (Some("b".into()), None, b"two\r\nlines".to_vec()),
                    (Some("e".into()), None, Vec::new()),
                    (Some("f".into()), Some(file_name.into()), near_miss.clone()),
                ],
                "{client}_{case}"
            );
        }
    }
}
