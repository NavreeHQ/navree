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
    if screen.title.trim().is_empty() {
        missing.push("the screen has no title".to_owned());
    }
    for action in &screen.actions {
        if let N::Button(button) = action
            && button.role == n::Role::None
        {
            missing.push(format!("the action “{}” has no role", button.label));
        }
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
                .unwrap_or("a");
            missing.extend(table_gaps(name, table));
        }
    }
    let grade = if missing.is_empty() {
        Grade::Idiom
    } else {
        Grade::Layout
    };
    (grade, missing)
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
        gaps.push(format!("the {name} table has no title column"));
    }
    if table.rows.iter().all(|row| row.action.is_none()) {
        gaps.push(format!("the {name} table's rows open nothing"));
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
        write!(
            f,
            "{} of {} native: {} idiom, {} layout, {} web",
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
        assert!(coverage.to_string().ends_with("1 of 2 native: 1 idiom, 0 layout, 1 web"));
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
            navree_native::Client::new(navree_native::Platform::Ios, navree_native::FormFactor::Phone),
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
