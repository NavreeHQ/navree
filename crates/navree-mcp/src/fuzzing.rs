//! Entry points for the fuzz targets in `fuzz/` (feature `fuzzing`): the
//! JSON-RPC parser and the stdio line framing, which the transports run on
//! bytes a client sends. Not part of the API.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use serde_json::Value;
use tokio::io::{AsyncRead, BufReader, ReadBuf};

use crate::protocol::jsonrpc::{self, MAX_JSON_DEPTH, Message, Parsed, exceeds_depth};
use crate::stdio::{Line, read_line};

/// How deeply arrays and objects nest in `value`.
fn depth(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// Parses `bytes` as a JSON-RPC message or batch. Returns how many messages
/// it holds (0 when it is refused).
///
/// # Panics
///
/// When an invariant breaks: what parses nests at most
/// [`MAX_JSON_DEPTH`] levels, a refusal serializes to a JSON-RPC error with
/// a code and a message, and the depth check agrees with the parser.
pub fn parse(bytes: &[u8]) -> usize {
    match jsonrpc::parse(bytes) {
        Err((id, error)) => {
            let answer = jsonrpc::error(&id, &error);
            assert_eq!(answer["jsonrpc"], "2.0");
            assert!(answer["error"]["code"].is_i64());
            assert!(answer["error"]["message"].is_string());
            assert!(answer["id"].is_null() || answer["id"].is_string() || answer["id"].is_i64() || answer["id"].is_u64());
            0
        }
        Ok(parsed) => {
            let value: Value = serde_json::from_slice(bytes).expect("what parses is JSON");
            assert!(
                depth(&value) <= MAX_JSON_DEPTH,
                "a message nested {} levels was accepted",
                depth(&value)
            );
            assert!(!exceeds_depth(bytes, MAX_JSON_DEPTH));
            match parsed {
                Parsed::Single(message) => usize::from(matches!(
                    message,
                    Message::Request(_) | Message::Notification(_) | Message::Reply
                )),
                Parsed::Batch(entries) => {
                    for (id, error) in entries.iter().filter_map(|entry| entry.as_ref().err()) {
                        let _ = jsonrpc::error(id, error);
                    }
                    entries.len()
                }
            }
        }
    }
}

/// An in-memory reader that yields at most `chunk` bytes per read.
struct Chunked<'a> {
    data: &'a [u8],
    chunk: usize,
}

impl AsyncRead for Chunked<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let take = self.chunk.min(self.data.len()).min(buf.remaining());
        let (head, tail) = self.data.split_at(take);
        buf.put_slice(head);
        self.data = tail;
        Poll::Ready(Ok(()))
    }
}

/// Reads `data` the way the stdio transport does, `chunk` bytes at a time,
/// and returns each line: its bytes, or `None` for a line longer than
/// `max`.
fn lines(data: &[u8], max: usize, chunk: usize) -> Vec<Option<Vec<u8>>> {
    futures::executor::block_on(async {
        let mut reader = BufReader::with_capacity(chunk.max(1), Chunked { data, chunk });
        let mut out = Vec::new();
        loop {
            match read_line(&mut reader, max).await.expect("memory reads") {
                Line::End => return out,
                Line::TooLong => out.push(None),
                Line::Message(line) => out.push(Some(line)),
            }
        }
    })
}

/// Frames `data` with a line limit of `max` bytes, read in chunks of every
/// size in `chunks`, and returns the number of lines.
///
/// # Panics
///
/// When an invariant breaks: the lines are `data` split at newlines (a last
/// line without one counts when it is not empty), a line longer than `max`
/// is reported as too long and never kept, and the chunk size changes
/// nothing.
pub fn frame(data: &[u8], max: usize, chunks: &[usize]) -> usize {
    let mut expected: Vec<Option<Vec<u8>>> = data
        .split(|byte| *byte == b'\n')
        .map(|line| (line.len() <= max).then(|| line.to_vec()))
        .collect();
    if data.last().is_none_or(|last| *last == b'\n') {
        // Nothing follows the last newline.
        expected.pop();
    }
    for chunk in chunks {
        assert_eq!(
            lines(data, max, (*chunk).max(1)),
            expected,
            "framing with chunks of {chunk}"
        );
    }
    expected.len()
}
