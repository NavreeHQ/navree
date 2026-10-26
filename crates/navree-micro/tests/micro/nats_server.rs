//! A small NATS server speaking the core protocol (INFO, CONNECT, PING,
//! SUB with queue groups and wildcards, UNSUB, PUB, HPUB, MSG, HMSG and the
//! 503 "no responders" status), so the NATS transport runs against the real
//! client without a broker. JetStream is not part of it.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::{BufMut, Bytes, BytesMut};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// A running server; stops when dropped.
pub struct MiniNats {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl MiniNats {
    /// Starts a server on a free local port.
    pub async fn start() -> MiniNats {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let state = Arc::new(State::default());
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve(stream, addr, state.clone()));
            }
        });
        MiniNats { addr, task }
    }

    /// `nats://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("nats://{}", self.addr)
    }
}

impl Drop for MiniNats {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
struct State {
    subs: Mutex<Vec<Sub>>,
    turns: Mutex<HashMap<String, usize>>,
    next_conn: AtomicU64,
}

#[derive(Clone)]
struct Sub {
    conn: u64,
    sid: String,
    subject: String,
    queue: Option<String>,
    left: Option<u64>,
    out: mpsc::UnboundedSender<Bytes>,
}

/// Whether subscription subject `filter` (with `*` and `>`) matches `subject`.
fn matches(filter: &str, subject: &str) -> bool {
    let mut subject = subject.split('.');
    for token in filter.split('.') {
        match (token, subject.next()) {
            (">", Some(_)) => return true,
            ("*", Some(_)) => {}
            (token, Some(part)) if token == part => {}
            _ => return false,
        }
    }
    subject.next().is_none()
}

impl State {
    /// Delivers a message to every plain subscriber and one member of each
    /// queue group; returns how many got it.
    fn route(&self, subject: &str, reply: Option<&str>, headers: Option<&[u8]>, payload: &[u8]) -> usize {
        let mut subs = self.subs.lock().unwrap();
        let mut chosen = Vec::new();
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for i in (0..subs.len()).filter(|&i| matches(&subs[i].subject, subject)) {
            match &subs[i].queue {
                Some(queue) => groups.entry(format!("{queue}|{}", subs[i].subject)).or_default().push(i),
                None => chosen.push(i),
            }
        }
        let mut turns = self.turns.lock().unwrap();
        for (key, members) in groups {
            let turn = turns.entry(key).or_default();
            chosen.push(members[*turn % members.len()]);
            *turn += 1;
        }
        let mut delivered = 0;
        for i in chosen {
            let sub = &mut subs[i];
            if sub.out.send(frame(subject, &sub.sid, reply, headers, payload)).is_ok() {
                delivered += 1;
            }
            if let Some(left) = &mut sub.left {
                *left = left.saturating_sub(1);
            }
        }
        subs.retain(|sub| sub.left != Some(0));
        delivered
    }
}

/// A MSG or HMSG frame.
fn frame(subject: &str, sid: &str, reply: Option<&str>, headers: Option<&[u8]>, payload: &[u8]) -> Bytes {
    let mut out = BytesMut::new();
    let reply = reply.map(|r| format!(" {r}")).unwrap_or_default();
    match headers {
        Some(headers) => {
            let total = headers.len() + payload.len();
            out.put_slice(format!("HMSG {subject} {sid}{reply} {} {total}\r\n", headers.len()).as_bytes());
            out.put_slice(headers);
        }
        None => out.put_slice(format!("MSG {subject} {sid}{reply} {}\r\n", payload.len()).as_bytes()),
    }
    out.put_slice(payload);
    out.put_slice(b"\r\n");
    out.freeze()
}

async fn serve(stream: TcpStream, addr: SocketAddr, state: Arc<State>) {
    let conn = state.next_conn.fetch_add(1, Ordering::SeqCst);
    let (read, mut write) = stream.into_split();
    let (out, mut outgoing) = mpsc::unbounded_channel::<Bytes>();
    let info = format!(
        "INFO {{\"server_id\":\"mini\",\"server_name\":\"mini\",\"version\":\"2.10.0\",\"proto\":1,\"host\":\"{}\",\"port\":{},\"headers\":true,\"max_payload\":1048576,\"client_id\":{conn}}}\r\n",
        addr.ip(),
        addr.port()
    );
    let _ = out.send(Bytes::from(info));
    let writer = tokio::spawn(async move {
        while let Some(bytes) = outgoing.recv().await {
            if write.write_all(&bytes).await.is_err() {
                break;
            }
        }
    });
    let mut reader = BufReader::new(read);
    let mut no_responders = false;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = line.trim_end();
        let (op, rest) = text.split_once(' ').unwrap_or((text, ""));
        match op.to_ascii_uppercase().as_str() {
            "CONNECT" => {
                let options: serde_json::Value = serde_json::from_str(rest).unwrap_or_default();
                no_responders = options["no_responders"] == true && options["headers"] == true;
            }
            "PING" => {
                let _ = out.send(Bytes::from_static(b"PONG\r\n"));
            }
            "SUB" => {
                let args: Vec<&str> = rest.split_whitespace().collect();
                let (subject, queue, sid) = match args.as_slice() {
                    [subject, sid] => (*subject, None, *sid),
                    [subject, queue, sid] => (*subject, Some((*queue).to_owned()), *sid),
                    _ => continue,
                };
                state.subs.lock().unwrap().push(Sub {
                    conn,
                    sid: sid.to_owned(),
                    subject: subject.to_owned(),
                    queue,
                    left: None,
                    out: out.clone(),
                });
            }
            "UNSUB" => {
                let args: Vec<&str> = rest.split_whitespace().collect();
                let mut subs = state.subs.lock().unwrap();
                match args.as_slice() {
                    [sid] => subs.retain(|s| !(s.conn == conn && s.sid == *sid)),
                    [sid, max] => {
                        let max: u64 = max.parse().unwrap_or(0);
                        for sub in subs.iter_mut().filter(|s| s.conn == conn && s.sid == *sid) {
                            sub.left = Some(max);
                        }
                        subs.retain(|sub| sub.left != Some(0));
                    }
                    _ => {}
                }
            }
            "PUB" | "HPUB" => {
                let args: Vec<&str> = rest.split_whitespace().collect();
                let with_headers = op.eq_ignore_ascii_case("HPUB");
                let (subject, reply, header_len, total) = match (with_headers, args.as_slice()) {
                    (false, [subject, size]) => (*subject, None, 0, size.parse().unwrap_or(0)),
                    (false, [subject, reply, size]) => (*subject, Some(*reply), 0, size.parse().unwrap_or(0)),
                    (true, [subject, h, t]) => (*subject, None, h.parse().unwrap_or(0), t.parse().unwrap_or(0)),
                    (true, [subject, reply, h, t]) => {
                        (*subject, Some(*reply), h.parse().unwrap_or(0), t.parse().unwrap_or(0))
                    }
                    _ => break,
                };
                let mut body = vec![0u8; total + 2];
                if reader.read_exact(&mut body).await.is_err() {
                    break;
                }
                body.truncate(total);
                let (headers, payload) = if with_headers {
                    (Some(&body[..header_len]), &body[header_len..])
                } else {
                    (None, &body[..])
                };
                let delivered = state.route(subject, reply, headers, payload);
                if delivered == 0
                    && no_responders
                    && let Some(reply) = reply
                {
                    state.route(reply, None, Some(b"NATS/1.0 503\r\n\r\n"), b"");
                }
            }
            _ => {}
        }
    }
    state.subs.lock().unwrap().retain(|sub| sub.conn != conn);
    writer.abort();
}

#[test]
fn subjects_match_with_wildcards() {
    assert!(matches("a.b", "a.b"));
    assert!(matches("a.*", "a.b"));
    assert!(!matches("a.*", "a.b.c"));
    assert!(matches("a.>", "a.b.c"));
    assert!(!matches("a.>", "a"));
    assert!(!matches("a.b", "a.c"));
}
