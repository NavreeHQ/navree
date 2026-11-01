//! `ui-proof bench`: how long each page takes to render, and how big it is, in
//! this build. Run it with `--release`.

use std::hint::black_box;
use std::time::Instant;

use navree::ui::form::FormView;
use navree::ui::{Card, DataTable, Shell, TableQuery, View};

use crate::deal_form::NewDeal;
use crate::deals::{DealFilter, DealRow, rows, select};

/// The median time of `rounds` runs, and the size of what `work` returned.
fn time(name: &str, rounds: u32, work: impl Fn() -> String) {
    let mut times = Vec::with_capacity(rounds as usize);
    let mut bytes = 0;
    for _ in 0..rounds {
        let start = Instant::now();
        let html = black_box(work());
        times.push(start.elapsed());
        bytes = html.len();
    }
    times.sort();
    let median = times[times.len() / 2];
    let p99 = times[times.len() * 99 / 100];
    println!("{name:<28} median {:>9.1} us   p99 {:>9.1} us   {bytes:>8} bytes", median.as_secs_f64() * 1e6, p99.as_secs_f64() * 1e6);
}

pub fn run() {
    let shell: Shell = crate::shell();
    let deals = rows(1000);
    let query = TableQuery::<DealRow, DealFilter>::parse("/deals", "per=1000");
    let (selected, total) = select(&query, &deals);

    time("deal form page", 2000, || {
        shell.page("New deal", "/deals/new").body(Card::new().body(FormView::<NewDeal>::new().post("/deals").submit("Create deal"))).to_view().to_html()
    });
    time("1,000-row table, render", 300, || {
        View::new("Deals").body(DataTable::new("deals", &query, &selected).total(total)).to_html()
    });
    time("1,000-row page, with shell", 300, || {
        shell.page("Deals", "/deals").body(DataTable::new("deals", &query, &selected).total(total)).to_view().to_html()
    });
    time("1,000-row page, sort and all", 300, || {
        let sorted = TableQuery::<DealRow, DealFilter>::parse("/deals", "per=1000&sort=amount&dir=desc");
        let (found, count) = select(&sorted, &deals);
        shell.page("Deals", "/deals").body(DataTable::new("deals", &sorted, &found).total(count)).to_view().to_html()
    });
    time("plain page", 5000, || crate::dashboard::plain().to_html());
}
