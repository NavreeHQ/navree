//! The desk at real scale: thousands of customers and up to 100,000 conversations of
//! 10 to 100 messages each, about 5.5 million messages at the top size.
//!
//! The rows are made inside SQLite by `INSERT ... SELECT` over small tables of words,
//! with arithmetic in place of chance, so the same desk comes out of every run and
//! nothing crosses between the app and the database row by row. Everything goes in one
//! transaction; the indexes are built after the rows, which is the quick way round.

use navree::db::{Db, Executor as _, ExecutorExt as _, Statement, Value};
use navree::prelude::BoxError;

use crate::data::{AGENTS, Plan, Priority, TOPICS};

/// How many conversations a desk has at full size.
pub const FULL: i64 = 100_000;
/// The first id of a generated conversation: the hand-written ones come first.
pub const FIRST_CONVERSATION: i64 = 1_001;
/// The first id of a generated customer.
pub const FIRST_CUSTOMER: i64 = 101;

/// What customers write, over and over, in a desk this size.
const CUSTOMER_LINES: [&str; 24] = [
    "Hi, a client says the booking page won't load on their phone. Can you check?",
    "We were charged for 12 seats but we only have 9 people. Can you fix the invoice?",
    "The staff app logs us out every morning. Is that expected?",
    "Two clients were booked into the same room at 10:00. How did that happen?",
    "Can we change the reminder text so it mentions parking?",
    "Our new manager can't see the reports tab.",
    "Is there a way to block bookings on public holidays automatically?",
    "The calendar shows last week's hours after we changed them.",
    "A client paid twice for the same class. Can you refund one?",
    "We'd like to add a second payment method for the studio.",
    "Some clients would rather read the booking page in Hindi. Can it follow their language?",
    "The waiting list didn't offer the free place to anyone.",
    "Our export is missing the phone numbers column.",
    "Can a therapist have different hours at each location?",
    "We moved to a new domain; the widget still points at the old one.",
    "How do we give a client a free session as a thank you?",
    "Tried that, still the same. Screenshot attached.",
    "Thanks, that worked!",
    "Any update on this? We have a busy weekend coming.",
    "Sorry, one more question about the same thing.",
    "The confirmation emails go to spam for some clients.",
    "Is there an API endpoint for cancelling a booking?",
    "Our accountant needs invoices with the company address on them.",
    "Can we close bookings 2 hours before a class starts?",
];

/// What the team answers.
const AGENT_LINES: [&str; 24] = [
    "Thanks for getting in touch. I'm looking at your account now.",
    "I can see what happened: the setting changed last Tuesday. I've put it back.",
    "That's fixed now. Could you try again and let me know?",
    "Could you send the time it happened and the client's name? I'll find it in the logs.",
    "I've refunded the duplicate payment; it takes 5 to 10 working days to show.",
    "Yes: under Settings > Locations you can set hours for each place.",
    "I've passed this to the engineers with the details you sent.",
    "Good news: the fix is out. Nothing to do on your side.",
    "That's expected for bookings made before the change. New ones are right.",
    "I've added the column to your export; the next file has it.",
    "You can do that from the client's page: Add credit, then choose the session.",
    "I've reissued the invoice with your company address.",
    "The page follows the browser's language once you switch on Hindi and Punjabi under Languages.",
    "I've moved your widget to the new domain. It may take an hour to show everywhere.",
    "Bookings can close a set time before a class: Classes > Rules > Booking window.",
    "Thanks for your patience on this one.",
    "I'll keep this open until you confirm it works.",
    "Glad that's sorted. I'll close this now; reply any time to open it again.",
    "The API has `DELETE /v2/bookings/{id}`; the docs link is in your dashboard.",
    "Spam filters dislike the old sender; I've switched you to a verified one.",
    "Holidays can be blocked from Calendar > Closures, with each state's list ready to add.",
    "Roles decide who sees reports; I've added it to the Manager role.",
    "Seats are counted when the invoice is made; I've corrected this one.",
    "Let me check with the team and come back to you today.",
];

/// What the team notes for each other.
const NOTE_LINES: [&str; 8] = [
    "Long-standing customer, renewing next month.",
    "Same report from three other accounts today; linked to the incident.",
    "Logs show the change came from their own admin user.",
    "Offered a credit; billing to apply it.",
    "Checked: nothing failed on our side in that window.",
    "Escalated to engineering, ticket in the platform queue.",
    "They asked for a call; booked for tomorrow 10:00.",
    "Waiting on the bank's answer before we refund.",
];

/// More subjects than the scripted topics have, for variety at scale.
const SUBJECTS: [(&str, &str, &str); 24] = [
    ("Booking page won't load on phones", "chat", "high"),
    (
        "Invoice shows the wrong number of seats",
        "help-centre",
        "normal",
    ),
    ("Staff app logs us out every day", "in-app", "normal"),
    ("Double booking in the same room", "chat", "urgent"),
    ("Change the wording of reminder texts", "help-centre", "low"),
    ("New manager can't see reports", "in-app", "normal"),
    ("Block bookings on public holidays", "help-centre", "low"),
    ("Calendar shows old opening hours", "chat", "high"),
    ("Refund a duplicate class payment", "help-centre", "normal"),
    ("Add a second payment method", "phone", "low"),
    (
        "Booking page in the client's language",
        "help-centre",
        "low",
    ),
    ("Waiting list didn't offer a free place", "chat", "high"),
    ("Export is missing phone numbers", "help-centre", "normal"),
    ("Different hours at each location", "phone", "low"),
    ("Widget still points at our old domain", "chat", "high"),
    ("Give a client a free session", "in-app", "low"),
    ("Confirmation emails land in spam", "help-centre", "high"),
    ("Cancel a booking through the API", "api", "normal"),
    ("Company address on our invoices", "help-centre", "low"),
    ("Close bookings two hours before class", "in-app", "normal"),
    ("Payments failing for one card type", "chat", "urgent"),
    ("Import clients from a spreadsheet", "phone", "normal"),
    ("Single sign-on for our staff", "help-centre", "normal"),
    ("Webhook signature doesn't verify", "api", "high"),
];

const FILES: [(&str, i64); 10] = [
    ("screenshot.png", 214_016),
    ("invoice.pdf", 88_064),
    ("bookings-export.csv", 1_536_000),
    ("error-log.txt", 4_096),
    ("calendar.png", 301_568),
    ("receipt.pdf", 64_512),
    ("clients.csv", 712_704),
    ("console.txt", 2_560),
    ("widget-settings.json", 1_024),
    ("photo.jpg", 1_254_400),
];

const FIRST_NAMES: [&str; 32] = [
    "Harleen",
    "Manpreet",
    "Jaspreet",
    "Simran",
    "Amandeep",
    "Rajveer",
    "Navdeep",
    "Kulwinder",
    "Ravneet",
    "Sukhmani",
    "Gurleen",
    "Inderjit",
    "Baljeet",
    "Tejinder",
    "Prabhjot",
    "Jasleen",
    "Harjot",
    "Amrit",
    "Ishaan",
    "Ekta",
    "Kunal",
    "Meera",
    "Tanvi",
    "Ritika",
    "Aditya",
    "Kavya",
    "Ananya",
    "Nikhil",
    "Shreya",
    "Saurabh",
    "Swati",
    "Abhinav",
];

const LAST_NAMES: [&str; 32] = [
    "Sandhu", "Gill", "Dhillon", "Brar", "Grewal", "Bains", "Sidhu", "Cheema", "Randhawa", "Virk",
    "Chahal", "Toor", "Sekhon", "Aulakh", "Ghuman", "Hundal", "Sohal", "Kahlon", "Malhotra",
    "Mehra", "Arora", "Bhatia", "Gupta", "Agarwal", "Khanna", "Joshi", "Talwar", "Bansal", "Goyal",
    "Chawla", "Anand", "Bhalla",
];

const PREFIXES: [&str; 24] = [
    "Sukoon", "Anand", "Shanti", "Prakash", "Sunehri", "Kesar", "Tulsi", "Chandan", "Gulmohar",
    "Neem", "Amaltas", "Pipal", "Sarson", "Basant", "Jharna", "Kiran", "Mehak", "Nirmal", "Roshni",
    "Bageecha", "Sangam", "Nadi", "Rangla", "Saanjh",
];

const KINDS: [&str; 16] = [
    "Yoga Studio",
    "Physiotherapy",
    "Dental Care",
    "Salon",
    "Tutorials",
    "Fitness Studio",
    "Vet Clinic",
    "Pilates",
    "Gents Salon",
    "Ayurveda Centre",
    "Music Academy",
    "Swimming Academy",
    "Counselling",
    "Gym",
    "Spa",
    "Language Institute",
];

/// Cities, all in India.
const PLACES: [&str; 24] = [
    "Ludhiana",
    "Amritsar",
    "Jalandhar",
    "Chandigarh",
    "Patiala",
    "Mohali",
    "Bathinda",
    "Delhi",
    "Gurugram",
    "Noida",
    "Jaipur",
    "Lucknow",
    "Dehradun",
    "Shimla",
    "Varanasi",
    "Kanpur",
    "Mumbai",
    "Pune",
    "Bengaluru",
    "Panchkula",
    "Hoshiarpur",
    "Moga",
    "Kapurthala",
    "Zirakpur",
];

/// Text as an SQL literal: in single quotes, its quotes doubled.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// `VALUES (...), (...)` of rows already written as literals.
fn values(rows: impl IntoIterator<Item = String>) -> String {
    rows.into_iter()
        .map(|row| format!("({row})"))
        .collect::<Vec<_>>()
        .join(",")
}

/// A name in lower case and plain letters, for an address.
fn plain(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|ch| match ch {
            'ł' => 'l',
            'é' | 'è' => 'e',
            'á' => 'a',
            'ó' => 'o',
            other => other,
        })
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// The kind of the `i`th message of a conversation: the customer opens, the two sides
/// take turns, and every ninth line is a note for the team.
fn kind_of(i: i64) -> &'static str {
    if i == 1 {
        "customer"
    } else if i % 9 == 0 {
        "note"
    } else if i % 2 == 0 {
        "agent"
    } else {
        "customer"
    }
}

/// What the run made.
#[derive(Clone, Copy, Debug, Default)]
pub struct Made {
    pub conversations: i64,
    pub messages: i64,
    pub customers: i64,
}

/// The small tables the rows are made from, as statements.
fn word_tables() -> Vec<String> {
    let mut out = vec![
        "DROP TABLE IF EXISTS temp.seed_k".to_owned(),
        "DROP TABLE IF EXISTS temp.seed_line".to_owned(),
        "DROP TABLE IF EXISTS temp.seed_subject".to_owned(),
        "DROP TABLE IF EXISTS temp.seed_file".to_owned(),
        "DROP TABLE IF EXISTS temp.seed_agent".to_owned(),
        "DROP TABLE IF EXISTS temp.seed_name".to_owned(),
        "DROP TABLE IF EXISTS temp.seed_company".to_owned(),
        "CREATE TEMP TABLE seed_k (i INTEGER PRIMARY KEY, kind TEXT NOT NULL, pool INTEGER NOT NULL)".to_owned(),
        "CREATE TEMP TABLE seed_line (kind TEXT NOT NULL, n INTEGER NOT NULL, body TEXT NOT NULL, PRIMARY KEY (kind, n))".to_owned(),
        "CREATE TEMP TABLE seed_subject (n INTEGER PRIMARY KEY, subject TEXT NOT NULL, channel TEXT NOT NULL, priority TEXT NOT NULL)".to_owned(),
        "CREATE TEMP TABLE seed_file (n INTEGER PRIMARY KEY, name TEXT NOT NULL, bytes INTEGER NOT NULL)".to_owned(),
        "CREATE TEMP TABLE seed_agent (n INTEGER PRIMARY KEY, id INTEGER NOT NULL, name TEXT NOT NULL)".to_owned(),
        "CREATE TEMP TABLE seed_name (n INTEGER PRIMARY KEY, first TEXT NOT NULL, last TEXT NOT NULL, address TEXT NOT NULL)".to_owned(),
        "CREATE TEMP TABLE seed_company (n INTEGER PRIMARY KEY, name TEXT NOT NULL, country TEXT NOT NULL, city TEXT NOT NULL)".to_owned(),
    ];
    let pool = |kind: &str| match kind {
        "customer" => CUSTOMER_LINES.len(),
        "agent" => AGENT_LINES.len(),
        _ => NOTE_LINES.len(),
    };
    out.push(format!(
        "INSERT INTO seed_k (i, kind, pool) VALUES {}",
        values((1..=100).map(|i| format!("{i}, '{}', {}", kind_of(i), pool(kind_of(i)))))
    ));
    let mut lines = Vec::new();
    for (kind, list) in [
        ("customer", &CUSTOMER_LINES[..]),
        ("agent", &AGENT_LINES[..]),
        ("note", &NOTE_LINES[..]),
    ] {
        for (n, body) in list.iter().enumerate() {
            lines.push(format!("'{kind}', {n}, {}", literal(body)));
        }
    }
    out.push(format!(
        "INSERT INTO seed_line (kind, n, body) VALUES {}",
        values(lines)
    ));
    let mut subjects: Vec<(String, &str, &str)> = SUBJECTS
        .iter()
        .map(|(subject, channel, priority)| ((*subject).to_owned(), *channel, *priority))
        .collect();
    for topic in &TOPICS {
        subjects.push((
            topic.subject.to_owned(),
            topic.channel.key(),
            topic.priority.key(),
        ));
    }
    out.push(format!(
        "INSERT INTO seed_subject (n, subject, channel, priority) VALUES {}",
        values(
            subjects
                .iter()
                .enumerate()
                .map(|(n, (subject, channel, priority))| {
                    format!("{n}, {}, '{channel}', '{priority}'", literal(subject))
                })
        )
    ));
    out.push(format!(
        "INSERT INTO seed_file (n, name, bytes) VALUES {}",
        values(
            FILES
                .iter()
                .enumerate()
                .map(|(n, (name, bytes))| format!("{n}, {}, {bytes}", literal(name)))
        )
    ));
    // Two people are away: work goes to the others.
    out.push(format!(
        "INSERT INTO seed_agent (n, id, name) VALUES {}",
        values(
            AGENTS
                .iter()
                .enumerate()
                .filter(|(at, _)| !matches!(at, 21 | 23))
                .enumerate()
                .map(|(n, (at, (name, _)))| format!("{n}, {}, {}", at + 1, literal(name)))
        )
    ));
    let mut names = Vec::new();
    for (a, first) in FIRST_NAMES.iter().enumerate() {
        for (b, last) in LAST_NAMES.iter().enumerate() {
            let n = a * LAST_NAMES.len() + b;
            names.push(format!(
                "{n}, {}, {}, {}",
                literal(first),
                literal(last),
                literal(&format!("{}.{}", plain(first), plain(last)))
            ));
        }
    }
    out.push(format!(
        "INSERT INTO seed_name (n, first, last, address) VALUES {}",
        values(names)
    ));
    let mut companies = Vec::new();
    for (a, prefix) in PREFIXES.iter().enumerate() {
        for (b, kind) in KINDS.iter().enumerate() {
            let n = a * KINDS.len() + b;
            let city = PLACES[(a * 7 + b * 3) % PLACES.len()];
            companies.push(format!(
                "{n}, {}, 'IN', {}",
                literal(&format!("{prefix} {kind}")),
                literal(city)
            ));
        }
    }
    out.push(format!(
        "INSERT INTO seed_company (n, name, country, city) VALUES {}",
        values(companies)
    ));
    out
}

/// Writes `tickets` conversations, their messages, files and events, and the customers
/// they come from, as the desk stands at `now` (today is `today`, `YYYY-MM-DD`). The
/// tables are empty of generated rows; the ids start after the hand-written ones.
pub async fn seed(db: &Db, now: i64, today: &str, tickets: i64) -> Result<Made, BoxError> {
    if tickets <= 0 {
        return Ok(Made::default());
    }
    // A customer for about every twenty conversations.
    let customers = (tickets / 20).clamp(50, 5_000);
    let agents = i64::try_from(AGENTS.len() - 2).unwrap_or(22);
    let subjects = i64::try_from(SUBJECTS.len() + TOPICS.len()).unwrap_or(1);
    let names = i64::try_from(FIRST_NAMES.len() * LAST_NAMES.len()).unwrap_or(1);
    let companies = i64::try_from(PREFIXES.len() * KINDS.len()).unwrap_or(1);
    let plans: Vec<String> = Plan::ALL
        .iter()
        .enumerate()
        .map(|(n, plan)| format!("WHEN {n} THEN '{}'", plan.key()))
        .collect();
    let seat_paise: Vec<String> = Plan::ALL
        .iter()
        .map(|plan| format!("WHEN '{}' THEN {}", plan.key(), plan.seat_paise()))
        .collect();
    let targets: Vec<String> = Priority::ALL
        .iter()
        .map(|priority| format!("WHEN '{}' THEN {}", priority.key(), priority.target()))
        .collect();
    let today = literal(today);
    let tx = db.pool().begin().await?;
    for statement in word_tables() {
        tx.execute_script(&statement).await?;
    }
    // The customers: a name, a business, a place, a plan and what they have paid.
    // Demo seed: only counts, the plans' fixed keys and `literal(today)` are written in.
    // nosemgrep: navree-sql-built-from-strings
    tx.execute_script(&format!(
        "WITH RECURSIVE g(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM g WHERE i < {last}),
         h AS (SELECT i, (i * 2654435761 + 97) % 4294967296 AS a FROM g),
         p AS (SELECT i, a, CASE a % 4 {plans} END AS plan, 3 + (a / 5) % 40 AS months FROM h)
         INSERT INTO customers (id, name, email, phone, company, role, plan, lifetime_paise, seats, country, city, since, tags)
         SELECT {FIRST_CUSTOMER} + p.i, s.first || ' ' || s.last, s.address || p.i || '@example.org',
                '+91 55555 ' || printf('%05d', ({FIRST_CUSTOMER} + p.i) % 100000),
                c.name, CASE p.a % 3 WHEN 0 THEN 'Owner' WHEN 1 THEN 'Front desk lead' ELSE 'Practice manager' END,
                p.plan,
                (CASE p.plan {seat_paise} END) * (2 + p.a % 20) * p.months,
                2 + p.a % 20, c.country, c.city,
                date({today}, '-' || p.months || ' months'),
                CASE p.a % 5 WHEN 0 THEN 'vip' WHEN 1 THEN 'multi-location,annual' WHEN 2 THEN 'api' WHEN 3 THEN '' ELSE 'migrating' END
         FROM p JOIN seed_name s ON s.n = (p.i * 37) % {names}
                JOIN seed_company c ON c.n = (p.i * 11 + p.a % 7) % {companies}",
        last = customers - 1,
        plans = plans.join(" "),
        seat_paise = seat_paise.join(" "),
    ))
    .await?;
    // The conversations: most long closed; a few days' worth open, pending or snoozed.
    // Demo seed: only counts and fixed CASE fragments are written in.
    // nosemgrep: navree-sql-built-from-strings
    tx.execute_script(&format!(
        "WITH RECURSIVE g(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM g WHERE i < {last}),
         h AS (SELECT i, (i * 2654435761 + 97) % 4294967296 AS a, (i * 40503 + 7919) % 1000003 AS b,
                      (i * 69069 + 12345) % 2147483647 AS c,
                      ((i * 1103515245 + 12345) % 2147483648) / 65536 AS d FROM g),
         t AS (SELECT i, a, c, {FIRST_CONVERSATION} + i AS id,
                      25200 + CAST((b * 1.0 / 1000003) * (b * 1.0 / 1000003) * 63072000 AS INTEGER) AS age,
                      10 + d % 91 AS m FROM h),
         u AS (SELECT t.*, s.subject, s.channel, s.priority,
                      CASE WHEN t.age < 259200 THEN (CASE WHEN t.a % 10 < 4 THEN 'open' WHEN t.a % 10 < 7 THEN 'pending' ELSE 'closed' END)
                           WHEN t.age < 1209600 THEN (CASE WHEN t.a % 20 < 5 THEN 'pending' WHEN t.a % 20 < 7 THEN 'snoozed' ELSE 'closed' END)
                           ELSE 'closed' END AS status,
                      CASE WHEN t.m % 9 = 0 THEN t.m - 1 ELSE t.m END AS shown
               FROM t JOIN seed_subject s ON s.n = t.a % {subjects})
         INSERT INTO conversations (id, customer_id, subject, channel, status, priority, assignee_id, created_at, last_at,
                                    sla_due, sla_from, snoozed_until, unread, preview, rev, messages, inbox)
         SELECT u.id,
                CASE WHEN u.a % 25 = 0 THEN 1 + u.a % 24 ELSE {FIRST_CUSTOMER} + (u.a / 3) % {customers} END,
                u.subject, u.channel, u.status, u.priority,
                CASE WHEN u.status = 'open' AND u.a % 10 = 3 THEN NULL ELSE ag.id END,
                {now} - u.age - u.m * (600 + u.c % 7200), {now} - u.age,
                CASE WHEN u.status = 'open' THEN {now} - u.age + (CASE u.priority {targets} END) END,
                CASE WHEN u.status = 'open' THEN {now} - u.age END,
                CASE WHEN u.status = 'snoozed' THEN date({today}, '+' || (1 + u.a % 10) || ' days') END,
                CASE WHEN u.status = 'open' AND u.a % 3 = 0 THEN 1 + u.a % 2 ELSE 0 END,
                substr(l.body, 1, 120), 1, u.m,
                CASE WHEN u.status IN ('open', 'pending') THEN 'open' ELSE u.status END
         FROM u JOIN seed_agent ag ON ag.n = (u.a / 7) % {agents}
                JOIN seed_k k ON k.i = u.shown
                JOIN seed_line l ON l.kind = k.kind AND l.n = (u.id * 7 + u.shown * 13) % k.pool",
        last = tickets - 1,
        targets = targets.join(" "),
    ))
    .await?;
    // Their messages, oldest first, so ids run in time within each conversation.
    tx.execute_script(&format!(
        "INSERT INTO messages (conversation_id, kind, author, agent_id, body, at)
         SELECT c.id, k.kind,
                CASE k.kind WHEN 'customer' THEN cu.name ELSE COALESCE(ag.name, 'Priya Verma') END,
                CASE k.kind WHEN 'customer' THEN NULL ELSE COALESCE(c.assignee_id, 2) END,
                l.body,
                c.created_at + ((c.last_at - c.created_at) * (k.i - 1)) / MAX(c.messages - 1, 1)
         FROM conversations c
         JOIN seed_k k ON k.i <= c.messages
         JOIN customers cu ON cu.id = c.customer_id
         LEFT JOIN seed_agent ag ON ag.id = c.assignee_id
         JOIN seed_line l ON l.kind = k.kind AND l.n = (c.id * 7 + k.i * 13) % k.pool
         WHERE c.id >= {FIRST_CONVERSATION}
         ORDER BY c.id, k.i"
    ))
    .await?;
    // A file with about one customer message in sixty.
    tx.execute_script(&format!(
        "INSERT INTO attachments (message_id, conversation_id, name, bytes)
         SELECT m.id, m.conversation_id, f.name, f.bytes
         FROM messages m JOIN seed_file f ON f.n = m.id % 10
         WHERE m.conversation_id >= {FIRST_CONVERSATION} AND m.kind = 'customer' AND m.id % 61 = 0"
    ))
    .await?;
    // Each customer's account: signing up, a plan, the latest invoice; and each
    // conversation: when it was opened, and when it was resolved.
    // Demo seed: only constants and the `now` timestamp (an i64) are written in.
    // nosemgrep: navree-sql-built-from-strings
    tx.execute_script(&format!(
        "INSERT INTO customer_events (customer_id, kind, at, title, detail)
         SELECT id, 'signed_up', CAST(strftime('%s', since) AS INTEGER), 'Signed up', 'Started a 14-day trial'
         FROM customers WHERE id >= {FIRST_CUSTOMER};
         INSERT INTO customer_events (customer_id, kind, at, title, detail)
         SELECT id, 'plan_changed', CAST(strftime('%s', since) AS INTEGER) + 1209600, 'Moved to ' || upper(substr(plan, 1, 1)) || substr(plan, 2), seats || ' seats, paid monthly'
         FROM customers WHERE id >= {FIRST_CUSTOMER};
         INSERT INTO customer_events (customer_id, kind, at, title, detail)
         SELECT id, 'invoice_paid', {now} - 86400 * (1 + id % 25), 'Paid the monthly invoice', 'Paid by UPI AutoPay'
         FROM customers WHERE id >= {FIRST_CUSTOMER};
         INSERT INTO customer_events (customer_id, kind, at, title, detail)
         SELECT customer_id, 'ticket_opened', created_at, 'Wrote in: ' || subject, 'Through ' || replace(channel, '-', ' ')
         FROM conversations WHERE id >= {FIRST_CONVERSATION};
         INSERT INTO customer_events (customer_id, kind, at, title, detail)
         SELECT customer_id, 'ticket_closed', last_at + 300, 'Resolved: ' || subject, 'Closed by the team'
         FROM conversations WHERE id >= {FIRST_CONVERSATION} AND status = 'closed'"
    ))
    .await?;
    for table in [
        "seed_k",
        "seed_line",
        "seed_subject",
        "seed_file",
        "seed_agent",
        "seed_name",
        "seed_company",
    ] {
        tx.execute_script(&format!("DROP TABLE IF EXISTS temp.{table}"))
            .await?;
    }
    let messages = tx
        .fetch_scalar::<i64>(&Statement::portable(
            "SELECT COALESCE(SUM(messages), 0) FROM conversations WHERE id >= ?",
            [Value::I64(FIRST_CONVERSATION)],
        ))
        .await?;
    tx.commit().await?;
    Ok(Made {
        conversations: tickets,
        messages,
        customers,
    })
}

/// Generated conversation `id` and its messages, as [`seed`] writes them into a desk of
/// [`FULL`] size at `now`, worked out here with the same arithmetic: for pages that draw
/// one with no database, such as the docs.
pub fn generated(id: i64, now: i64) -> (crate::data::Conversation, Vec<crate::data::Message>) {
    let i = (id - FIRST_CONVERSATION).max(0);
    let a = (i * 2_654_435_761 + 97) % 4_294_967_296;
    let b = (i * 40_503 + 7_919) % 1_000_003;
    let c = (i * 69_069 + 12_345) % 2_147_483_647;
    let d = ((i * 1_103_515_245 + 12_345) % 2_147_483_648) / 65_536;
    let share = b as f64 / 1_000_003.0;
    let age = 25_200 + (share * share * 63_072_000.0) as i64;
    let m = 10 + d % 91;
    let subjects = SUBJECTS.len() as i64 + TOPICS.len() as i64;
    let at = usize::try_from(a % subjects).unwrap_or(0);
    let (subject, channel, priority) = match SUBJECTS.get(at) {
        Some((subject, channel, priority)) => ((*subject).to_owned(), *channel, *priority),
        None => {
            let topic = &TOPICS[at - SUBJECTS.len()];
            (
                topic.subject.to_owned(),
                topic.channel.key(),
                topic.priority.key(),
            )
        }
    };
    let status = if age < 259_200 {
        match a % 10 {
            0..=3 => "open",
            4..=6 => "pending",
            _ => "closed",
        }
    } else if age < 1_209_600 {
        match a % 20 {
            0..=4 => "pending",
            5..=6 => "snoozed",
            _ => "closed",
        }
    } else {
        "closed"
    };
    let customers = (FULL / 20).clamp(50, 5_000);
    let customer_id = if a % 25 == 0 {
        1 + a % 24
    } else {
        FIRST_CUSTOMER + (a / 3) % customers
    };
    let names = FIRST_NAMES.len() * LAST_NAMES.len();
    let n =
        usize::try_from(((customer_id - FIRST_CUSTOMER).max(0) * 37) % names as i64).unwrap_or(0);
    let customer = format!(
        "{} {}",
        FIRST_NAMES[n / LAST_NAMES.len()],
        LAST_NAMES[n % LAST_NAMES.len()]
    );
    let active: Vec<(i64, &str)> = AGENTS
        .iter()
        .enumerate()
        .filter(|(at, _)| !matches!(at, 21 | 23))
        .map(|(at, (name, _))| (i64::try_from(at).unwrap_or(0) + 1, *name))
        .collect();
    let (agent_id, agent) = active[usize::try_from((a / 7) % active.len() as i64).unwrap_or(0)];
    let assignee = (!(status == "open" && a % 10 == 3)).then_some(agent_id);
    let last_at = now - age;
    let created_at = last_at - m * (600 + c % 7_200);
    let line = |k: i64| -> &'static str {
        let kind = kind_of(k);
        let pool: &[&str] = match kind {
            "customer" => &CUSTOMER_LINES,
            "agent" => &AGENT_LINES,
            _ => &NOTE_LINES,
        };
        pool[usize::try_from((id * 7 + k * 13) % pool.len() as i64).unwrap_or(0)]
    };
    let shown = if m % 9 == 0 { m - 1 } else { m };
    let conversation = crate::data::Conversation {
        id,
        customer_id,
        subject,
        channel: channel.to_owned(),
        status: status.to_owned(),
        priority: priority.to_owned(),
        assignee_id: assignee,
        created_at,
        last_at,
        last_rev: 0,
        sla_due: None,
        sla_from: None,
        snoozed_until: None,
        unread: 0,
        preview: line(shown).chars().take(120).collect(),
        rev: 1,
        messages: m,
        inbox: crate::data::inbox_of(status).to_owned(),
    };
    let messages = (1..=m)
        .map(|k| {
            let kind = kind_of(k);
            crate::data::Message {
                id: 1_000_000 + id * 100 + k,
                conversation_id: id,
                kind: kind.to_owned(),
                author: if kind == "customer" {
                    customer.clone()
                } else {
                    assignee.map_or("Priya Verma", |_| agent).to_owned()
                },
                agent_id: (kind != "customer").then_some(assignee.unwrap_or(2)),
                body: line(k).to_owned(),
                at: created_at + ((last_at - created_at) * (k - 1)) / (m - 1).max(1),
            }
        })
        .collect();
    (conversation, messages)
}

/// The indexes every query of the app reads by, and the search index, built once the
/// rows are in. Then the planner's statistics.
///
/// A list reads one view, newest first, by the view's index. The channel and priority
/// filters have indexes of their own under each list (`inbox`), alone and together, so a
/// filter that matches little reads only what it matches instead of walking the list.
/// "Mine" and "Unassigned" read by the agent and by the open, unassigned conversations:
/// sets of a few hundred, which a filter narrows as it reads them.
pub const INDEXES: [&str; 16] = [
    "CREATE INDEX IF NOT EXISTS conversations_inbox ON conversations (inbox, last_at DESC, last_rev DESC, id DESC)",
    "CREATE INDEX IF NOT EXISTS conversations_channel ON conversations (inbox, channel, last_at DESC, last_rev DESC, id DESC)",
    "CREATE INDEX IF NOT EXISTS conversations_priority ON conversations (inbox, priority, last_at DESC, last_rev DESC, id DESC)",
    "CREATE INDEX IF NOT EXISTS conversations_channel_priority ON conversations (inbox, channel, priority, last_at DESC, last_rev DESC, id DESC)",
    "CREATE INDEX IF NOT EXISTS conversations_mine ON conversations (assignee_id, inbox, last_at DESC, last_rev DESC, id DESC)",
    "CREATE INDEX IF NOT EXISTS conversations_unassigned ON conversations (status, assignee_id, last_at DESC, last_rev DESC, id DESC)",
    "CREATE INDEX IF NOT EXISTS conversations_rev ON conversations (rev)",
    "CREATE INDEX IF NOT EXISTS conversations_customer ON conversations (customer_id, last_at DESC, last_rev DESC, id DESC)",
    "CREATE INDEX IF NOT EXISTS conversations_clock ON conversations (status, sla_due)",
    "CREATE INDEX IF NOT EXISTS conversations_unread ON conversations (inbox, assignee_id) WHERE unread > 0",
    "CREATE INDEX IF NOT EXISTS messages_conversation ON messages (conversation_id, id)",
    "CREATE INDEX IF NOT EXISTS attachments_message ON attachments (message_id)",
    "CREATE INDEX IF NOT EXISTS customer_events_customer ON customer_events (customer_id, at)",
    "CREATE INDEX IF NOT EXISTS agents_name ON agents (name, id)",
    "INSERT INTO conversations_fts (rowid, subject, customer, company) SELECT c.id, c.subject, cu.name, cu.company FROM conversations c JOIN customers cu ON cu.id = c.customer_id",
    "ANALYZE",
];

/// Search words as a full-text query: each word a prefix, all of them needed. `None`
/// when nothing searchable is left.
pub fn match_query(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(8)
        .map(|word| format!("\"{}\"*", word.to_lowercase()))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_words_become_a_safe_prefix_query() {
        assert_eq!(
            match_query("Card declined"),
            Some("\"card\"* \"declined\"*".to_owned())
        );
        assert_eq!(
            match_query("\"; DROP TABLE x; --"),
            Some("\"drop\"* \"table\"* \"x\"*".to_owned())
        );
        assert_eq!(match_query("   ***  "), None);
        assert_eq!(match_query("Wrocław"), Some("\"wrocław\"*".to_owned()));
        assert_eq!(
            match_query(&"a ".repeat(50)).map(|q| q.matches('*').count()),
            Some(8)
        );
    }

    #[test]
    fn the_word_tables_are_quoted() {
        let tables = word_tables().join(";\n");
        assert!(tables.contains(
            "'Hi, a client says the booking page won''t load on their phone. Can you check?'"
        ));
        assert_eq!(kind_of(1), "customer");
        assert_eq!(kind_of(2), "agent");
        assert_eq!(kind_of(9), "note");
        assert_eq!(literal("it's"), "'it''s'");
    }
}
