//! A small Redis server speaking RESP2 with the stream commands the Redis
//! transport uses (XADD, XGROUP CREATE, XREADGROUP and XREAD with BLOCK,
//! XACK, XCLAIM, XAUTOCLAIM, XPENDING, XINFO GROUPS, XREVRANGE, XTRIM,
//! EXPIRE), consumer groups and their pending lists, so the transport runs
//! against the real client without a server. Every other command answers
//! `+OK`.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// A running server; stops when dropped.
pub struct MiniRedis {
    addr: SocketAddr,
    task: JoinHandle<()>,
    state: Arc<State>,
}

impl MiniRedis {
    /// Starts a server on a free local port.
    pub async fn start() -> MiniRedis {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let state = Arc::new(State::default());
        let shared = state.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve(stream, shared.clone()));
            }
        });
        MiniRedis { addr, task, state }
    }

    /// `redis://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("redis://{}", self.addr)
    }

    /// The entries of stream `key`, as their `payload` fields.
    pub fn payloads(&self, key: &str) -> Vec<String> {
        let data = self.state.data.lock().unwrap();
        data.streams
            .get(key)
            .map(|stream| {
                stream
                    .entries
                    .values()
                    .filter_map(|fields| fields.iter().find(|(name, _)| name == b"payload"))
                    .map(|(_, value)| String::from_utf8_lossy(value).into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether stream `key` exists.
    pub fn has_stream(&self, key: &str) -> bool {
        self.state.data.lock().unwrap().streams.contains_key(key)
    }

    /// Makes the server slow with `command` (`XGROUP`, `XREVRANGE`): it takes `delay` to
    /// start on it, as a busy server would, so what a client does in that time happens first.
    pub fn slow(&self, command: &str, delay: std::time::Duration) {
        self.state
            .delays
            .lock()
            .unwrap()
            .insert(command.to_owned(), delay);
    }
}

impl Drop for MiniRedis {
    fn drop(&mut self) {
        self.task.abort();
    }
}

type Id = (u64, u64);
type Fields = Vec<(Vec<u8>, Vec<u8>)>;

#[derive(Default)]
struct State {
    data: Mutex<Data>,
    added: Notify,
    /// How long the server takes to start on a command: see `MiniRedis::slow`.
    delays: Mutex<HashMap<String, std::time::Duration>>,
}

#[derive(Default)]
struct Data {
    streams: HashMap<String, Stream>,
}

#[derive(Default)]
struct Stream {
    entries: BTreeMap<Id, Fields>,
    last: Id,
    groups: BTreeMap<String, Group>,
}

#[derive(Default)]
struct Group {
    last_delivered: Id,
    pending: BTreeMap<Id, Pending>,
}

struct Pending {
    consumer: String,
    delivered_at: Instant,
    count: u64,
}

/// A RESP2 reply.
enum Reply {
    Simple(&'static str),
    Error(String),
    Int(i64),
    Bulk(Vec<u8>),
    NilBulk,
    NilArray,
    Array(Vec<Reply>),
}

impl Reply {
    fn text(text: impl Into<String>) -> Reply {
        Reply::Bulk(text.into().into_bytes())
    }

    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Reply::Simple(text) => out.extend_from_slice(format!("+{text}\r\n").as_bytes()),
            Reply::Error(text) => out.extend_from_slice(format!("-{text}\r\n").as_bytes()),
            Reply::Int(n) => out.extend_from_slice(format!(":{n}\r\n").as_bytes()),
            Reply::Bulk(bytes) => {
                out.extend_from_slice(format!("${}\r\n", bytes.len()).as_bytes());
                out.extend_from_slice(bytes);
                out.extend_from_slice(b"\r\n");
            }
            Reply::NilBulk => out.extend_from_slice(b"$-1\r\n"),
            Reply::NilArray => out.extend_from_slice(b"*-1\r\n"),
            Reply::Array(items) => {
                out.extend_from_slice(format!("*{}\r\n", items.len()).as_bytes());
                for item in items {
                    item.write(out);
                }
            }
        }
    }
}

fn format_id((ms, seq): Id) -> String {
    format!("{ms}-{seq}")
}

fn parse_id(text: &str) -> Option<Id> {
    match text.split_once('-') {
        Some((ms, seq)) => Some((ms.parse().ok()?, seq.parse().ok()?)),
        None => Some((text.parse().ok()?, 0)),
    }
}

fn entry(id: Id, fields: &Fields) -> Reply {
    Reply::Array(vec![
        Reply::text(format_id(id)),
        Reply::Array(
            fields
                .iter()
                .flat_map(|(name, value)| [Reply::Bulk(name.clone()), Reply::Bulk(value.clone())])
                .collect(),
        ),
    ])
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn upper(arg: &[u8]) -> String {
    String::from_utf8_lossy(arg).to_ascii_uppercase()
}

fn text(arg: &[u8]) -> String {
    String::from_utf8_lossy(arg).into_owned()
}

/// Reads one command (an array of bulk strings).
async fn read_command(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> Option<Vec<Vec<u8>>> {
    let mut line = String::new();
    if reader.read_line(&mut line).await.ok()? == 0 {
        return None;
    }
    let count: usize = line.trim_end().strip_prefix('*')?.parse().ok()?;
    let mut args = Vec::with_capacity(count);
    for _ in 0..count {
        line.clear();
        reader.read_line(&mut line).await.ok()?;
        let len: usize = line.trim_end().strip_prefix('$')?.parse().ok()?;
        let mut bytes = vec![0u8; len + 2];
        reader.read_exact(&mut bytes).await.ok()?;
        bytes.truncate(len);
        args.push(bytes);
    }
    Some(args)
}

async fn serve(stream: TcpStream, state: Arc<State>) {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    while let Some(args) = read_command(&mut reader).await {
        // A blocked read ends when its client goes away, as in Redis.
        let executing = execute(&state, &args);
        tokio::pin!(executing);
        let reply = tokio::select! {
            reply = &mut executing => reply,
            more = reader.fill_buf() => match more {
                Ok(bytes) if !bytes.is_empty() => executing.await,
                _ => break,
            },
        };
        let mut out = Vec::new();
        reply.write(&mut out);
        if write.write_all(&out).await.is_err() {
            break;
        }
    }
}

async fn execute(state: &State, args: &[Vec<u8>]) -> Reply {
    let Some(name) = args.first() else {
        return Reply::Error("ERR empty command".into());
    };
    let delay = state.delays.lock().unwrap().get(&upper(name)).copied();
    if let Some(delay) = delay {
        tokio::time::sleep(delay).await;
    }
    match upper(name).as_str() {
        "PING" => Reply::Simple("PONG"),
        "XADD" => xadd(state, &args[1..]),
        "XGROUP" => xgroup(state, &args[1..]),
        "XREADGROUP" | "XREAD" => read(state, &upper(name), &args[1..]).await,
        "XACK" => xack(state, &args[1..]),
        "XCLAIM" => xclaim(state, &args[1..]),
        "XAUTOCLAIM" => xautoclaim(state, &args[1..]),
        "XPENDING" => xpending(state, &args[1..]),
        "XINFO" => xinfo(state, &args[1..]),
        "XREVRANGE" => xrevrange(state, &args[1..]),
        "XTRIM" => xtrim(state, &args[1..]),
        "EXPIRE" => Reply::Int(1),
        "DEL" => {
            let mut data = state.data.lock().unwrap();
            let removed = args[1..]
                .iter()
                .filter(|key| data.streams.remove(&text(key)).is_some())
                .count();
            Reply::Int(i64::try_from(removed).unwrap())
        }
        _ => Reply::Simple("OK"),
    }
}

fn xadd(state: &State, args: &[Vec<u8>]) -> Reply {
    let key = text(&args[0]);
    let mut i = 1;
    let mut max_len = None;
    while i < args.len() && upper(&args[i]) != "*" {
        match upper(&args[i]).as_str() {
            "MAXLEN" => {
                i += 1;
                if matches!(text(&args[i]).as_str(), "~" | "=") {
                    i += 1;
                }
                max_len = text(&args[i]).parse::<usize>().ok();
            }
            "NOMKSTREAM" => {}
            _ => return Reply::Error("ERR syntax error".into()),
        }
        i += 1;
    }
    let fields: Fields = args[i + 1..]
        .chunks(2)
        .filter(|pair| pair.len() == 2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect();
    let mut data = state.data.lock().unwrap();
    let stream = data.streams.entry(key).or_default();
    let ms = now_ms();
    let id = if ms > stream.last.0 {
        (ms, 0)
    } else {
        (stream.last.0, stream.last.1 + 1)
    };
    stream.last = id;
    stream.entries.insert(id, fields);
    if let Some(max_len) = max_len {
        while stream.entries.len() > max_len {
            let first = *stream.entries.keys().next().unwrap();
            stream.entries.remove(&first);
        }
    }
    drop(data);
    state.added.notify_waiters();
    Reply::text(format_id(id))
}

fn xgroup(state: &State, args: &[Vec<u8>]) -> Reply {
    if upper(&args[0]) != "CREATE" {
        return Reply::Simple("OK");
    }
    let key = text(&args[1]);
    let group = text(&args[2]);
    let start = text(&args[3]);
    let mkstream = args.iter().skip(4).any(|arg| upper(arg) == "MKSTREAM");
    let mut data = state.data.lock().unwrap();
    if !data.streams.contains_key(&key) && !mkstream {
        return Reply::Error("ERR The XGROUP subcommand requires the key to exist".into());
    }
    let stream = data.streams.entry(key).or_default();
    if stream.groups.contains_key(&group) {
        return Reply::Error("BUSYGROUP Consumer Group name already exists".into());
    }
    let last_delivered = if start == "$" {
        stream.last
    } else {
        parse_id(&start).unwrap_or_default()
    };
    stream.groups.insert(
        group,
        Group {
            last_delivered,
            pending: BTreeMap::new(),
        },
    );
    Reply::Simple("OK")
}

async fn read(state: &State, command: &str, args: &[Vec<u8>]) -> Reply {
    let mut i = 0;
    let mut group = None;
    let mut count = usize::MAX;
    let mut block = None;
    while i < args.len() {
        match upper(&args[i]).as_str() {
            "GROUP" => {
                group = Some((text(&args[i + 1]), text(&args[i + 2])));
                i += 3;
            }
            "COUNT" => {
                count = text(&args[i + 1]).parse().unwrap_or(usize::MAX);
                i += 2;
            }
            "BLOCK" => {
                block = text(&args[i + 1]).parse::<u64>().ok();
                i += 2;
            }
            "NOACK" => i += 1,
            "STREAMS" => {
                i += 1;
                break;
            }
            _ => return Reply::Error("ERR syntax error".into()),
        }
    }
    let rest = &args[i..];
    let half = rest.len() / 2;
    let keys: Vec<String> = rest[..half].iter().map(|key| text(key)).collect();
    let mut ids: Vec<String> = rest[half..].iter().map(|id| text(id)).collect();
    if command == "XREAD" {
        // `$` is the last id now.
        let data = state.data.lock().unwrap();
        for (key, id) in keys.iter().zip(ids.iter_mut()) {
            if id == "$" {
                *id = format_id(data.streams.get(key).map(|s| s.last).unwrap_or_default());
            }
        }
    }
    let deadline = block.map(|ms| {
        if ms == 0 {
            None
        } else {
            Some(Instant::now() + Duration::from_millis(ms))
        }
    });
    loop {
        let notified = state.added.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        match take(state, group.as_ref(), &keys, &ids, count) {
            Err(error) => return error,
            Ok(found) if !found.is_empty() => return Reply::Array(found),
            Ok(_) => {}
        }
        match deadline {
            None => return Reply::NilArray,
            Some(None) => notified.await,
            Some(Some(at)) => {
                let left = at.saturating_duration_since(Instant::now());
                if left.is_zero() || tokio::time::timeout(left, notified).await.is_err() {
                    return Reply::NilArray;
                }
            }
        }
    }
}

/// The entries a read takes now, per stream with any.
fn take(
    state: &State,
    group: Option<&(String, String)>,
    keys: &[String],
    ids: &[String],
    count: usize,
) -> Result<Vec<Reply>, Reply> {
    let mut data = state.data.lock().unwrap();
    let mut found = Vec::new();
    for (key, id) in keys.iter().zip(ids) {
        let mut items = Vec::new();
        match group {
            Some((group, consumer)) => {
                let Some(stream) = data.streams.get_mut(key) else {
                    return Err(Reply::Error(format!(
                        "NOGROUP No such key '{key}' or consumer group '{group}' in XREADGROUP with GROUP option"
                    )));
                };
                let entries = &stream.entries;
                let Some(state) = stream.groups.get_mut(group) else {
                    return Err(Reply::Error(format!(
                        "NOGROUP No such key '{key}' or consumer group '{group}' in XREADGROUP with GROUP option"
                    )));
                };
                if id != ">" {
                    continue;
                }
                let start = state.last_delivered;
                for (entry_id, fields) in entries
                    .range((std::ops::Bound::Excluded(start), std::ops::Bound::Unbounded))
                    .take(count)
                {
                    state.last_delivered = *entry_id;
                    state.pending.insert(
                        *entry_id,
                        Pending {
                            consumer: consumer.clone(),
                            delivered_at: Instant::now(),
                            count: 1,
                        },
                    );
                    items.push(entry(*entry_id, fields));
                }
            }
            None => {
                let Some(stream) = data.streams.get(key) else {
                    continue;
                };
                let after = parse_id(id).unwrap_or_default();
                for (entry_id, fields) in stream
                    .entries
                    .range((std::ops::Bound::Excluded(after), std::ops::Bound::Unbounded))
                    .take(count)
                {
                    items.push(entry(*entry_id, fields));
                }
            }
        }
        if !items.is_empty() {
            found.push(Reply::Array(vec![
                Reply::text(key.clone()),
                Reply::Array(items),
            ]));
        }
    }
    Ok(found)
}

fn xack(state: &State, args: &[Vec<u8>]) -> Reply {
    let key = text(&args[0]);
    let group = text(&args[1]);
    let mut data = state.data.lock().unwrap();
    let Some(group) = data
        .streams
        .get_mut(&key)
        .and_then(|s| s.groups.get_mut(&group))
    else {
        return Reply::Int(0);
    };
    let acked = args[2..]
        .iter()
        .filter_map(|id| parse_id(&text(id)))
        .filter(|id| group.pending.remove(id).is_some())
        .count();
    Reply::Int(i64::try_from(acked).unwrap())
}

fn xclaim(state: &State, args: &[Vec<u8>]) -> Reply {
    let key = text(&args[0]);
    let group_name = text(&args[1]);
    let consumer = text(&args[2]);
    let min_idle = Duration::from_millis(text(&args[3]).parse().unwrap_or(0));
    let mut ids = Vec::new();
    let mut idle = None;
    let mut justid = false;
    let mut i = 4;
    while i < args.len() {
        match upper(&args[i]).as_str() {
            "IDLE" => {
                idle = text(&args[i + 1]).parse::<u64>().ok();
                i += 2;
            }
            "JUSTID" => {
                justid = true;
                i += 1;
            }
            _ => {
                if let Some(id) = parse_id(&text(&args[i])) {
                    ids.push(id);
                }
                i += 1;
            }
        }
    }
    let mut data = state.data.lock().unwrap();
    let Some(stream) = data.streams.get_mut(&key) else {
        return Reply::Array(Vec::new());
    };
    let entries = &stream.entries;
    let Some(group) = stream.groups.get_mut(&group_name) else {
        return Reply::Error("NOGROUP".into());
    };
    let mut claimed = Vec::new();
    for id in ids {
        let Some(pending) = group.pending.get_mut(&id) else {
            continue;
        };
        if pending.delivered_at.elapsed() < min_idle {
            continue;
        }
        pending.consumer.clone_from(&consumer);
        pending.delivered_at = Instant::now()
            .checked_sub(Duration::from_millis(idle.unwrap_or(0)))
            .unwrap_or_else(Instant::now);
        if !justid {
            pending.count += 1;
        }
        claimed.push(if justid {
            Reply::text(format_id(id))
        } else {
            entries
                .get(&id)
                .map_or(Reply::NilBulk, |fields| entry(id, fields))
        });
    }
    Reply::Array(claimed)
}

fn xautoclaim(state: &State, args: &[Vec<u8>]) -> Reply {
    let key = text(&args[0]);
    let group_name = text(&args[1]);
    let consumer = text(&args[2]);
    let min_idle = Duration::from_millis(text(&args[3]).parse().unwrap_or(0));
    let start = parse_id(&text(&args[4])).unwrap_or_default();
    let count = args
        .iter()
        .position(|arg| upper(arg) == "COUNT")
        .and_then(|i| text(&args[i + 1]).parse::<usize>().ok())
        .unwrap_or(100);
    let mut data = state.data.lock().unwrap();
    let Some(stream) = data.streams.get_mut(&key) else {
        return Reply::Error("NOGROUP".into());
    };
    let entries = &stream.entries;
    let Some(group) = stream.groups.get_mut(&group_name) else {
        return Reply::Error("NOGROUP".into());
    };
    let mut claimed = Vec::new();
    for (id, pending) in group.pending.range_mut(start..) {
        if claimed.len() >= count {
            break;
        }
        if pending.delivered_at.elapsed() < min_idle {
            continue;
        }
        pending.consumer.clone_from(&consumer);
        pending.delivered_at = Instant::now();
        pending.count += 1;
        claimed.push(
            entries
                .get(id)
                .map_or(Reply::NilBulk, |fields| entry(*id, fields)),
        );
    }
    Reply::Array(vec![
        Reply::text("0-0"),
        Reply::Array(claimed),
        Reply::Array(Vec::new()),
    ])
}

fn xpending(state: &State, args: &[Vec<u8>]) -> Reply {
    let key = text(&args[0]);
    let group_name = text(&args[1]);
    let mut rest = &args[2..];
    if rest.first().is_some_and(|arg| upper(arg) == "IDLE") {
        rest = &rest[2..];
    }
    let parse = |arg: &Vec<u8>, default: Id| match text(arg).as_str() {
        "-" => (0, 0),
        "+" => (u64::MAX, u64::MAX),
        other => parse_id(other).unwrap_or(default),
    };
    let start = rest.first().map_or((0, 0), |arg| parse(arg, (0, 0)));
    let end = rest
        .get(1)
        .map_or((u64::MAX, u64::MAX), |arg| parse(arg, (u64::MAX, u64::MAX)));
    let data = state.data.lock().unwrap();
    let Some(group) = data
        .streams
        .get(&key)
        .and_then(|s| s.groups.get(&group_name))
    else {
        return Reply::Array(Vec::new());
    };
    Reply::Array(
        group
            .pending
            .range(start..=end)
            .map(|(id, pending)| {
                Reply::Array(vec![
                    Reply::text(format_id(*id)),
                    Reply::text(pending.consumer.clone()),
                    Reply::Int(i64::try_from(pending.delivered_at.elapsed().as_millis()).unwrap()),
                    Reply::Int(i64::try_from(pending.count).unwrap()),
                ])
            })
            .collect(),
    )
}

fn xinfo(state: &State, args: &[Vec<u8>]) -> Reply {
    if upper(&args[0]) != "GROUPS" {
        return Reply::Error("ERR unknown subcommand".into());
    }
    let key = text(&args[1]);
    let data = state.data.lock().unwrap();
    let Some(stream) = data.streams.get(&key) else {
        return Reply::Error("ERR no such key".into());
    };
    Reply::Array(
        stream
            .groups
            .iter()
            .map(|(name, group)| {
                Reply::Array(vec![
                    Reply::text("name"),
                    Reply::text(name.clone()),
                    Reply::text("pending"),
                    Reply::Int(i64::try_from(group.pending.len()).unwrap()),
                    Reply::text("last-delivered-id"),
                    Reply::text(format_id(group.last_delivered)),
                ])
            })
            .collect(),
    )
}

fn xrevrange(state: &State, args: &[Vec<u8>]) -> Reply {
    let key = text(&args[0]);
    let count = args
        .iter()
        .position(|arg| upper(arg) == "COUNT")
        .and_then(|i| text(&args[i + 1]).parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    let data = state.data.lock().unwrap();
    let Some(stream) = data.streams.get(&key) else {
        return Reply::Array(Vec::new());
    };
    Reply::Array(
        stream
            .entries
            .iter()
            .rev()
            .take(count)
            .map(|(id, fields)| entry(*id, fields))
            .collect(),
    )
}

fn xtrim(state: &State, args: &[Vec<u8>]) -> Reply {
    let key = text(&args[0]);
    let strategy = upper(&args[1]);
    let mut i = 2;
    if matches!(text(&args[i]).as_str(), "~" | "=") {
        i += 1;
    }
    let mut data = state.data.lock().unwrap();
    let Some(stream) = data.streams.get_mut(&key) else {
        return Reply::Int(0);
    };
    let before = stream.entries.len();
    match strategy.as_str() {
        "MINID" => {
            let min = parse_id(&text(&args[i])).unwrap_or_default();
            stream.entries.retain(|id, _| *id >= min);
        }
        _ => {
            let max: usize = text(&args[i]).parse().unwrap_or(usize::MAX);
            while stream.entries.len() > max {
                let first = *stream.entries.keys().next().unwrap();
                stream.entries.remove(&first);
            }
        }
    }
    Reply::Int(i64::try_from(before - stream.entries.len()).unwrap())
}

#[test]
fn ids_format_and_parse() {
    assert_eq!(format_id((5, 1)), "5-1");
    assert_eq!(parse_id("5-1"), Some((5, 1)));
    assert_eq!(parse_id("7"), Some((7, 0)));
    assert_eq!(parse_id("x"), None);
}
