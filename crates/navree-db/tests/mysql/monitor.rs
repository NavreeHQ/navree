//! `db:monitor` on MySQL: the server's open connections, and `db.busy`
//! for a database above the limit.

use std::sync::{Arc, Mutex};

use navree_db::{DatabaseBusy, DatabaseModule, DbConnections};
use navree_kernel::{BoxError, BoxFuture, Module, ModuleBuilder, Navree};

use crate::support::Scratch;

/// Records the events published through `dyn EventSink`.
#[derive(Default)]
struct Recorder(Mutex<Vec<(String, serde_json::Value)>>);

impl navree_contracts::EventSink for Recorder {
    fn publish(
        &self,
        name: &str,
        payload: serde_json::Value,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.lock().unwrap().push((name.to_owned(), payload));
        Box::pin(async { Ok(()) })
    }
}

struct Events(Arc<Recorder>);

impl Module for Events {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global();
        m.value_arc::<dyn navree_contracts::EventSink>(self.0.clone());
        m.exports::<dyn navree_contracts::EventSink>();
    }
}

#[tokio::test]
async fn the_monitor_counts_open_connections_and_reports_busy_databases() {
    let Some(scratch) =
        Scratch::new("the_monitor_counts_open_connections_and_reports_busy_databases").await
    else {
        return;
    };
    let recorder = Arc::new(Recorder::default());
    let app = Navree::app()
        .module(Events(recorder.clone()))
        .module(DatabaseModule::for_root(scratch.options()))
        .boot()
        .await
        .unwrap();
    let connections = app.ctx().get::<DbConnections>().await.unwrap();
    let statuses = connections.monitor(&[], Some(0)).await.unwrap();
    assert_eq!(statuses.len(), 1);
    let open = statuses[0].open.expect("MySQL counts its connections");
    assert!(open >= 1, "{statuses:?}");
    assert!(statuses[0].busy);
    let busy: Vec<serde_json::Value> = recorder
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == DatabaseBusy::NAME)
        .map(|(_, payload)| payload.clone())
        .collect();
    assert_eq!(busy.len(), 1);
    assert_eq!(busy[0]["connection"], "default");
    assert!(busy[0]["open"].as_u64().unwrap() >= 1);
    let calm = connections.monitor(&[], Some(1_000_000)).await.unwrap();
    assert!(!calm[0].busy);
}
