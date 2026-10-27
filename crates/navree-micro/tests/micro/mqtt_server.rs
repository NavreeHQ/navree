//! A small MQTT 5 broker on rumqttc's packet codec: CONNECT, SUBSCRIBE
//! (with `$share/<group>/` shared subscriptions and subscription
//! identifiers), UNSUBSCRIBE, QoS 1 PUBLISH with PUBACK ("no matching
//! subscribers" when nobody gets it), PINGREQ and DISCONNECT, so the MQTT
//! transport runs against a broker in the tests. Sessions are not kept
//! after a disconnect, and unacknowledged messages are not resent.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::mqttbytes::v5::{
    Codec, ConnAck, ConnectReturnCode, Packet, PingResp, PubAck, PubAckReason, Publish, SubAck,
    SubscribeReasonCode, UnsubAck, UnsubAckReason,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::codec::Framed;
use tokio_util::sync::CancellationToken;

/// A running broker; stops when dropped.
pub struct MiniMqtt {
    addr: SocketAddr,
    task: JoinHandle<()>,
    state: Arc<State>,
    stop: CancellationToken,
}

impl MiniMqtt {
    /// Starts a broker on a free local port.
    pub async fn start() -> MiniMqtt {
        Self::start_on("127.0.0.1:0").await
    }

    /// Starts a broker on `addr` (retrying while the port is still taken).
    pub async fn start_on(addr: &str) -> MiniMqtt {
        let mut tries = 0;
        let listener = loop {
            match TcpListener::bind(addr).await {
                Ok(listener) => break listener,
                Err(error) if tries < 100 => {
                    tries += 1;
                    let _ = error;
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(error) => panic!("bind {addr}: {error}"),
            }
        };
        let addr = listener.local_addr().expect("addr");
        let state = Arc::new(State::default());
        let shared = state.clone();
        let stop = CancellationToken::new();
        let stopping = stop.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve(stream, shared.clone(), stopping.clone()));
            }
        });
        MiniMqtt { addr, task, state, stop }
    }

    /// `mqtt://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("mqtt://{}", self.addr)
    }

    /// The topics published so far, in order.
    pub fn published(&self) -> Vec<String> {
        self.state.published.lock().unwrap().clone()
    }

    /// The client ids connected now.
    pub fn clients(&self) -> Vec<String> {
        self.state.clients.lock().unwrap().values().cloned().collect()
    }
}

impl Drop for MiniMqtt {
    fn drop(&mut self) {
        self.task.abort();
        self.stop.cancel();
    }
}

#[derive(Default)]
struct State {
    subs: Mutex<Vec<Sub>>,
    turns: Mutex<HashMap<String, usize>>,
    next_conn: AtomicU64,
    published: Mutex<Vec<String>>,
    clients: Mutex<HashMap<u64, String>>,
}

#[derive(Clone)]
struct Sub {
    conn: u64,
    filter: String,
    group: Option<String>,
    id: Option<usize>,
    out: mpsc::UnboundedSender<Publish>,
}

/// Whether topic filter `filter` matches `topic`.
fn matches(filter: &str, topic: &str) -> bool {
    let mut levels = topic.split('/');
    for part in filter.split('/') {
        match (part, levels.next()) {
            ("#", _) => return true,
            ("+", Some(_)) => {}
            (part, Some(level)) if part == level => {}
            _ => return false,
        }
    }
    levels.next().is_none()
}

impl State {
    /// Sends `publish` to every plain subscriber and one member of each
    /// shared group; returns how many got it.
    fn route(&self, publish: &Publish) -> usize {
        let topic = String::from_utf8_lossy(&publish.topic).into_owned();
        self.published.lock().unwrap().push(topic.clone());
        let subs = self.subs.lock().unwrap();
        let mut chosen: Vec<&Sub> = Vec::new();
        let mut groups: HashMap<String, Vec<&Sub>> = HashMap::new();
        for sub in subs.iter().filter(|sub| matches(&sub.filter, &topic)) {
            match &sub.group {
                Some(group) => groups.entry(format!("{group}|{}", sub.filter)).or_default().push(sub),
                None => chosen.push(sub),
            }
        }
        let mut turns = self.turns.lock().unwrap();
        for (key, members) in groups {
            let turn = turns.entry(key).or_default();
            chosen.push(members[*turn % members.len()]);
            *turn += 1;
        }
        let mut delivered = 0;
        for sub in chosen {
            let mut copy = publish.clone();
            copy.dup = false;
            copy.retain = false;
            let mut properties = copy.properties.take().unwrap_or_default();
            properties.subscription_identifiers = sub.id.into_iter().collect();
            copy.properties = Some(properties);
            if sub.out.send(copy).is_ok() {
                delivered += 1;
            }
        }
        delivered
    }
}

async fn serve(stream: TcpStream, state: Arc<State>, stop: CancellationToken) {
    let conn = state.next_conn.fetch_add(1, Ordering::SeqCst);
    let mut framed = Framed::new(
        stream,
        Codec {
            max_incoming_size: None,
            max_outgoing_size: None,
        },
    );
    let (out, mut outgoing) = mpsc::unbounded_channel::<Publish>();
    let mut next_pkid: u16 = 0;
    loop {
        tokio::select! {
            () = stop.cancelled() => break,
            packet = framed.next() => {
                let Some(Ok(packet)) = packet else { break };
                let reply = match packet {
                    Packet::Connect(connect, _, _) => {
                        state.clients.lock().unwrap().insert(conn, connect.client_id);
                        Some(Packet::ConnAck(ConnAck {
                            session_present: false,
                            code: ConnectReturnCode::Success,
                            properties: None,
                        }))
                    }
                    Packet::Subscribe(subscribe) => {
                        let id = subscribe.properties.as_ref().and_then(|p| p.id);
                        let mut codes = Vec::new();
                        let mut subs = state.subs.lock().unwrap();
                        for filter in subscribe.filters {
                            let (group, path) = match filter.path.strip_prefix("$share/") {
                                Some(rest) => match rest.split_once('/') {
                                    Some((group, path)) => (Some(group.to_owned()), path.to_owned()),
                                    None => (None, rest.to_owned()),
                                },
                                None => (None, filter.path.clone()),
                            };
                            subs.retain(|s| !(s.conn == conn && s.filter == path && s.group == group));
                            subs.push(Sub { conn, filter: path, group, id, out: out.clone() });
                            codes.push(SubscribeReasonCode::Success(QoS::AtLeastOnce));
                        }
                        Some(Packet::SubAck(SubAck { pkid: subscribe.pkid, return_codes: codes, properties: None }))
                    }
                    Packet::Unsubscribe(unsubscribe) => {
                        let mut subs = state.subs.lock().unwrap();
                        for path in &unsubscribe.filters {
                            let plain = path.strip_prefix("$share/").and_then(|rest| rest.split_once('/')).map_or(path.as_str(), |(_, p)| p);
                            subs.retain(|s| !(s.conn == conn && s.filter == plain));
                        }
                        Some(Packet::UnsubAck(UnsubAck {
                            pkid: unsubscribe.pkid,
                            reasons: vec![UnsubAckReason::Success; unsubscribe.filters.len()],
                            properties: None,
                        }))
                    }
                    Packet::Publish(publish) => {
                        let delivered = state.route(&publish);
                        (publish.qos != QoS::AtMostOnce).then(|| {
                            let mut ack = PubAck::new(publish.pkid, None);
                            if delivered == 0 {
                                ack.reason = PubAckReason::NoMatchingSubscribers;
                            }
                            Packet::PubAck(ack)
                        })
                    }
                    Packet::PingReq(_) => Some(Packet::PingResp(PingResp)),
                    Packet::Disconnect(_) => break,
                    _ => None,
                };
                if let Some(reply) = reply && framed.send(reply).await.is_err() {
                    break;
                }
            }
            publish = outgoing.recv() => {
                let Some(mut publish) = publish else { break };
                next_pkid = next_pkid.wrapping_add(1).max(1);
                publish.pkid = next_pkid;
                publish.qos = QoS::AtLeastOnce;
                if framed.send(Packet::Publish(publish)).await.is_err() {
                    break;
                }
            }
        }
    }
    state.subs.lock().unwrap().retain(|sub| sub.conn != conn);
    state.clients.lock().unwrap().remove(&conn);
}

#[test]
fn filters_match_topics() {
    assert!(matches("a/+", "a/b"));
    assert!(!matches("a/+", "a/b/c"));
    assert!(matches("a/#", "a/b/c"));
    assert!(matches("a/b", "a/b"));
}
