//! [`Stamp`]: text written over the pages of a PDF, and [`Pages`], which
//! pages a stamp or an overlay goes on.

use serde::{Deserialize, Serialize};

use crate::DocumentsError;

/// Which pages a stamp or an overlay goes on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pages {
    /// Every page.
    #[default]
    All,
    /// The first page.
    First,
    /// The last page.
    Last,
    /// These pages, numbered from 1.
    Only(Vec<usize>),
}

impl Pages {
    /// Whether page `number` (from 1) of `total` is selected.
    pub fn includes(&self, number: usize, total: usize) -> bool {
        match self {
            Pages::All => true,
            Pages::First => number == 1,
            Pages::Last => number == total,
            Pages::Only(numbers) => numbers.contains(&number),
        }
    }
}

/// Where a stamp sits on the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    /// Centered on the page (with [`Stamp::text`]'s default rotation, a
    /// diagonal watermark).
    #[default]
    Center,
    /// Centered at the top.
    Top,
    /// Centered at the bottom.
    Bottom,
    /// The top-left corner.
    TopLeft,
    /// The top-right corner.
    TopRight,
    /// The bottom-left corner.
    BottomLeft,
    /// The bottom-right corner.
    BottomRight,
}

/// Text written over pages of a PDF: a watermark (`DRAFT`), a status
/// (`PAID`), a copy number. Set in Helvetica Bold, so the text must be
/// Latin-1; for other scripts, render the stamp as a PDF with the native
/// engine and use [`Pdf::overlay`](crate::Pdf::overlay).
///
/// ```
/// use navree_documents::{Pages, Position, Stamp};
///
/// // A gray diagonal watermark on every page (the defaults).
/// let draft = Stamp::text("DRAFT");
/// // A small red mark in the top-right corner of the first page.
/// let copy = Stamp::text("Copy 2 of 3")
///     .at(Position::TopRight)
///     .size(10.0)
///     .color(0.8, 0.1, 0.1)
///     .opacity(1.0)
///     .pages(Pages::First);
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stamp {
    /// The text.
    pub text: String,
    /// Where it sits (default [`Position::Center`]).
    pub position: Position,
    /// Font size in points (default 72 centered, 12 elsewhere).
    pub size: f32,
    /// Rotation in degrees, counter-clockwise (default 45 centered, 0
    /// elsewhere).
    pub rotation: f32,
    /// Fill color as red, green and blue from 0 to 1 (default gray).
    pub color: (f32, f32, f32),
    /// Opacity from 0 to 1 (default 0.25).
    pub opacity: f32,
    /// Distance from the page edges for edge positions, in points
    /// (default 36, half an inch).
    pub margin: f32,
    /// The pages (default every page).
    pub pages: Pages,
}

impl Stamp {
    /// A gray, 25% opaque, 72-point diagonal stamp centered on every page.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            position: Position::Center,
            size: 72.0,
            rotation: 45.0,
            color: (0.5, 0.5, 0.5),
            opacity: 0.25,
            margin: 36.0,
            pages: Pages::All,
        }
    }

    /// Moves the stamp. Moving it away from the centre also sets the size
    /// to 12 points and the rotation to 0, unless set afterwards.
    pub fn at(mut self, position: Position) -> Self {
        if self.position == Position::Center && position != Position::Center {
            self.size = 12.0;
            self.rotation = 0.0;
        }
        self.position = position;
        self
    }

    /// The font size in points.
    pub fn size(mut self, points: f32) -> Self {
        self.size = points;
        self
    }

    /// The rotation in degrees, counter-clockwise.
    pub fn rotate(mut self, degrees: f32) -> Self {
        self.rotation = degrees;
        self
    }

    /// The fill color (red, green, blue from 0 to 1).
    pub fn color(mut self, red: f32, green: f32, blue: f32) -> Self {
        self.color = (red, green, blue);
        self
    }

    /// The opacity from 0 to 1.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// The pages to stamp.
    pub fn pages(mut self, pages: Pages) -> Self {
        self.pages = pages;
        self
    }

    /// The text in WinAnsi, or an error naming the first character that
    /// Helvetica cannot show.
    pub(crate) fn encoded_text(&self) -> Result<Vec<u8>, DocumentsError> {
        self.text
            .chars()
            .map(|c| match c as u32 {
                code @ (0x20..=0x7e | 0xa0..=0xff) => Ok(code as u8),
                _ => Err(DocumentsError::Pdf(format!(
                    "a text stamp is set in Helvetica, which cannot show {c:?}; \
                     render the stamp with the native engine and use Pdf::overlay"
                ))),
            })
            .collect()
    }

    /// The width of the text in points.
    pub(crate) fn text_width(&self) -> f32 {
        let units: u32 = self.text.chars().map(|c| standard_width(c as u32)).sum();
        units as f32 * self.size / 1000.0
    }
}

/// Glyph widths of Helvetica Bold (per 1000 units) for printable ASCII,
/// from its font metrics; Latin-1 letters use 611, an average.
pub(crate) fn standard_width(code: u32) -> u32 {
    const ASCII: [u16; 95] = [
        278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278,
        278, // ' '..'/'
        556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0..9
        333, 333, 584, 584, 584, 611, 975, // ':'..'@'
        722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722,
        667, 611, 722, 667, 944, 667, 667, 611, // A..Z
        333, 278, 333, 584, 556, 333, // '['..'`'
        556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389,
        556, 333, 611, 556, 778, 556, 556, 500, // a..z
        389, 280, 389, 584, // '{'..'~'
    ];
    match code {
        0x20..=0x7e => u32::from(ASCII[(code - 0x20) as usize]),
        _ => 611,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_selection() {
        assert!(Pages::All.includes(3, 5));
        assert!(Pages::First.includes(1, 5) && !Pages::First.includes(2, 5));
        assert!(Pages::Last.includes(5, 5) && !Pages::Last.includes(4, 5));
        assert!(Pages::Only(vec![2, 4]).includes(4, 5));
        assert!(!Pages::Only(vec![2, 4]).includes(3, 5));
    }

    #[test]
    fn edge_positions_default_to_small_upright_text() {
        let stamp = Stamp::text("Copy").at(Position::TopRight);
        assert_eq!((stamp.size, stamp.rotation), (12.0, 0.0));
        let stamp = Stamp::text("Copy")
            .size(20.0)
            .at(Position::Bottom)
            .rotate(10.0);
        assert_eq!((stamp.size, stamp.rotation), (12.0, 10.0));
        let centered = Stamp::text("DRAFT");
        assert_eq!((centered.size, centered.rotation), (72.0, 45.0));
    }

    #[test]
    fn widths_follow_the_font_metrics() {
        // "HI" = 722 + 278 units at 10 pt.
        assert_eq!(Stamp::text("HI").size(10.0).text_width(), 10.0);
        assert!(Stamp::text("café").encoded_text().is_ok());
        let error = Stamp::text("مدفوع").encoded_text().unwrap_err();
        assert!(error.to_string().contains("Pdf::overlay"), "{error}");
    }
}
