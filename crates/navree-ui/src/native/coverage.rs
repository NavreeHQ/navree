//! [`Coverage`]: which pages a native client gets as screens that belong to
//! its system, which as native parts in a web-shaped layout, which fall back
//! to its web view, and why, with what each answer weighs beside its HTML.

use std::fmt;

use navree_native::Screen;
use navree_native::node::{self as n, Node as N};

/// How much a page belongs to the system a native client runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Grade {
    /// Its parts say what they are, so the client lays it out as its own
    /// system does: a table's records as rows with a title, rows that open
    /// something, actions with a role.
    Idiom,
    /// Written for native clients, not read from a page: its author laid it
    /// out with the system's own styles. What a designed screen says that
    /// keeps it from belonging is listed beside it, by [`designed`].
    Designed,
    /// Native parts, laid out as the web page is: it works, and it looks
    /// like a web page drawn with the system's controls.
    Layout,
    /// Shown in the client's web view.
    Web,
}

impl Grade {
    /// The grade's word.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idiom => "idiom",
            Self::Designed => "designed",
            Self::Layout => "layout",
            Self::Web => "web",
        }
    }
}

impl fmt::Display for Grade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A screen's grade, and in words what keeps it from being idiom.
///
/// ```
/// use navree_ui::native::{Grade, TestNative, graded};
/// use navree_ui::{Button, Role, View};
///
/// let page = TestNative::of(&View::new("Orders").body(Button::new("Export").href("/export")));
/// assert_eq!(graded(page.screen()).0, Grade::Idiom);
/// ```
pub fn graded(screen: &Screen) -> (Grade, Vec<String>) {
    let mut missing = Vec::new();
    // The answer to a search is the options found, not a page: it has no name to give.
    if screen.title.trim().is_empty() && !answers_a_search(&screen.body) {
        missing.push("the screen has no title".to_owned());
    }
    for action in &screen.actions {
        if let N::Button(button) = action
            && button.role == n::Role::None
        {
            missing.push(format!("the action “{}” has no role", button.label));
        }
    }
    // A screen is for one thing: two actions that both claim to be it are one too many.
    let main: Vec<&str> = screen
        .actions
        .iter()
        .filter_map(|action| match action {
            N::Button(button) if matches!(button.role, n::Role::Primary | n::Role::Create) => {
                Some(button.label.as_str())
            }
            _ => None,
        })
        .collect();
    if main.len() > 1 {
        missing.push(format!(
            "the screen has {} main actions: {}",
            match main.len() {
                2 => "two".to_owned(),
                3 => "three".to_owned(),
                count => count.to_string(),
            },
            sentence(&main)
        ));
    }
    let mut stack: Vec<&N> = vec![&screen.body];
    // The id of the region a table stands in names the table.
    let mut named: Vec<(&N, Option<&str>)> = Vec::new();
    while let Some(node) = stack.pop() {
        if let N::Region(region) = node {
            for child in &region.children {
                named.push((child, Some(region.id.as_str())));
            }
        }
        for child in node.children() {
            stack.push(child);
        }
        if let N::Table(table) = node {
            let name = named
                .iter()
                .find(|(seen, _)| std::ptr::eq(*seen, node))
                .and_then(|(_, id)| *id)
                .or(table.caption.as_deref())
                .map_or_else(|| "a table".to_owned(), |name| format!("the {name} table"));
            missing.extend(table_gaps(&name, table));
        }
    }
    let grade = if missing.is_empty() {
        Grade::Idiom
    } else {
        Grade::Layout
    };
    (grade, missing)
}

/// What a screen written for native clients says that keeps it from
/// belonging to its system, in words: nothing, when it is sound.
///
/// A designed screen is held to what every screen is held to (a title, a role
/// on each action, one main action, a table whose rows are records) and to
/// what a composition may say: a card that is tapped has a name; a card does
/// not stand in a card, nor a carousel in a carousel; one figure is set in
/// the display style; and a part with an id stands in one room of an adaptive
/// part only, since an answer that names an id finds one node.
///
/// ```
/// use navree_ui::native::{designed, ui::*};
///
/// let screen = Screen::titled("Shelves").body(
///     Card::new().opens("", Action::navigate("/inventory/7")).parts(Card::new()),
/// );
/// assert_eq!(
///     designed(&screen),
///     ["a card that is tapped has no name", "a card stands inside a card"]
/// );
/// ```
pub fn designed(screen: &Screen) -> Vec<String> {
    let mut gaps = graded(screen).1;
    let mut say = |gap: String| {
        if !gaps.contains(&gap) {
            gaps.push(gap);
        }
    };
    let mut display = 0;
    // Each node with whether a card, and whether a carousel, stands round it.
    let mut stack: Vec<(&N, bool, bool)> = std::iter::once(&screen.body)
        .chain(&screen.actions)
        .map(|node| (node, false, false))
        .collect();
    while let Some((node, in_card, in_carousel)) = stack.pop() {
        let (mut card, mut carousel) = (in_card, in_carousel);
        match node {
            N::Card(part) => {
                if in_card {
                    say("a card stands inside a card".to_owned());
                }
                if part.action.is_some()
                    && part
                        .label
                        .as_deref()
                        .is_none_or(|label| label.trim().is_empty())
                {
                    say("a card that is tapped has no name".to_owned());
                }
                card = true;
            }
            N::Carousel(_) => {
                if in_carousel {
                    say("a carousel stands inside a carousel".to_owned());
                }
                carousel = true;
            }
            N::Text(text) if text.style == n::TextStyle::Display => display += 1,
            N::Adaptive(part) => {
                let ids = |nodes: &[N]| {
                    let mut ids: Vec<String> = Vec::new();
                    let mut inside: Vec<&N> = nodes.iter().collect();
                    while let Some(node) = inside.pop() {
                        ids.extend(node.id().map(str::to_owned));
                        inside.extend(node.children());
                    }
                    ids
                };
                let wide = ids(&part.wide);
                for id in ids(&part.compact) {
                    if wide.contains(&id) {
                        say(format!(
                            "the part “{id}” stands in both rooms of an adaptive part"
                        ));
                    }
                }
            }
            _ => {}
        }
        for child in node.children() {
            stack.push((child, card, carousel));
        }
    }
    if display > 1 {
        say(format!(
            "{display} figures are set in the display style: a screen has one"
        ));
    }
    gaps
}

/// The labels as a sentence names them: “A” and “B”, or “A”, “B” and “C”.
fn sentence(labels: &[&str]) -> String {
    let quoted: Vec<String> = labels.iter().map(|label| format!("“{label}”")).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((only, [])) => only.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// Whether `body` is what a searched choice is answered with: a choice, and
/// perhaps the way to the next options.
fn answers_a_search(body: &N) -> bool {
    use navree_native::input::Input;
    let is_choice = |node: &N| matches!(node, N::Field(field) if matches!(field.input, Input::Choice(_) | Input::Choices(_)));
    match body {
        N::Stack(stack) => match stack.children.as_slice() {
            [field] => is_choice(field),
            [field, N::Link(_)] => is_choice(field),
            _ => false,
        },
        other => is_choice(other),
    }
}

/// What a table lacks to be drawn as a system's list of records.
fn table_gaps(name: &str, table: &n::Table) -> Vec<String> {
    let mut gaps = Vec::new();
    if table.rows.is_empty() {
        return gaps;
    }
    let titled = table
        .columns
        .iter()
        .any(|column| column.role == Some(n::ColumnRole::Title));
    if !titled {
        gaps.push(format!("{name} has no title column"));
    }
    if table.rows.iter().all(|row| row.action.is_none()) {
        gaps.push(format!("{name}'s rows open nothing"));
    }
    gaps
}

/// One page's line of the report.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    route: String,
    grade: Grade,
    why: Vec<String>,
    dropped: Vec<String>,
    html: usize,
    envelope: usize,
}

/// A report of native coverage, a line a page, each graded.
///
/// ```
/// use navree_ui::native::{Coverage, Grade, TestNative};
/// use navree_ui::{Text, View, el};
///
/// let mut coverage = Coverage::new();
/// let orders = View::new("Orders").body(Text::new("No orders yet"));
/// coverage.add("/orders", orders.to_html().len(), &TestNative::of(&orders));
/// let board = View::new("Board").body(el("canvas"));
/// coverage.add("/board", board.to_html().len(), &TestNative::of(&board));
/// assert_eq!(coverage.native(), 1);
/// assert_eq!(coverage.graded(Grade::Idiom), 1);
/// assert_eq!(coverage.pages(), 2);
/// assert!(coverage.to_string().contains("| /board | web | tag:canvas |"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    lines: Vec<Line>,
}

impl Coverage {
    /// An empty report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the page at `route`, whose HTML is `html` bytes long.
    pub fn add(&mut self, route: impl Into<String>, html: usize, page: &super::TestNative) {
        let route = route.into();
        let envelope = serde_json::to_string(&page.envelope_at(&route))
            .map(|json| json.len())
            .unwrap_or(0);
        let (grade, why) = if page.is_native() {
            graded(page.screen())
        } else {
            (Grade::Web, page.why())
        };
        self.lines.push(Line {
            grade,
            why,
            dropped: page.page().dropped.clone(),
            html,
            envelope,
            route,
        });
    }

    /// Adds a page as a running app answered it: what the reply layer kept
    /// about the answer, and the two answers' sizes in bytes.
    pub fn add_answer(
        &mut self,
        route: impl Into<String>,
        html: usize,
        envelope: usize,
        answered: &super::Answered,
    ) {
        self.lines.push(Line {
            route: route.into(),
            grade: answered.grade,
            why: if answered.native {
                answered.missing.clone()
            } else {
                answered.why.clone()
            },
            dropped: answered.dropped.clone(),
            html,
            envelope,
        });
    }

    /// How many pages are in the report.
    pub fn pages(&self) -> usize {
        self.lines.len()
    }

    /// How many of them a native client gets as a screen.
    pub fn native(&self) -> usize {
        self.lines
            .iter()
            .filter(|line| line.grade != Grade::Web)
            .count()
    }

    /// How many of them have `grade`.
    pub fn graded(&self, grade: Grade) -> usize {
        self.lines.iter().filter(|line| line.grade == grade).count()
    }

    /// The grade of the page at `route`, when the report has it.
    pub fn grade_of(&self, route: &str) -> Option<Grade> {
        self.lines
            .iter()
            .find(|line| line.route == route)
            .map(|line| line.grade)
    }

    /// What kept pages from being native, each thing with the number of
    /// pages it kept, the most common first.
    pub fn reasons(&self) -> Vec<(String, usize)> {
        let mut reasons: Vec<(String, usize)> = Vec::new();
        for line in self.lines.iter().filter(|line| line.grade == Grade::Web) {
            for why in &line.why {
                match reasons.iter_mut().find(|(have, _)| have == why) {
                    Some((_, count)) => *count += 1,
                    None => reasons.push((why.clone(), 1)),
                }
            }
        }
        reasons.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        reasons
    }
}

impl fmt::Display for Coverage {
    /// The report as a Markdown table.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "| Route | Grade | Why not idiom | HTML bytes | Envelope bytes | Left out |"
        )?;
        writeln!(f, "|---|---|---|---:|---:|---|")?;
        for line in &self.lines {
            writeln!(
                f,
                "| {} | {} | {} | {} | {} | {} |",
                line.route,
                line.grade,
                line.why.join(", "),
                line.html,
                line.envelope,
                line.dropped.join(", "),
            )?;
        }
        // Screens written for native clients are counted where an app has some.
        let designed = match self.graded(Grade::Designed) {
            0 => String::new(),
            count => format!("{count} designed, "),
        };
        write!(
            f,
            "{} of {} native: {} idiom, {designed}{} layout, {} web",
            self.native(),
            self.pages(),
            self.graded(Grade::Idiom),
            self.graded(Grade::Layout),
            self.graded(Grade::Web),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::TestNative;
    use crate::{Button, Role, Text, View, el};

    #[test]
    fn a_page_is_graded_idiom_layout_or_web() {
        let idiom = TestNative::of(&View::new("Orders").body(Text::new("No orders yet")));
        assert_eq!(graded(idiom.screen()), (Grade::Idiom, Vec::new()));
        let mut coverage = Coverage::new();
        coverage.add("/orders", 100, &idiom);
        coverage.add(
            "/board",
            100,
            &TestNative::of(&View::new("Board").body(el("canvas"))),
        );
        assert_eq!(coverage.grade_of("/orders"), Some(Grade::Idiom));
        assert_eq!(coverage.grade_of("/board"), Some(Grade::Web));
        assert!(
            coverage
                .to_string()
                .ends_with("1 of 2 native: 1 idiom, 0 layout, 1 web")
        );
    }

    #[test]
    fn a_designed_screen_is_held_to_what_a_composition_may_say() {
        use navree_native::ui::{
            Action, Adaptive, Button, Card, Carousel, Role, Screen, Stack, Text, TextStyle,
        };
        let sound = Screen::titled("Shelves")
            .action(
                Button::new("Inventory")
                    .role(Role::Secondary)
                    .to("/inventory"),
            )
            .body(
                Stack::down().parts((
                    Text::new("6").style(TextStyle::Display),
                    Card::new()
                        .id("shelf-7")
                        .opens("Brass lota, out of stock", Action::navigate("/inventory/7"))
                        .parts(Text::new("Brass lota")),
                    Carousel::new().parts(Card::new().parts(Text::new("Phulkari dupatta"))),
                    Adaptive::new()
                        .compact(Text::new("3 shelves are empty").id("empty"))
                        .wide(Text::new("3 shelves are empty and 3 are running low")),
                )),
            );
        assert_eq!(designed(&sound), Vec::<String>::new());
        let unsound = Screen::titled("").action(Button::new("Export")).body(
            Stack::down().parts((
                Text::new("6").style(TextStyle::Display),
                Text::new("12").style(TextStyle::Display),
                Card::new()
                    .opens(" ", Action::navigate("/inventory/7"))
                    .parts(Card::new().parts(Carousel::new().parts(Carousel::new()))),
                Adaptive::new()
                    .compact(Text::new("3 shelves are empty").id("empty"))
                    .wide(Stack::down().parts(Text::new("3 are empty").id("empty"))),
            )),
        );
        let mut gaps = designed(&unsound);
        gaps.sort();
        assert_eq!(
            gaps,
            [
                "2 figures are set in the display style: a screen has one",
                "a card stands inside a card",
                "a card that is tapped has no name",
                "a carousel stands inside a carousel",
                "the action “Export” has no role",
                "the part “empty” stands in both rooms of an adaptive part",
                "the screen has no title",
            ]
        );
    }

    #[test]
    fn designed_screens_are_counted_where_there_are_some() {
        let mut coverage = Coverage::new();
        let mut answered = crate::native::Answered::parts(Vec::new());
        answered.grade = Grade::Designed;
        coverage.add_answer("/", 100, 80, &answered);
        coverage.add_answer(
            "/orders",
            100,
            80,
            &crate::native::Answered::parts(Vec::new()),
        );
        assert_eq!(
            (coverage.native(), coverage.graded(Grade::Designed)),
            (2, 1)
        );
        assert!(
            coverage
                .to_string()
                .ends_with("2 of 2 native: 1 idiom, 1 designed, 0 layout, 0 web"),
            "{coverage}"
        );
    }

    #[test]
    fn a_screen_with_more_than_one_main_action_is_a_layout() {
        let action = |label: &str, role| {
            N::Button(n::Button {
                label: label.to_owned(),
                role,
                ..n::Button::default()
            })
        };
        let screen = |actions: Vec<N>| Screen {
            actions,
            ..Screen::new("Order CS-20481", N::text("x"))
        };
        // One main action and the others that are not: as it should be.
        let one = screen(vec![
            action("Mark as packed", n::Role::Primary),
            action("Print label", n::Role::Secondary),
        ]);
        assert_eq!(graded(&one), (Grade::Idiom, Vec::new()));
        let two = screen(vec![
            action("Mark as packed", n::Role::Primary),
            action("Print label", n::Role::Create),
        ]);
        assert_eq!(
            graded(&two),
            (
                Grade::Layout,
                vec![
                    "the screen has two main actions: “Mark as packed” and “Print label”"
                        .to_owned()
                ]
            )
        );
        let three = screen(vec![
            action("A", n::Role::Primary),
            action("B", n::Role::Primary),
            action("C", n::Role::Primary),
        ]);
        assert_eq!(
            graded(&three).1,
            ["the screen has three main actions: “A”, “B” and “C”"]
        );
    }

    #[test]
    fn the_options_a_search_found_are_not_a_page_that_lacks_a_title() {
        use navree_native::input as i;
        let field = N::Field(n::Field {
            input: i::Input::Choice(i::ChoiceInput::default()),
            ..n::Field::default()
        });
        assert_eq!(graded(&Screen::new("", field.clone())).0, Grade::Idiom);
        // A screen of anything else with no name still lacks one.
        assert_eq!(graded(&Screen::new("", N::text("x"))).0, Grade::Layout);
        let with_link = N::Stack(n::Stack {
            children: vec![
                field,
                N::Link(n::Link {
                    id: None,
                    label: "Show more".into(),
                    action: navree_native::Action::Refresh,
                    children: Vec::new(),
                }),
            ],
            ..n::Stack::default()
        });
        assert_eq!(graded(&Screen::new("", with_link)).0, Grade::Idiom);
    }

    #[test]
    fn what_is_missing_is_said_in_words() {
        let screen = Screen {
            actions: vec![N::Button(n::Button {
                label: "Export".to_owned(),
                ..n::Button::default()
            })],
            ..Screen::new(
                "Orders",
                N::Region(n::Region {
                    id: "orders".to_owned(),
                    src: "/orders".to_owned(),
                    lazy: false,
                    live: None,
                    children: vec![N::Table(n::Table {
                        columns: vec![n::Column {
                            label: "Order".to_owned(),
                            ..n::Column::default()
                        }],
                        rows: vec![n::TableRow {
                            cells: vec![N::text("CS-20481")],
                            ..n::TableRow::default()
                        }],
                        ..n::Table::default()
                    })],
                }),
            )
        };
        assert_eq!(
            graded(&screen),
            (
                Grade::Layout,
                vec![
                    "the action “Export” has no role".to_owned(),
                    "the orders table has no title column".to_owned(),
                    "the orders table's rows open nothing".to_owned(),
                ]
            )
        );
        // Saying what the action is clears its line.
        let page = TestNative::build(
            navree_native::Client::new(
                navree_native::Platform::Ios,
                navree_native::FormFactor::Phone,
            ),
            || {
                crate::Shell::new("Store")
                    .page("Orders", "/orders")
                    .actions(Button::new("Export").href("/export").role(Role::Secondary))
                    .to_view()
            },
        );
        assert_eq!(graded(page.screen()).0, Grade::Idiom);
    }
}
