//! [`Coverage`]: which pages a native client gets as screens, which fall back
//! to its web view and why, and what each answer weighs beside its HTML.

use std::fmt;

/// One page's line of the report.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    route: String,
    native: bool,
    why: Vec<String>,
    dropped: Vec<String>,
    html: usize,
    envelope: usize,
}

/// A report of native coverage, a line a page.
///
/// ```
/// use navree_ui::native::{Coverage, TestNative};
/// use navree_ui::{Text, View, el};
///
/// let mut coverage = Coverage::new();
/// let orders = View::new("Orders").body(Text::new("No orders yet"));
/// coverage.add("/orders", orders.to_html().len(), &TestNative::of(&orders));
/// let board = View::new("Board").body(el("canvas"));
/// coverage.add("/board", board.to_html().len(), &TestNative::of(&board));
/// assert_eq!(coverage.native(), 1);
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
        self.lines.push(Line {
            native: page.is_native(),
            why: page.why(),
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
            native: answered.native,
            why: answered.why.clone(),
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
        self.lines.iter().filter(|line| line.native).count()
    }

    /// What kept pages from being native, each thing with the number of
    /// pages it kept, the most common first.
    pub fn reasons(&self) -> Vec<(String, usize)> {
        let mut reasons: Vec<(String, usize)> = Vec::new();
        for line in &self.lines {
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
            "| Route | Answer | Why not native | HTML bytes | Envelope bytes | Left out |"
        )?;
        writeln!(f, "|---|---|---|---:|---:|---|")?;
        for line in &self.lines {
            writeln!(
                f,
                "| {} | {} | {} | {} | {} | {} |",
                line.route,
                if line.native { "native" } else { "web" },
                line.why.join(", "),
                line.html,
                line.envelope,
                line.dropped.join(", "),
            )?;
        }
        write!(f, "{} of {} native", self.native(), self.pages())
    }
}
