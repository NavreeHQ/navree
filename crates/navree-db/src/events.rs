//! Connection events ([`ConnectionEstablished`], [`DatabaseBusy`]) and the
//! `db:monitor` report ([`DatabaseStatus`]).

use navree_contracts::EventSink;
use navree_kernel::AppContext;
use serde::{Deserialize, Serialize};

/// A connection's pool opened: the default connection's at boot, a named
/// connection's on first use. Published as event
/// [`NAME`](ConnectionEstablished::NAME) through the app's `dyn EventSink`
/// (navree-events), when the app has one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionEstablished {
    /// The connection's name (`default`, or a named connection's).
    pub connection: String,
}

impl ConnectionEstablished {
    /// The event's name.
    pub const NAME: &'static str = "db.connection_established";
}

/// A database has more open connections than the limit `db:monitor` was
/// given. Published as event [`NAME`](DatabaseBusy::NAME) through the app's
/// `dyn EventSink`, when the app has one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseBusy {
    /// The connection's name.
    pub connection: String,
    /// The connections the server has open (from every client).
    pub open: u64,
}

impl DatabaseBusy {
    /// The event's name.
    pub const NAME: &'static str = "db.busy";
}

/// One database in a [`DbConnections::monitor`](crate::DbConnections::monitor)
/// report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseStatus {
    /// The connection's name.
    pub connection: String,
    /// The connections the server has open; `None` on SQLite, which has no
    /// server.
    pub open: Option<u64>,
    /// Whether `open` is above the limit.
    pub busy: bool,
}

/// Publishes `payload` as event `name` through the app's `dyn EventSink`,
/// when it has one; a failure is logged, never returned.
pub(crate) async fn publish<T: Serialize>(app: &AppContext, name: &str, payload: &T) {
    let Some(sink) = app.try_get::<dyn EventSink>().await else {
        return;
    };
    let payload = match serde_json::to_value(payload) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(target: "navree_db", %error, event = name, "the event could not be serialised");
            return;
        }
    };
    if let Err(error) = sink.publish(name, payload).await {
        tracing::warn!(target: "navree_db", %error, event = name, "the event could not be published");
    }
}
