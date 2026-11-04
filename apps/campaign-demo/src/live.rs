//! The campaign as it happens: a simulator that makes readers open, click,
//! convert and leave while the app runs, and the server-sent event stream that
//! tells every open page.
//!
//! An event on the stream is only a nudge. The page never reads what it says: each
//! live part of a page fetches its own fragment again, so the stream can't put
//! anything in a page that the server didn't draw.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use http::HeaderMap;
use http::header::CONTENT_TYPE;
use navree::db::{Db, Executor as _};
use navree::http::response::sse::{Event as Sse, KeepAlive, Sse as SseResponse};
use navree::http::response::{IntoResponse, Response};
use navree::orm::Model as _;
use navree::prelude::BoxError;
use tokio::sync::broadcast;

use crate::data::{self, Dice, Event, Kind, LINKS, Recipient, Status, day_of};

/// What happens next, and who hears about it.
pub struct Hub {
    tx: broadcast::Sender<()>,
    dice: Mutex<Dice>,
    next_event: AtomicI64,
    /// When the app seeded the campaign: "now" for the seed.
    seeded_at: AtomicI64,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(64).0,
            dice: Mutex::new(Dice::new(2_026)),
            next_event: AtomicI64::new(1),
            seeded_at: AtomicI64::new(0),
        }
    }

    /// When the campaign was sent.
    pub fn sent_at(&self) -> i64 {
        self.seeded_at.load(Ordering::SeqCst) - data::SENT_AGO
    }

    /// Tells every open stream that the campaign moved.
    pub fn nudge(&self) {
        let _ = self.tx.send(());
    }

    fn roll(&self, n: u64) -> u64 {
        self.dice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .below(n)
    }

    /// Creates the tables, empty.
    pub async fn create(&self, db: &Db) -> Result<(), BoxError> {
        let pool = db.pool();
        for statement in [
            "DROP TABLE IF EXISTS events",
            "DROP TABLE IF EXISTS recipients",
            "CREATE TABLE recipients (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL, role TEXT NOT NULL, company TEXT NOT NULL, country TEXT NOT NULL, city TEXT NOT NULL, device TEXT NOT NULL, os TEXT NOT NULL, client TEXT NOT NULL, network TEXT NOT NULL, ip TEXT NOT NULL, source TEXT NOT NULL, phone TEXT NOT NULL, customer_since DATE, status TEXT NOT NULL, opens INTEGER NOT NULL, clicks INTEGER NOT NULL, last_activity DATE)",
            "CREATE INDEX recipients_status ON recipients (status, id)",
            "CREATE INDEX recipients_last_activity ON recipients (last_activity, id)",
            "CREATE INDEX recipients_name ON recipients (name, id)",
            "CREATE TABLE events (id INTEGER PRIMARY KEY, recipient_id INTEGER NOT NULL, kind TEXT NOT NULL, at INTEGER NOT NULL, device TEXT NOT NULL, os TEXT NOT NULL, client TEXT NOT NULL, country TEXT NOT NULL, city TEXT NOT NULL, ip TEXT NOT NULL, link TEXT NOT NULL, detail TEXT NOT NULL)",
            "CREATE INDEX events_recipient ON events (recipient_id, at)",
            "CREATE INDEX events_kind ON events (kind, id)",
        ] {
            pool.execute_script(statement).await?;
        }
        Ok(())
    }

    /// Writes the campaign afresh: the recipients and their events, the campaign
    /// sent `SENT_AGO` before `now`.
    pub async fn seed(&self, db: &Db, now: i64) -> Result<(), BoxError> {
        Event::query().delete(db).await?;
        Recipient::query().delete(db).await?;
        let (recipients, events) = data::seed(now);
        for chunk in recipients.chunks(100) {
            Recipient::query().insert(db, chunk.to_vec()).await?;
        }
        for chunk in events.chunks(200) {
            Event::query().insert(db, chunk.to_vec()).await?;
        }
        self.next_event.store(
            i64::try_from(events.len()).unwrap_or(0) + 1,
            Ordering::SeqCst,
        );
        self.seeded_at.store(now, Ordering::SeqCst);
        Ok(())
    }

    /// One reader does one thing: `kind` when given, otherwise whatever the dice
    /// say. Returns what happened, or `None` when nobody was left to do it.
    pub async fn step(
        &self,
        db: &Db,
        now: i64,
        kind: Option<Kind>,
    ) -> Result<Option<Event>, BoxError> {
        let kind = kind.unwrap_or_else(|| match self.roll(100) {
            0..=44 => Kind::Opened,
            45..=59 => Kind::ProxyOpen,
            60..=84 => Kind::Clicked,
            85..=95 => Kind::Converted,
            _ => Kind::Unsubscribed,
        });
        // Who can do it: an open needs a delivered copy, a click an open, and so on.
        let from = match kind {
            Kind::Opened | Kind::ProxyOpen => Status::Delivered,
            Kind::Clicked | Kind::Unsubscribed => Status::Opened,
            Kind::Converted => Status::Clicked,
            _ => return Ok(None),
        };
        let Some(mut person) = self.pick(db, from).await? else {
            return Ok(None);
        };
        let link = LINKS[usize::try_from(self.roll(3)).unwrap_or(0)];
        let (detail, url) = match kind {
            Kind::ProxyOpen => ("Images fetched by a mail privacy proxy", ""),
            Kind::Clicked => link,
            Kind::Converted => ("Started a 14-day Pro trial", ""),
            Kind::Unsubscribed => ("From the link in the footer", ""),
            _ => ("", ""),
        };
        let reader = kind.by_reader();
        let event = Event {
            id: self.next_event.fetch_add(1, Ordering::SeqCst),
            recipient_id: person.id,
            kind: kind.key().to_owned(),
            at: now,
            device: if reader {
                person.device.clone()
            } else {
                String::new()
            },
            os: if reader {
                person.os.clone()
            } else {
                String::new()
            },
            client: if reader {
                person.client.clone()
            } else {
                String::new()
            },
            country: if reader {
                person.country.clone()
            } else {
                String::new()
            },
            city: if reader {
                person.city.clone()
            } else {
                String::new()
            },
            ip: if reader {
                person.ip.clone()
            } else {
                String::new()
            },
            link: url.to_owned(),
            detail: detail.to_owned(),
        };
        Event::query().insert(db, [event.clone()]).await?;
        match kind {
            Kind::Opened => {
                person.opens += 1;
                person.status = Status::Opened.key().to_owned();
            }
            Kind::Clicked => {
                person.clicks += 1;
                person.status = Status::Clicked.key().to_owned();
            }
            Kind::Converted => person.status = Status::Converted.key().to_owned(),
            Kind::Unsubscribed => person.status = Status::Unsubscribed.key().to_owned(),
            _ => {}
        }
        if reader {
            person.last_activity = day_of(now);
        }
        Recipient::query()
            .where_(Recipient::ID.eq(person.id))
            .update(
                db,
                [
                    Recipient::STATUS.set(person.status.clone()),
                    Recipient::OPENS.set(person.opens),
                    Recipient::CLICKS.set(person.clicks),
                    Recipient::LAST_ACTIVITY.set(person.last_activity),
                ],
            )
            .await?;
        self.nudge();
        Ok(Some(event))
    }

    /// Someone whose copy is at `from`. A click, a conversion or an unsubscribe
    /// comes from someone who acted lately, as it does in a real campaign; an open
    /// from anyone it was delivered to.
    async fn pick(&self, db: &Db, from: Status) -> Result<Option<Recipient>, BoxError> {
        let before = match from {
            Status::Opened => Some(Kind::Opened),
            Status::Clicked => Some(Kind::Clicked),
            _ => None,
        };
        if let Some(kind) = before {
            let recent: Vec<i64> = Event::query()
                .where_(Event::KIND.eq(kind.key()))
                .order_by_desc("id")
                .limit(40)
                .get(db)
                .await?
                .into_vec()
                .into_iter()
                .map(|record| record.recipient_id)
                .collect();
            let ready: Vec<Recipient> = Recipient::query()
                .where_(Recipient::STATUS.eq(from.key()))
                .find_many(db, recent)
                .await?
                .into_vec()
                .into_iter()
                .map(|record| record.into_inner())
                .collect();
            if !ready.is_empty() {
                let at = usize::try_from(self.roll(ready.len() as u64)).unwrap_or(0);
                return Ok(ready.into_iter().nth(at));
            }
        }
        let candidates = Recipient::query().where_(Recipient::STATUS.eq(from.key()));
        let count = candidates.clone().count(db).await?;
        if count == 0 {
            return Ok(None);
        }
        Ok(candidates
            .order_by("id")
            .offset(self.roll(count))
            .first(db)
            .await?
            .map(|record| record.into_inner()))
    }

    /// Runs the simulator: something happens every `every`, give or take a
    /// little. It runs until the task holding it is dropped.
    pub async fn simulate(self: Arc<Self>, db: Arc<Db>, every: Duration) {
        loop {
            let jitter = Duration::from_millis(self.roll(900));
            tokio::time::sleep(every + jitter).await;
            if let Err(error) = self.step(&db, now(), None).await {
                eprintln!("campaign-demo: the simulator could not take a step: {error}");
            }
        }
    }

    /// `GET /events`: the stream. Each change says three things, one for each kind
    /// of live part: `activity` (the feed), `recipients` (the table) and `stats`
    /// (the figures, the chart and the funnel).
    pub fn stream(&self, headers: &HeaderMap) -> Response {
        // The runtime asks once what a failed stream answers: say it's a stream.
        if headers.contains_key("x-ui-probe") {
            return ([(CONTENT_TYPE, "text/event-stream")], "").into_response();
        }
        let rx = self.tx.subscribe();
        // A first comment goes out at once: Firefox reports a stream open only when
        // its first bytes arrive.
        let queue = VecDeque::from([Sse::default().comment("ready")]);
        let events = futures_util::stream::unfold((rx, queue), |(mut rx, mut queue)| async move {
            loop {
                if let Some(event) = queue.pop_front() {
                    return Some((Ok::<_, Infallible>(event), (rx, queue)));
                }
                match rx.recv().await {
                    Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        for name in ["activity", "recipients", "stats"] {
                            queue.push_back(Sse::default().event(name).data("changed"));
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        });
        SseResponse::new(events)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
            .into_response()
    }
}

/// Now, in unix seconds.
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
