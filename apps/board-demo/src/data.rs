//! What the app stores: one team's board, its cards, their checklists, comments
//! and what happened to them, with a seed that writes the same board on every run.
//!
//! The team, the product and every person are invented. Addresses are on
//! `example.com`.

use chrono::{DateTime, Days, NaiveDate};
use navree::orm::Model;
use navree::ui::sortable::RANK_GAP;
use navree::ui::{Tone, icons};

/// The team.
pub const TEAM: &str = "Saltmarsh";
/// The product the board plans: a field-notes app for coastal surveys.
pub const PROJECT: &str = "Tern 3.2";

/// A column of the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Backlog,
    Ready,
    Doing,
    Review,
    Done,
}

impl Column {
    pub const ALL: [Self; 5] = [
        Self::Backlog,
        Self::Ready,
        Self::Doing,
        Self::Review,
        Self::Done,
    ];

    /// What a move posts.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Ready => "ready",
            Self::Doing => "doing",
            Self::Review => "review",
            Self::Done => "done",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Backlog => "Backlog",
            Self::Ready => "Ready",
            Self::Doing => "In progress",
            Self::Review => "Review",
            Self::Done => "Done",
        }
    }

    /// The most cards it holds at once: work in progress is kept small on purpose.
    pub const fn limit(self) -> Option<u32> {
        match self {
            Self::Ready => Some(5),
            Self::Doing => Some(3),
            Self::Review => Some(2),
            Self::Backlog | Self::Done => None,
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|column| column.key() == key)
    }

    pub const fn tone(self) -> Tone {
        match self {
            Self::Backlog => Tone::Neutral,
            Self::Ready => Tone::Info,
            Self::Doing => Tone::Warning,
            Self::Review => Tone::named("violet"),
            Self::Done => Tone::Success,
        }
    }
}

/// A label on a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    Bug,
    Feature,
    Design,
    Speed,
    Docs,
    Chore,
}

impl Label {
    pub const ALL: [Self; 6] = [
        Self::Bug,
        Self::Feature,
        Self::Design,
        Self::Speed,
        Self::Docs,
        Self::Chore,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Feature => "feature",
            Self::Design => "design",
            Self::Speed => "speed",
            Self::Docs => "docs",
            Self::Chore => "chore",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Bug => "Bug",
            Self::Feature => "Feature",
            Self::Design => "Design",
            Self::Speed => "Speed",
            Self::Docs => "Docs",
            Self::Chore => "Chore",
        }
    }

    pub const fn tone(self) -> Tone {
        match self {
            Self::Bug => Tone::Danger,
            Self::Feature => Tone::Info,
            Self::Design => Tone::named("violet"),
            Self::Speed => Tone::Warning,
            Self::Docs => Tone::named("cyan"),
            Self::Chore => Tone::Neutral,
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|label| label.key() == key)
    }
}

/// The labels a card's comma-separated list names, in the order of [`Label::ALL`].
pub fn labels_of(text: &str) -> Vec<Label> {
    let keys: Vec<&str> = text.split(',').map(str::trim).collect();
    Label::ALL
        .into_iter()
        .filter(|label| keys.contains(&label.key()))
        .collect()
}

/// Someone on the team.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "people")]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub email: String,
    /// What they do.
    pub job: String,
    /// `lead` or `member`: a lead closes cards.
    pub role: String,
}

impl Person {
    pub fn is_lead(&self) -> bool {
        self.role == "lead"
    }
}

/// A card, as stored. Its place is its column and its rank there: cards are in
/// rank order, and a move writes one rank between its new neighbours'.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "cards")]
pub struct Card {
    pub id: i64,
    /// A [`Column`]'s key.
    pub list: String,
    pub rank: i64,
    pub title: String,
    pub description: String,
    pub assignee: Option<i64>,
    /// [`Label`] keys, comma-separated.
    pub labels: String,
    pub due: Option<NaiveDate>,
    /// Unix seconds.
    pub created_at: i64,
    /// How many checklist lines are ticked, and how many there are, kept with the
    /// card so the board reads one row a card.
    pub checks_done: i64,
    pub checks_total: i64,
    /// How many comments it has.
    pub comments: i64,
}

/// One line of a card's checklist.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "checks")]
pub struct Check {
    pub id: i64,
    pub card_id: i64,
    pub position: i64,
    pub text: String,
    /// 1 when ticked.
    pub done: i64,
}

/// A comment on a card.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "comments")]
pub struct Comment {
    pub id: i64,
    pub card_id: i64,
    pub person_id: i64,
    pub at: i64,
    pub body: String,
}

/// Something that happened to a card: made, moved, commented on, a line ticked.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "activity")]
pub struct Activity {
    pub id: i64,
    pub card_id: i64,
    pub person_id: i64,
    pub at: i64,
    /// `created`, `moved`, `commented` or `checked`.
    pub kind: String,
    pub detail: String,
}

/// The board as the seed writes it.
#[derive(Clone, Debug, Default)]
pub struct Seed {
    pub people: Vec<Person>,
    pub cards: Vec<Card>,
    pub checks: Vec<Check>,
    pub comments: Vec<Comment>,
    pub activity: Vec<Activity>,
}

/// The day `at` falls on, in UTC.
pub fn day_of(at: i64) -> NaiveDate {
    DateTime::from_timestamp(at, 0)
        .map(|time| time.date_naive())
        .unwrap_or_default()
}

/// The icon that stands for a person: people are shown by an icon and their name,
/// never by letters.
pub fn person_icon(person: &Person) -> icons::Icon {
    if person.is_lead() {
        icons::USER_ROUND_CHECK
    } else {
        icons::CIRCLE_USER_ROUND
    }
}

const PEOPLE: [(&str, &str, &str, &str); 8] = [
    (
        "Mara Quinn",
        "mara.quinn@example.com",
        "Product lead",
        "lead",
    ),
    (
        "Theo Park",
        "theo.park@example.com",
        "Android engineer",
        "member",
    ),
    (
        "Ines Okafor",
        "ines.okafor@example.com",
        "iOS engineer",
        "member",
    ),
    (
        "Ravi Menon",
        "ravi.menon@example.com",
        "Backend engineer",
        "member",
    ),
    (
        "Lena Brandt",
        "lena.brandt@example.com",
        "Designer",
        "member",
    ),
    (
        "Sol Ferreira",
        "sol.ferreira@example.com",
        "Support and docs",
        "member",
    ),
    (
        "Kai Nakamura",
        "kai.nakamura@example.com",
        "Data engineer",
        "member",
    ),
    (
        "Priya Raman",
        "priya.raman@example.com",
        "Quality engineer",
        "member",
    ),
];

/// Column, title, assignee, labels, due in days from today (None: no date), the
/// description, and the checklist with what is ticked.
type CardSeed = (
    Column,
    &'static str,
    i64,
    &'static str,
    Option<i64>,
    &'static str,
    &'static [(&'static str, bool)],
);

const CARDS: [CardSeed; 16] = [
    (
        Column::Backlog,
        "Offline maps for tidal zones",
        3,
        "feature",
        None,
        "Surveyors lose signal on the flats. Cache the tiles for the zones a survey covers, the day before it runs.",
        &[
            ("Pick a tile budget per zone", false),
            ("Cache on Wi-Fi only", false),
            ("Show what is cached", false),
        ],
    ),
    (
        Column::Backlog,
        "Export a survey as CSV",
        4,
        "feature",
        Some(12),
        "One row per sighting, with the species, the count, the place and the time, for the partners who keep spreadsheets.",
        &[],
    ),
    (
        Column::Backlog,
        "Dark mode for the species guide",
        5,
        "design",
        None,
        "The guide is read at dawn on the shore. A dark theme that keeps the photos true to colour.",
        &[],
    ),
    (
        Column::Backlog,
        "Crash when a photo is over 20 MB",
        2,
        "bug",
        Some(3),
        "Reported from a tablet with a 108 MP camera. The upload reads the whole file into memory.",
        &[
            ("Reproduce on a 108 MP photo", true),
            ("Stream the upload", false),
        ],
    ),
    (
        Column::Backlog,
        "Tidy the settings screen",
        6,
        "chore",
        None,
        "Three settings nobody uses, and two that should sit together.",
        &[],
    ),
    (
        Column::Ready,
        "Sync conflicts show both versions",
        1,
        "feature",
        Some(5),
        "When two people edit one sighting offline, show both and let the second choose, instead of keeping the last.",
        &[
            ("Write the merge rules", true),
            ("Design the side-by-side view", true),
            ("Build it", false),
            ("Test with two phones offline", false),
        ],
    ),
    (
        Column::Ready,
        "Retry uploads on a weak signal",
        4,
        "speed",
        Some(2),
        "Uploads fail on one bar and never try again. Back off and retry, and say how many are waiting.",
        &[
            ("Back off 1 s, 2 s, 4 s up to a minute", false),
            ("Show the waiting count", false),
        ],
    ),
    (
        Column::Ready,
        "Write the release notes for 3.2",
        6,
        "docs",
        Some(6),
        "What changed, in the words surveyors use, with a screenshot of the new summary card.",
        &[],
    ),
    (
        Column::Ready,
        "Species search ignores accents",
        3,
        "bug,feature",
        Some(-1),
        "Searching \"Gaviao\" doesn't find \"Gavião\". Fold accents when matching, keep them when showing.",
        &[
            ("Fold accents in the index", true),
            ("Keep them in results", false),
        ],
    ),
    (
        Column::Doing,
        "Fix the sign-in redirect on Android",
        2,
        "bug",
        Some(0),
        "After signing in with a magic link, Android opens the browser instead of the app. The intent filter misses the new path.",
        &[
            ("Add the path to the intent filter", true),
            ("Test on Android 13, 14 and 15", true),
            ("Ship to the beta group", false),
        ],
    ),
    (
        Column::Doing,
        "Speed up the map's first draw",
        5,
        "speed,design",
        Some(4),
        "The map takes 2.4 s to first draw on a mid-range phone. Draw the last viewport from cache first.",
        &[
            ("Measure on three phones", true),
            ("Draw from cache", false),
            ("Measure again", false),
        ],
    ),
    (
        Column::Doing,
        "Redesign the survey summary card",
        1,
        "design",
        Some(8),
        "The card shows twelve numbers. Keep four, and let the rest open on a tap.",
        &[
            ("Pick the four numbers with surveyors", true),
            ("Draw it", true),
            ("Build it", false),
        ],
    ),
    (
        Column::Review,
        "Batch-tag photos from a dive",
        3,
        "feature",
        Some(1),
        "Select many photos and tag them with one species at once.",
        &[
            ("Select many", true),
            ("Tag many", true),
            ("Undo a batch", true),
        ],
    ),
    (
        Column::Done,
        "Move the photo cache to the new store",
        4,
        "chore",
        None,
        "The old cache kept every size of every photo. The new one keeps two.",
        &[],
    ),
    (
        Column::Done,
        "Add the tide table to the home screen",
        5,
        "feature",
        None,
        "The next two tides, from the nearest station, on the home screen.",
        &[("Find the nearest station", true), ("Draw the table", true)],
    ),
    (
        Column::Done,
        "Labels on the map controls",
        6,
        "design,docs",
        None,
        "Every map control says what it does to a screen reader.",
        &[],
    ),
];

const COMMENTS: [(usize, i64, i64, &str); 7] = [
    (
        9,
        2,
        26 * 3600,
        "The intent filter only lists /login. The new links go to /auth/link.",
    ),
    (
        9,
        1,
        25 * 3600,
        "Good find. Can you check the iOS side has the same path?",
    ),
    (
        9,
        3,
        23 * 3600,
        "iOS is fine: universal links read the whole domain.",
    ),
    (
        10,
        5,
        20 * 3600,
        "First draw from cache is down to 0.6 s on the Pixel 6a.",
    ),
    (
        5,
        1,
        50 * 3600,
        "Let's show both versions side by side, newest on the right.",
    ),
    (
        12,
        3,
        6 * 3600,
        "Ready for a look. Undo works for the last batch only.",
    ),
    (
        8,
        2,
        30 * 3600,
        "Folding with Unicode NFD and dropping the marks covers Portuguese and Spanish.",
    ),
];

/// How many cards the seed writes: a project some years old, most of it done or
/// waiting in the backlog.
pub const CARD_COUNT: usize = 20_000;

/// The order a column's ranks start in, apart by [`RANK_GAP`].
fn rank(at: usize) -> i64 {
    i64::try_from(at).unwrap_or(i64::MAX / RANK_GAP) * RANK_GAP
}

/// A small fixed sequence of numbers, so every run writes the same board.
struct Dice(u64);

impl Dice {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        if n == 0 {
            0
        } else {
            usize::try_from(self.0 % n as u64).unwrap_or(0)
        }
    }
}

const VERBS: [&str; 10] = [
    "Fix",
    "Speed up",
    "Add",
    "Tidy",
    "Write up",
    "Redesign",
    "Test",
    "Measure",
    "Simplify",
    "Translate",
];
const SUBJECTS: [&str; 16] = [
    "the map legend",
    "photo uploads",
    "species search",
    "offline sync",
    "the sighting export",
    "the tide table",
    "the survey summary",
    "the field guide",
    "sign-in",
    "the settings screen",
    "dive logs",
    "weather notes",
    "the team list",
    "GPS tracks",
    "photo tags",
    "the release checklist",
];
const PLACES: [&str; 8] = [
    "on Android",
    "on iOS",
    "on a weak signal",
    "for tablets",
    "in the web app",
    "for new surveyors",
    "in Portuguese",
    "with large photos",
];
const SAYINGS: [&str; 8] = [
    "I can take this one next week.",
    "Is this still wanted after the redesign?",
    "Reproduced it on a Pixel 6a.",
    "Added a screenshot to the thread.",
    "This blocks the export work.",
    "Looks good to me.",
    "Can we split this in two?",
    "Moved the notes to the description.",
];
const LINES: [&str; 6] = [
    "Reproduce it",
    "Write the fix",
    "Add a test",
    "Check it on a phone",
    "Ask a surveyor to try it",
    "Note it in the release",
];

/// The board written afresh, as it stands at `now` (unix seconds): the sixteen
/// cards above in their columns, then thousands more in the backlog and in done.
pub fn seed(now: i64) -> Seed {
    // Times on the board are whole minutes.
    let now = now - now.rem_euclid(60);
    let today = day_of(now);
    let people = PEOPLE
        .iter()
        .enumerate()
        .map(|(at, (name, email, job, role))| Person {
            id: at as i64 + 1,
            name: (*name).to_owned(),
            email: (*email).to_owned(),
            job: (*job).to_owned(),
            role: (*role).to_owned(),
        })
        .collect::<Vec<_>>();
    let mut seed = Seed {
        people,
        ..Seed::default()
    };
    let mut dice = Dice(0x5a17_3a25_1104);
    let mut counts = [0_usize; 5];
    // The cards a reader meets first.
    for (at, (column, title, assignee, labels, due, description, lines)) in CARDS.iter().enumerate()
    {
        let due = due.and_then(|days| {
            if days >= 0 {
                today.checked_add_days(Days::new(days.unsigned_abs()))
            } else {
                today.checked_sub_days(Days::new(days.unsigned_abs()))
            }
        });
        let created = now - (9 - (at as i64 % 7)) * 86_400 - at as i64 * 1_800;
        let lines: Vec<(String, bool)> = lines
            .iter()
            .map(|(text, done)| ((*text).to_owned(), *done))
            .collect();
        add(
            &mut seed,
            &mut counts,
            *column,
            title,
            *assignee,
            labels,
            due,
            description,
            &lines,
            created,
            now,
        );
    }
    for (card, person, ago, body) in COMMENTS {
        comment(&mut seed, card as i64 + 1, person, now - ago, body);
    }
    // The rest of the project.
    while seed.cards.len() < CARD_COUNT {
        let n = seed.cards.len();
        let column = if dice.below(10) < 6 {
            Column::Backlog
        } else {
            Column::Done
        };
        let title = format!(
            "{} {} {}",
            VERBS[dice.below(VERBS.len())],
            SUBJECTS[dice.below(SUBJECTS.len())],
            PLACES[dice.below(PLACES.len())]
        );
        let label = Label::ALL[dice.below(Label::ALL.len())].key();
        let assignee = dice.below(PEOPLE.len()) as i64 + 1;
        let created = now - 86_400 * (30 + dice.below(900) as i64) - 60 * dice.below(1_440) as i64;
        let due = (column == Column::Backlog && dice.below(5) == 0)
            .then(|| today.checked_add_days(Days::new(dice.below(90) as u64)))
            .flatten();
        let lines: Vec<(String, bool)> = (0..dice.below(5))
            .map(|line| {
                (
                    LINES[line].to_owned(),
                    column == Column::Done || dice.below(3) == 0,
                )
            })
            .collect();
        let description = format!(
            "Card {} of the project. Noted while planning {}.",
            n + 1,
            PROJECT
        );
        add(
            &mut seed,
            &mut counts,
            column,
            &title,
            assignee,
            label,
            due,
            &description,
            &lines,
            created,
            now,
        );
        let id = seed.cards.len() as i64;
        for said in 0..dice.below(5) {
            let at =
                (created + 3_600 * (said as i64 + 1) * (1 + dice.below(48) as i64)).min(now - 60);
            comment(
                &mut seed,
                id,
                dice.below(PEOPLE.len()) as i64 + 1,
                at,
                SAYINGS[dice.below(SAYINGS.len())],
            );
        }
    }
    seed
}

/// Adds a card at the end of `column`, its checklist, and how it got there.
#[allow(clippy::too_many_arguments)]
fn add(
    seed: &mut Seed,
    counts: &mut [usize; 5],
    column: Column,
    title: &str,
    assignee: i64,
    labels: &str,
    due: Option<NaiveDate>,
    description: &str,
    lines: &[(String, bool)],
    created: i64,
    now: i64,
) {
    let id = seed.cards.len() as i64 + 1;
    let slot = Column::ALL.iter().position(|c| *c == column).unwrap_or(0);
    let at = counts[slot];
    counts[slot] += 1;
    seed.cards.push(Card {
        id,
        list: column.key().to_owned(),
        rank: rank(at),
        title: title.to_owned(),
        description: description.to_owned(),
        assignee: Some(assignee),
        labels: labels.to_owned(),
        due,
        created_at: created,
        checks_done: lines.iter().filter(|(_, done)| *done).count() as i64,
        checks_total: lines.len() as i64,
        comments: 0,
    });
    for (line, (text, done)) in lines.iter().enumerate() {
        seed.checks.push(Check {
            id: seed.checks.len() as i64 + 1,
            card_id: id,
            position: line as i64,
            text: text.clone(),
            done: i64::from(*done),
        });
    }
    let mut history = |person: i64, at: i64, kind: &str, detail: String| {
        seed.activity.push(Activity {
            id: seed.activity.len() as i64 + 1,
            card_id: id,
            person_id: person,
            at,
            kind: kind.to_owned(),
            detail,
        });
    };
    history(1, created, "created", "Backlog".to_owned());
    // A card past the backlog came through each column on the way.
    let mut from = Column::Backlog;
    let mut when = created;
    for step in Column::ALL.iter().skip(1).take(slot) {
        when += 14 * 3600;
        let by = if *step == Column::Done { 1 } else { assignee };
        history(
            by,
            when.min(now - 600),
            "moved",
            format!("{} to {}", from.title(), step.title()),
        );
        from = *step;
    }
}

/// Adds a comment to card `card`, and says so in its history.
fn comment(seed: &mut Seed, card: i64, person: i64, at: i64, body: &str) {
    seed.comments.push(Comment {
        id: seed.comments.len() as i64 + 1,
        card_id: card,
        person_id: person,
        at,
        body: body.to_owned(),
    });
    if let Some(found) = seed
        .cards
        .get_mut(usize::try_from(card - 1).unwrap_or(usize::MAX))
    {
        found.comments += 1;
    }
    seed.activity.push(Activity {
        id: seed.activity.len() as i64 + 1,
        card_id: card,
        person_id: person,
        at,
        kind: "commented".to_owned(),
        detail: String::new(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_fills_every_column_within_its_limit() {
        let seed = seed(1_793_793_600);
        assert_eq!(seed.cards.len(), CARD_COUNT);
        for column in Column::ALL {
            let n = seed.cards.iter().filter(|c| c.list == column.key()).count();
            assert!(n > 0, "{column:?} has cards");
            if let Some(limit) = column.limit() {
                assert!(n <= limit as usize, "{column:?} holds {n}");
            }
        }
        // In progress starts full, so the limit shows.
        assert_eq!(seed.cards.iter().filter(|c| c.list == "doing").count(), 3);
        for person in &seed.people {
            assert!(person.email.ends_with("@example.com"));
        }
        // The counters on each card agree with its lines and comments.
        let card = &seed.cards[9];
        assert_eq!(card.comments, 3);
        assert_eq!((card.checks_done, card.checks_total), (2, 3));
        // The same board every time.
        assert_eq!(
            seed.cards[12_345].title,
            super::seed(1_793_793_600).cards[12_345].title
        );
    }

    #[test]
    fn labels_read_back_in_their_order() {
        assert_eq!(labels_of("speed,design"), [Label::Design, Label::Speed]);
        assert_eq!(labels_of(" bug , nonsense"), [Label::Bug]);
        assert!(labels_of("").is_empty());
    }
}
