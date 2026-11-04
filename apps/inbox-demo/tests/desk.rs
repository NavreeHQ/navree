//! The desk on a real SQLite file, as the app runs it: pages walked by cursor across tied
//! times, writers that overlap while a reader follows them live, filters that match
//! little, a snoozed conversation coming back on its day, and the agent's signed cookie.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use inbox_demo::data::{Conversation, Message, Status};
use inbox_demo::live::{Happening, Hub};
use inbox_demo::pages::{agent_cookie, agent_of_cookie, explain};
use inbox_demo::queries::{self, Since};
use inbox_demo::views::{Filters, View};
use navree::db::{Db, DbOptions, ExecutorExt as _, SqliteOptions, Statement, Value};
use navree::http::UrlSigner;
use navree::orm::Model as _;
use navree::ui::LazyAsk;

/// 4 November 2026, 12:00 in London: the moment every desk here is written at.
const NOW: i64 = 1_793_793_600;

/// A database file of its own, taken away when the test ends.
struct Desk {
    db: Db,
    hub: Arc<Hub>,
    path: PathBuf,
}

impl Drop for Desk {
    fn drop(&mut self) {
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", self.path.display()));
        }
    }
}

/// A desk written afresh at `NOW` with `scale` generated conversations.
async fn desk(name: &str, scale: i64) -> Desk {
    let path = std::env::temp_dir().join(format!("inbox-demo-{name}-{}.db", std::process::id()));
    for end in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{end}", path.display()));
    }
    let db = Db::connect(&DbOptions {
        url: format!("sqlite://{}?mode=rwc", path.display()),
        max_connections: 8,
        // SQLite's wait for its lock keeps no queue: six writers in tight loops on an
        // unoptimised test build can keep one waiting past the default five seconds.
        sqlite: SqliteOptions {
            busy_timeout_ms: 60_000,
            ..SqliteOptions::default()
        },
        ..DbOptions::default()
    })
    .await
    .expect("a database file");
    let hub = Arc::new(Hub::new());
    hub.seed(&db, NOW, scale).await.expect("the seed");
    Desk { db, hub, path }
}

fn rows(records: navree::orm::Collection<Conversation>) -> Vec<Conversation> {
    records
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect()
}

/// Where a conversation sorts: its last activity, the revision it happened at, its id.
fn key(c: &Conversation) -> (i64, i64, i64) {
    (c.last_at, c.last_rev, c.id)
}

/// Every conversation of `filters`' view, newest first, as the database orders them.
async fn every(db: &Db, filters: &Filters) -> Vec<(i64, i64, i64)> {
    rows(
        queries::newest_first(queries::in_view(filters, 1))
            .get(db)
            .await
            .expect("the view"),
    )
    .iter()
    .map(key)
    .collect()
}

/// The view walked page by page by the cursor of each page's last row, as the runtime
/// asks for them.
async fn walk(db: &Db, filters: &Filters) -> Vec<(i64, i64, i64)> {
    let mut seen = Vec::new();
    let mut ask = LazyAsk::First;
    loop {
        let (page, more, _) = queries::list_page(db, filters, 1, &ask)
            .await
            .expect("a page");
        seen.extend(page.iter().map(|row| key(&row.conversation)));
        match (more, page.last()) {
            (true, Some(last)) => ask = LazyAsk::After(last.conversation.cursor()),
            _ => return seen,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pages_hold_every_conversation_once_when_times_tie_at_a_page_edge() {
    let desk = desk("ties", 400).await;
    // A burst in one second, as the simulator makes: forty customers write at once, so
    // the top of the open list is one time shared by many rows, across a page's edge.
    let open: Vec<Conversation> = rows(
        Conversation::query()
            .where_(Conversation::STATUS.is_in(["open", "pending"]))
            .order_by("id")
            .limit(60)
            .get(&desk.db)
            .await
            .expect("open conversations"),
    );
    assert!(open.len() >= 55, "{}", open.len());
    for conversation in &open {
        desk.hub
            .step(
                &desk.db,
                NOW,
                Some(Happening::CustomerReply),
                Some(conversation.id),
            )
            .await
            .expect("a customer writes");
    }
    for view in View::ALL {
        let filters = Filters::with_view(view);
        let want = every(&desk.db, &filters).await;
        let got = walk(&desk.db, &filters).await;
        assert_eq!(got, want, "{view:?}: every row once, in order");
        // Strictly newest first: (last_at, last_rev, id) never repeats or goes back.
        assert!(got.windows(2).all(|pair| pair[0] > pair[1]), "{view:?}");
    }
    let open = every(&desk.db, &Filters::default()).await;
    let mut by_time: BTreeMap<i64, Vec<(i64, i64)>> = BTreeMap::new();
    for (at, rev, id) in &open {
        by_time.entry(*at).or_default().push((*rev, *id));
    }
    let ties: Vec<(i64, Vec<(i64, i64)>)> = by_time
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .collect();
    assert!(
        ties.iter().any(|(at, ids)| *at == NOW && ids.len() >= 55),
        "the burst shares one time across the first page's edge: {ties:?}"
    );
    // A page that ends inside a tie goes on with the next row of the same time, from
    // either way.
    for (at, tied) in &ties {
        let mut desc = tied.clone();
        desc.sort_unstable_by(|a, b| b.cmp(a));
        let cursor = |(rev, id): (i64, i64)| {
            Conversation {
                last_at: *at,
                last_rev: rev,
                id,
                ..Conversation::default()
            }
            .cursor()
        };
        for pair in desc.windows(2) {
            let (after, _, _) = queries::list_page(
                &desk.db,
                &Filters::default(),
                1,
                &LazyAsk::After(cursor(pair[0])),
            )
            .await
            .expect("the page after");
            assert_eq!(
                after.first().map(|row| row.conversation.id),
                Some(pair[1].1),
                "after ({at}, {:?})",
                pair[0]
            );
            let (before, _, _) = queries::list_page(
                &desk.db,
                &Filters::default(),
                1,
                &LazyAsk::Before(cursor(pair[1])),
            )
            .await
            .expect("the page before");
            assert_eq!(
                before.last().map(|row| row.conversation.id),
                Some(pair[0].1),
                "before ({at}, {:?})",
                pair[1]
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writers_that_overlap_never_hide_a_message_a_change_or_a_count_from_a_reader() {
    let desk = desk("race", 0).await;
    let open: Vec<Conversation> = rows(
        Conversation::query()
            .where_(Conversation::STATUS.eq(Status::Open.key()))
            .where_(Conversation::ASSIGNEE_ID.is_not_null())
            .order_by("id")
            .limit(6)
            .get(&desk.db)
            .await
            .expect("open conversations"),
    );
    assert!(open.len() >= 4);
    let watched = open[0].clone();
    let others: Vec<i64> = open[1..].iter().map(|c| c.id).collect();
    let start = rows(
        Conversation::query()
            .where_(Conversation::ID.eq(watched.id))
            .get(&desk.db)
            .await
            .expect("the watched conversation"),
    )[0]
    .clone();
    let first_message = Message::query()
        .where_(Message::CONVERSATION_ID.eq(watched.id))
        .order_by_desc("id")
        .first(&desk.db)
        .await
        .expect("its messages")
        .map_or(0, |record| record.id);
    let since = Since {
        rev: inbox_demo::live::current_rev(&desk.db)
            .await
            .expect("the revision"),
        at: NOW,
    };

    // The reader follows the runtime: the thread asks for what came after the newest
    // message it has, the list for what changed since the revision it has.
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let (db, done) = (desk.db.clone(), done.clone());
        let id = watched.id;
        tokio::spawn(async move {
            let mut thread_since = first_message.to_string();
            let mut list_since = since;
            let mut messages = BTreeSet::new();
            let mut revisions: BTreeMap<i64, i64> = BTreeMap::new();
            let mut last_round = false;
            loop {
                let finished = done.load(std::sync::atomic::Ordering::SeqCst);
                loop {
                    let (found, _, _, more) =
                        queries::thread_window(&db, id, &LazyAsk::Since(thread_since.clone()))
                            .await
                            .expect("the thread's news");
                    if let Some(last) = found.last() {
                        thread_since = last.id.to_string();
                    }
                    messages.extend(found.iter().map(|m| m.id));
                    if !more {
                        break;
                    }
                }
                loop {
                    let (changed, _, newest, more) =
                        queries::changes(&db, &Filters::default(), 1, list_since, None, NOW)
                            .await
                            .expect("the list's news");
                    for row in changed {
                        revisions.insert(row.conversation.id, row.conversation.rev);
                    }
                    list_since = Since {
                        rev: newest,
                        at: NOW,
                    };
                    if !more {
                        break;
                    }
                }
                if last_round {
                    return (messages, revisions);
                }
                last_round = finished;
                tokio::task::yield_now().await;
            }
        })
    };

    // Writers, all at once: customers writing in the watched conversation and others,
    // and the team's notes in it, each in its own unit of work.
    let mut writers = Vec::new();
    for writer in 0..6_usize {
        let (db, hub) = (desk.db.clone(), desk.hub.clone());
        let others = others.clone();
        let id = watched.id;
        writers.push(tokio::spawn(async move {
            let (mut customer, mut here) = (0_i64, 0_i64);
            for round in 0..40_usize {
                let (what, on) = match (writer + round) % 3 {
                    0 => (Happening::CustomerReply, id),
                    1 => (Happening::TeammateNote, id),
                    _ => (
                        Happening::CustomerReply,
                        others[(writer + round) % others.len()],
                    ),
                };
                let change = hub
                    .step(
                        &db,
                        NOW + i64::try_from(round).unwrap_or(0),
                        Some(what),
                        Some(on),
                    )
                    .await
                    .expect("a write");
                assert!(change.is_some(), "{what:?} on {on}");
                if on == id {
                    here += 1;
                    if what == Happening::CustomerReply {
                        customer += 1;
                    }
                }
            }
            (customer, here)
        }));
    }
    let (mut customer_messages, mut written_here) = (0, 0);
    for writer in writers {
        let (customer, here) = writer.await.expect("a writer");
        customer_messages += customer;
        written_here += here;
    }
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let (seen_messages, seen_revisions) = reader.await.expect("the reader");

    let written: BTreeSet<i64> = Message::query()
        .where_(Message::CONVERSATION_ID.eq(watched.id))
        .where_(Message::ID.gt(first_message))
        .get(&desk.db)
        .await
        .expect("the messages written")
        .into_vec()
        .into_iter()
        .map(|record| record.id)
        .collect();
    assert_eq!(written.len() as i64, written_here, "every write landed");
    let missed: Vec<&i64> = written.difference(&seen_messages).collect();
    assert!(missed.is_empty(), "the reader missed messages: {missed:?}");

    // The list's reader holds each conversation as it is now.
    let now: Vec<Conversation> = rows(
        Conversation::query()
            .where_(Conversation::ID.is_in(open.iter().map(|c| c.id).collect::<Vec<_>>()))
            .get(&desk.db)
            .await
            .expect("the conversations"),
    );
    for conversation in &now {
        assert_eq!(
            seen_revisions.get(&conversation.id),
            Some(&conversation.rev),
            "conversation {}: the reader holds its last change",
            conversation.id
        );
    }

    // The kept counts are the real ones.
    let watched_now = now.iter().find(|c| c.id == watched.id).expect("watched");
    let real = desk
        .db
        .fetch_scalar::<i64>(&Statement::portable(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ?",
            [Value::I64(watched.id)],
        ))
        .await
        .expect("the count");
    assert_eq!(watched_now.messages, real, "the kept count of messages");
    assert_eq!(watched_now.messages, start.messages + written_here);
    assert_eq!(
        watched_now.unread,
        start.unread + customer_messages,
        "the kept count of what is new"
    );
}

#[tokio::test]
async fn a_filter_that_matches_little_reads_only_what_it_matches() {
    let desk = desk("filters", 2_000).await;
    let plans = explain(&desk.db).await.expect("the plans");
    let mut checked = 0;
    for section in plans
        .split("\n## ")
        .map(|part| part.trim_start_matches("## "))
    {
        let mut lines = section.lines();
        let name = lines.next().unwrap_or_default();
        let expect = lines
            .next()
            .and_then(|line| line.strip_prefix("expect:"))
            .unwrap_or_default()
            .trim();
        let plan: Vec<&str> = lines.skip(1).collect();
        let search = plan
            .iter()
            .find(|line| line.contains("SEARCH conversations") || line.contains("SEARCH messages"))
            .copied()
            .unwrap_or_default();
        // One of the indexes named, matched on every column it names.
        let matched = expect.is_empty()
            || expect.split('|').any(|columns| {
                columns
                    .split_whitespace()
                    .all(|column| search.contains(&format!("{column}=?")))
            });
        assert!(matched, "{name}: the index matches {expect}: {plan:?}");
        let walks = |line: &&str| {
            let line = line.trim();
            ["SCAN conversations", "SCAN messages"]
                .iter()
                .any(|scan| line == *scan || line.starts_with(&format!("{scan} ")))
        };
        assert!(!plan.iter().any(walks), "{name} walks the table: {plan:?}");
        checked += 1;
    }
    assert!(checked >= 20, "{checked} plans");
    // And the answer holds only what matches, in order.
    let filters = Filters::read(Some("closed"), None, Some("api"), Some("urgent"));
    let want = every(&desk.db, &filters).await;
    assert_eq!(walk(&desk.db, &filters).await, want);
}

#[tokio::test]
async fn of_two_conversations_written_to_in_one_second_the_later_comes_first() {
    let desk = desk("second", 0).await;
    let open = every(&desk.db, &Filters::default()).await;
    let (low, high) = (
        open.iter()
            .map(|(_, _, id)| *id)
            .min()
            .expect("an open conversation"),
        open.iter()
            .map(|(_, _, id)| *id)
            .max()
            .expect("an open conversation"),
    );
    assert!(low < high);
    // A customer writes in the newer conversation, then, in the same second, one writes
    // in the older: the older is where the latest word is, so it goes to the top.
    for id in [high, low] {
        desk.hub
            .step(&desk.db, NOW + 60, Some(Happening::CustomerReply), Some(id))
            .await
            .expect("a customer writes");
    }
    let (page, _, _) = queries::list_page(&desk.db, &Filters::default(), 1, &LazyAsk::First)
        .await
        .expect("the first page");
    let top: Vec<i64> = page.iter().take(2).map(|row| row.conversation.id).collect();
    assert_eq!(top, [low, high], "the one written to last is on top");
    // A live list puts what changed by its cursor, as text: the same order.
    assert!(page[0].conversation.cursor() > page[1].conversation.cursor());
    let (changed, _, _, _) = queries::changes(
        &desk.db,
        &Filters::default(),
        1,
        Since { rev: 1, at: NOW },
        None,
        NOW + 60,
    )
    .await
    .expect("what changed");
    let mut cursors: Vec<(String, i64)> = changed
        .iter()
        .map(|row| (row.conversation.cursor(), row.conversation.id))
        .collect();
    cursors.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    assert_eq!(
        cursors.iter().map(|(_, id)| *id).collect::<Vec<i64>>(),
        [low, high]
    );
}

#[tokio::test]
async fn a_snoozed_conversation_comes_back_on_its_day() {
    let desk = desk("snooze", 0).await;
    let snoozed: Vec<Conversation> = rows(
        Conversation::query()
            .where_(Conversation::INBOX.eq("snoozed"))
            .get(&desk.db)
            .await
            .expect("snoozed conversations"),
    );
    assert!(!snoozed.is_empty());
    let day = snoozed
        .iter()
        .filter_map(|c| c.snoozed_until)
        .min()
        .expect("a day");
    let at = |date: chrono::NaiveDate| {
        Utc.from_utc_datetime(&date.and_hms_opt(6, 0, 0).unwrap_or_default())
            .timestamp()
    };
    // The day before: nothing comes back.
    assert_eq!(
        desk.hub
            .wake(&desk.db, at(day - Duration::days(1)))
            .await
            .expect("wake"),
        0
    );
    // On the day it does, open, its clock started, with a line that says so.
    let woken = desk.hub.wake(&desk.db, at(day)).await.expect("wake");
    assert!(woken >= 1);
    let back = Conversation::query()
        .find(&desk.db, snoozed[0].id)
        .await
        .expect("it")
        .expect("there")
        .into_inner();
    if snoozed[0].snoozed_until == Some(day) {
        assert_eq!(back.status, "open");
        assert_eq!(back.inbox, "open");
        assert_eq!(back.snoozed_until, None);
        assert!(back.sla_due.is_some());
        assert_eq!(back.messages, snoozed[0].messages + 1);
        let last = Message::query()
            .where_(Message::CONVERSATION_ID.eq(back.id))
            .order_by_desc("id")
            .first(&desk.db)
            .await
            .expect("its messages")
            .expect("a line");
        assert!(last.body.starts_with("Back from snooze"), "{}", last.body);
    }
    // Twice is once.
    assert_eq!(desk.hub.wake(&desk.db, at(day)).await.expect("wake"), 0);
}

#[test]
fn the_agent_cookie_is_signed_and_a_forged_one_names_nobody() {
    let signer = UrlSigner::new(b"a long random key for the tests");
    let now = Utc::now();
    let value = agent_cookie(&signer, 7, now);
    assert!(value.starts_with("7."), "{value}");
    assert!(
        value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.'),
        "a cookie value: {value}"
    );
    assert_eq!(agent_of_cookie(&signer, &value, now), Some(7));
    // Another agent's number on the same signature, another key, an old cookie, and
    // what an older page set (the bare number): nobody.
    let forged = value.replacen('7', "8", 1);
    assert_eq!(agent_of_cookie(&signer, &forged, now), None);
    assert_eq!(
        agent_of_cookie(&UrlSigner::new(b"another key"), &value, now),
        None
    );
    assert_eq!(
        agent_of_cookie(&signer, &value, now + Duration::days(366)),
        None
    );
    for bad in [
        "7",
        "",
        "7..",
        "7.1.",
        "x.1.ab",
        "7.1.zz",
        "-7.1.ab",
        "7.99999999999999999999.ab",
    ] {
        assert_eq!(agent_of_cookie(&signer, bad, now), None, "{bad}");
    }
}

#[tokio::test]
async fn cursors_page_a_long_conversation_and_answer_a_bad_one_with_the_first_page() {
    let desk = desk("cursors", 20).await;
    // The longest conversation of the generated ones.
    let long = rows(
        Conversation::query()
            .where_(Conversation::ID.gte(inbox_demo::scale::FIRST_CONVERSATION))
            .order_by_desc("messages")
            .limit(1)
            .get(&desk.db)
            .await
            .expect("a conversation"),
    )[0]
    .clone();
    assert!(long.messages > 60, "{}", long.messages);
    let (newest, _, older, newer) = queries::thread_window(&desk.db, long.id, &LazyAsk::First)
        .await
        .expect("the newest");
    assert_eq!(newest.len(), 30);
    assert!(older && !newer);
    // Back to the first message, thirty at a time, each page before the last.
    let mut all = newest.clone();
    let mut first = newest[0].id;
    loop {
        let (page, _, more, newer) = queries::thread_window(
            &desk.db,
            long.id,
            &LazyAsk::Before(inbox_demo::data::message_cursor(first)),
        )
        .await
        .expect("a page");
        assert!(newer);
        all.splice(0..0, page.iter().cloned());
        match page.first() {
            Some(m) if more => first = m.id,
            _ => break,
        }
    }
    assert_eq!(all.len() as i64, long.messages);
    assert!(all.windows(2).all(|pair| pair[0].id < pair[1].id));
    // Forward again from the first.
    let (after, _, _, newer) = queries::thread_window(
        &desk.db,
        long.id,
        &LazyAsk::After(inbox_demo::data::message_cursor(all[0].id)),
    )
    .await
    .expect("after the first");
    assert_eq!(after.first().map(|m| m.id), Some(all[1].id));
    assert!(newer);
    // A live ask from the newest has nothing; from a bad cursor, nothing either.
    let (live, _, _, more) = queries::thread_window(
        &desk.db,
        long.id,
        &LazyAsk::Since(all[all.len() - 1].id.to_string()),
    )
    .await
    .expect("live");
    assert!(live.is_empty() && !more);
    let (live, _, _, _) =
        queries::thread_window(&desk.db, long.id, &LazyAsk::Since("x' OR 1=1".to_owned()))
            .await
            .expect("live");
    assert!(live.is_empty());
    // A cursor that doesn't read is the first page, for the list and the thread.
    let (first_page, _, _) = queries::list_page(&desk.db, &Filters::default(), 1, &LazyAsk::First)
        .await
        .expect("the first page");
    for bad in ["' OR 1=1--", "abc", &"9".repeat(27), ""] {
        let (page, _, _) = queries::list_page(
            &desk.db,
            &Filters::default(),
            1,
            &LazyAsk::After(bad.to_owned()),
        )
        .await
        .expect("a page");
        assert_eq!(
            page.iter().map(|r| r.conversation.id).collect::<Vec<_>>(),
            first_page
                .iter()
                .map(|r| r.conversation.id)
                .collect::<Vec<_>>(),
            "{bad}"
        );
        let (thread, _, _, _) =
            queries::thread_window(&desk.db, long.id, &LazyAsk::Before(bad.to_owned()))
                .await
                .expect("a page");
        assert_eq!(
            thread.first().map(|m| m.id),
            newest.first().map(|m| m.id),
            "{bad}"
        );
    }
}
