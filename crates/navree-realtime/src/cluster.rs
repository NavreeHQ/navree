//! What the hubs of an app tell each other over a [`Backplane`](crate::Backplane):
//! the messages ([`Envelope`], [`Message`]) and the presence members other
//! processes report ([`RemotePresence`]).
//!
//! Every process (a *node*, with a random id) publishes its broadcasts,
//! user disconnections and presence changes. Presence converges through
//! snapshots: a node answers `hello` (sent when it starts) with its
//! members, resends them every [`RESYNC_EVERY`] heartbeats, and says
//! `goodbye` when it shuts down. A node that stops sending heartbeats for
//! [`NODE_TIMEOUT`] (a crash) is forgotten, with its members.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::Instant;

/// How often a node says it is alive.
pub(crate) const HEARTBEAT: Duration = Duration::from_secs(5);

/// Every this many heartbeats a node resends all its presence members.
pub(crate) const RESYNC_EVERY: u32 = 6;

/// A node not heard from for this long is forgotten with its members.
pub(crate) const NODE_TIMEOUT: Duration = Duration::from_secs(20);

/// The version of the message format.
const VERSION: u8 = 1;

/// One message on the backplane: who sent it and what.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Envelope {
    /// The format version.
    pub(crate) v: u8,
    /// The sending node.
    pub(crate) node: String,
    /// The message.
    #[serde(flatten)]
    pub(crate) message: Message,
}

impl Envelope {
    pub(crate) fn new(node: &str, message: Message) -> Self {
        Self {
            v: VERSION,
            node: node.to_owned(),
            message,
        }
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// `None` for malformed messages and other versions.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Envelope> {
        serde_json::from_slice::<Envelope>(bytes)
            .ok()
            .filter(|envelope| envelope.v == VERSION)
    }
}

/// What nodes tell each other.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Message {
    /// Deliver `event` with `data` (JSON text) to the subscribers of `channels`.
    Broadcast {
        channels: Vec<String>,
        event: String,
        data: String,
        /// The sender's id for live streams (`<ms>-<seq>`), so a stream
        /// resumed on another process finds what it missed. Absent from
        /// senders that predate it: the receiver gives one of its own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Close the user's connections with 4009.
    DisconnectUser { user_id: String },
    /// Close the user's connections with 4200 (reconnect and authorize again).
    EndUserSubscriptions { user_id: String },
    /// The user's first connection on the sending node joined `channel`.
    MemberAdded {
        channel: String,
        user_id: String,
        user_info: Value,
    },
    /// The user's last connection on the sending node left `channel`.
    MemberRemoved { channel: String, user_id: String },
    /// A node started: every node answers with a `state`.
    Hello,
    /// Every presence member of the sending node (replaces what it sent before).
    State { members: Vec<MemberEntry> },
    /// The sending node is alive.
    Heartbeat,
    /// The sending node shuts down: forget it and its members.
    Goodbye,
}

/// A presence member of one channel.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct MemberEntry {
    pub(crate) channel: String,
    pub(crate) user_id: String,
    pub(crate) user_info: Value,
}

/// A change in who other nodes report as present.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Change {
    /// The user is now present on some other node (and was on none).
    Added {
        channel: String,
        user_id: String,
        user_info: Value,
    },
    /// The user is no longer present on any other node.
    Removed { channel: String, user_id: String },
}

/// A user other nodes report on a channel.
#[derive(Clone, Debug)]
struct RemoteMember {
    user_info: Value,
    nodes: HashSet<String>,
}

/// The presence members other nodes report, and when each node was last
/// heard from.
#[derive(Debug, Default)]
pub(crate) struct RemotePresence {
    /// channel → user id → member.
    channels: HashMap<String, HashMap<String, RemoteMember>>,
    /// node → last heard from.
    nodes: HashMap<String, Instant>,
}

impl RemotePresence {
    /// Records that `node` was heard from at `now`.
    pub(crate) fn seen(&mut self, node: &str, now: Instant) {
        self.nodes.insert(node.to_owned(), now);
    }

    /// Whether another node reports `user_id` on `channel`.
    pub(crate) fn has(&self, channel: &str, user_id: &str) -> bool {
        self.channels
            .get(channel)
            .is_some_and(|members| members.contains_key(user_id))
    }

    /// The members other nodes report on `channel`, with their `user_info`.
    pub(crate) fn members(&self, channel: &str) -> Vec<(String, Value)> {
        self.channels
            .get(channel)
            .map(|members| {
                members
                    .iter()
                    .map(|(user_id, member)| (user_id.clone(), member.user_info.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `node` reports `user_id` on `channel`. Returns the change, if the
    /// user was on no other node before.
    pub(crate) fn add(
        &mut self,
        node: &str,
        channel: &str,
        user_id: &str,
        user_info: Value,
    ) -> Option<Change> {
        let members = self.channels.entry(channel.to_owned()).or_default();
        match members.get_mut(user_id) {
            Some(member) => {
                member.nodes.insert(node.to_owned());
                member.user_info = user_info;
                None
            }
            None => {
                members.insert(
                    user_id.to_owned(),
                    RemoteMember {
                        user_info: user_info.clone(),
                        nodes: HashSet::from([node.to_owned()]),
                    },
                );
                Some(Change::Added {
                    channel: channel.to_owned(),
                    user_id: user_id.to_owned(),
                    user_info,
                })
            }
        }
    }

    /// `node` no longer reports `user_id` on `channel`. Returns the change,
    /// if no other node reports the user any more.
    pub(crate) fn remove(&mut self, node: &str, channel: &str, user_id: &str) -> Option<Change> {
        let members = self.channels.get_mut(channel)?;
        let member = members.get_mut(user_id)?;
        if !member.nodes.remove(node) || !member.nodes.is_empty() {
            return None;
        }
        members.remove(user_id);
        if members.is_empty() {
            self.channels.remove(channel);
        }
        Some(Change::Removed {
            channel: channel.to_owned(),
            user_id: user_id.to_owned(),
        })
    }

    /// Replaces everything `node` reported with `members`; returns the
    /// changes.
    pub(crate) fn replace(&mut self, node: &str, members: Vec<MemberEntry>) -> Vec<Change> {
        let wanted: HashSet<(String, String)> = members
            .iter()
            .map(|m| (m.channel.clone(), m.user_id.clone()))
            .collect();
        let stale: Vec<(String, String)> = self
            .channels
            .iter()
            .flat_map(|(channel, users)| {
                users
                    .iter()
                    .filter(|(_, member)| member.nodes.contains(node))
                    .map(|(user_id, _)| (channel.clone(), user_id.clone()))
            })
            .filter(|key| !wanted.contains(key))
            .collect();
        let mut changes = Vec::new();
        for (channel, user_id) in stale {
            changes.extend(self.remove(node, &channel, &user_id));
        }
        for member in members {
            changes.extend(self.add(node, &member.channel, &member.user_id, member.user_info));
        }
        changes
    }

    /// Forgets `node` and its members; returns the changes.
    pub(crate) fn forget(&mut self, node: &str) -> Vec<Change> {
        self.nodes.remove(node);
        self.replace(node, Vec::new())
    }

    /// Forgets every node not heard from since `now - timeout`; returns
    /// the changes.
    pub(crate) fn expire(&mut self, now: Instant, timeout: Duration) -> Vec<Change> {
        let silent: Vec<String> = self
            .nodes
            .iter()
            .filter(|(_, seen)| now.saturating_duration_since(**seen) >= timeout)
            .map(|(node, _)| node.clone())
            .collect();
        silent.iter().flat_map(|node| self.forget(node)).collect()
    }

    /// The number of other nodes heard from.
    #[cfg(test)]
    pub(crate) fn nodes(&self) -> usize {
        self.nodes.len()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn entry(channel: &str, user_id: &str) -> MemberEntry {
        MemberEntry {
            channel: channel.to_owned(),
            user_id: user_id.to_owned(),
            user_info: json!({ "name": user_id }),
        }
    }

    fn added(channel: &str, user_id: &str) -> Change {
        Change::Added {
            channel: channel.to_owned(),
            user_id: user_id.to_owned(),
            user_info: json!({ "name": user_id }),
        }
    }

    fn removed(channel: &str, user_id: &str) -> Change {
        Change::Removed {
            channel: channel.to_owned(),
            user_id: user_id.to_owned(),
        }
    }

    #[test]
    fn envelopes_round_trip_and_reject_other_versions() {
        let envelope = Envelope::new(
            "n1",
            Message::Broadcast {
                channels: vec!["news".to_owned()],
                event: "posted".to_owned(),
                data: "{\"id\":1}".to_owned(),
                id: Some("1730000000000-7".to_owned()),
            },
        );
        let bytes = envelope.encode();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(
            text.contains("\"type\":\"broadcast\"") && text.contains("\"node\":\"n1\""),
            "{text}"
        );
        assert_eq!(Envelope::decode(&bytes), Some(envelope));
        assert_eq!(
            Envelope::decode(b"{\"v\":2,\"node\":\"n\",\"type\":\"hello\"}"),
            None
        );
        assert_eq!(Envelope::decode(b"not json"), None);
        // A broadcast without an id (an older sender) still reads.
        let without =
            br#"{"v":1,"node":"n","type":"broadcast","channels":["a"],"event":"e","data":"{}"}"#;
        assert!(matches!(
            Envelope::decode(without).map(|e| e.message),
            Some(Message::Broadcast { id: None, .. })
        ));
        assert_eq!(
            Envelope::decode(b"{\"v\":1,\"node\":\"n\",\"type\":\"goodbye\"}").map(|e| e.message),
            Some(Message::Goodbye)
        );
    }

    #[test]
    fn a_user_is_added_once_and_removed_with_its_last_node() {
        let mut presence = RemotePresence::default();
        assert_eq!(
            presence.add("n1", "presence-room", "anu", json!({ "name": "anu" })),
            Some(added("presence-room", "anu"))
        );
        assert_eq!(
            presence.add("n2", "presence-room", "anu", json!({ "name": "anu" })),
            None
        );
        assert!(presence.has("presence-room", "anu"));
        assert_eq!(presence.remove("n1", "presence-room", "anu"), None);
        assert_eq!(presence.remove("n1", "presence-room", "anu"), None, "twice");
        assert_eq!(
            presence.remove("n2", "presence-room", "anu"),
            Some(removed("presence-room", "anu"))
        );
        assert!(!presence.has("presence-room", "anu"));
        assert!(presence.members("presence-room").is_empty());
        assert_eq!(presence.remove("n9", "nowhere", "anu"), None);
    }

    #[test]
    fn a_snapshot_replaces_what_the_node_reported() {
        let mut presence = RemotePresence::default();
        presence.add("n1", "a", "anu", json!({ "name": "anu" }));
        presence.add("n1", "a", "bob", json!({ "name": "bob" }));
        presence.add("n2", "a", "bob", json!({ "name": "bob" }));
        let mut changes = presence.replace("n1", vec![entry("a", "anu"), entry("b", "cy")]);
        changes.sort_by_key(|change| format!("{change:?}"));
        assert_eq!(changes, vec![added("b", "cy")], "bob stays through n2");
        let changes = presence.replace("n2", Vec::new());
        assert_eq!(changes, vec![removed("a", "bob")]);
        let mut members = presence.members("a");
        members.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(members, vec![("anu".to_owned(), json!({ "name": "anu" }))]);
    }

    #[test]
    fn silent_nodes_expire_with_their_members() {
        let mut presence = RemotePresence::default();
        let start = Instant::now();
        presence.seen("n1", start);
        presence.seen("n2", start);
        presence.add("n1", "a", "anu", json!({ "name": "anu" }));
        presence.add("n2", "a", "bob", json!({ "name": "bob" }));
        presence.seen("n2", start + Duration::from_secs(15));

        let now = start + NODE_TIMEOUT;
        assert_eq!(
            presence.expire(now, NODE_TIMEOUT),
            vec![removed("a", "anu")]
        );
        assert_eq!(presence.nodes(), 1);
        assert!(presence.has("a", "bob"));
        assert_eq!(presence.forget("n2"), vec![removed("a", "bob")]);
        assert_eq!(presence.nodes(), 0);
    }
}
