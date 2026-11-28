//! A handler's future must be `Send`, and a `DataTable` holds the functions it is given per row
//! as closures that need not be: build it after the handler's last `.await`, and the future is
//! `Send` with any closures at all.

use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use navree_ui::{DataTable, Node, TableQuery, TableRow, render};

#[derive(Clone, TableRow)]
struct Deal {
    #[col(skip)]
    id: i64,
    #[col(sortable)]
    title: String,
}

/// What a handler awaits: the rows, from a store.
async fn load() -> Vec<Deal> {
    vec![Deal {
        id: 7,
        title: "Sutlej Supply renewal".to_owned(),
    }]
}

/// `future`, where a handler's future must be: `Send`.
fn handler<F: Future + Send>(future: F) -> F {
    future
}

/// Runs a future that is ready at once.
fn run<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(out) => out,
        Poll::Pending => panic!("the future waits"),
    }
}

#[test]
fn a_table_built_after_the_last_await_makes_a_send_future_whatever_its_closures_hold() {
    let html = run(handler(async {
        let rows = load().await;
        // Built after the last `.await`, so nothing of it is held across one: its closures may
        // hold what is not `Send`, such as an `Rc`.
        let prefix = Rc::new("/deals/".to_owned());
        let query = TableQuery::<Deal>::parse("/deals", "");
        let table = DataTable::new("deals", &query, &rows)
            .row_href(move |deal| format!("{prefix}{}", deal.id))
            .current_row(|deal| deal.id == 7);
        render(&Node::from(table))
    }));
    assert!(html.contains(r#"data-ui-row-href="/deals/7""#), "{html}");
    assert!(html.contains(r#"aria-current="true""#), "{html}");
}
