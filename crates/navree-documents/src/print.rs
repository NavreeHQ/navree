//! [`PrintOptions`]: page settings for HTML printed by Chrome.

use serde::{Deserialize, Serialize};

/// A paper size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Paper {
    /// 210 × 297 mm.
    #[default]
    A4,
    /// 297 × 420 mm.
    A3,
    /// 148 × 210 mm.
    A5,
    /// 8.5 × 11 in.
    Letter,
    /// 8.5 × 14 in.
    Legal,
    /// Any size, in millimetres.
    Custom {
        /// The width in millimetres.
        width_mm: f64,
        /// The height in millimetres.
        height_mm: f64,
    },
}

impl Paper {
    /// Width and height in millimetres (portrait).
    pub fn size_mm(self) -> (f64, f64) {
        match self {
            Paper::A4 => (210.0, 297.0),
            Paper::A3 => (297.0, 420.0),
            Paper::A5 => (148.0, 210.0),
            Paper::Letter => (215.9, 279.4),
            Paper::Legal => (215.9, 355.6),
            Paper::Custom {
                width_mm,
                height_mm,
            } => (width_mm, height_mm),
        }
    }
}

/// Page margins in millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Margins {
    /// Top margin.
    pub top: f64,
    /// Right margin.
    pub right: f64,
    /// Bottom margin.
    pub bottom: f64,
    /// Left margin.
    pub left: f64,
}

impl Margins {
    /// The same margin on every side.
    pub fn all(mm: f64) -> Self {
        Self {
            top: mm,
            right: mm,
            bottom: mm,
            left: mm,
        }
    }
}

impl Default for Margins {
    /// 10 mm on every side.
    fn default() -> Self {
        Self::all(10.0)
    }
}

/// How Chrome prints a page: paper, orientation, margins, backgrounds,
/// header and footer. A CSS `@page { size: .. }` rule in the document wins
/// over `paper` while `prefer_css_page_size` is on.
///
/// ```toml
/// [documents.chrome.print]
/// paper = "a4"              # a4 | a3 | a5 | letter | legal
/// landscape = false
/// margin = { top = 10, right = 10, bottom = 10, left = 10 }  # millimetres
/// print_background = true
/// prefer_css_page_size = true
/// scale = 1.0
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrintOptions {
    /// The paper size (default A4).
    pub paper: Paper,
    /// Landscape orientation (default false).
    pub landscape: bool,
    /// The margins (default 10 mm on every side).
    pub margin: Margins,
    /// Print background colors and images (default true).
    pub print_background: bool,
    /// Let a CSS `@page { size }` rule set the paper (default true).
    pub prefer_css_page_size: bool,
    /// Scale of the page rendering, 0.1 to 2 (default 1).
    pub scale: f64,
    /// An HTML template for the header of every page. Chrome fills
    /// elements with the classes `date`, `title`, `url`, `pageNumber` and
    /// `totalPages`. Style it inline: page styles do not apply.
    pub header: Option<String>,
    /// An HTML template for the footer of every page (same classes).
    pub footer: Option<String>,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            paper: Paper::A4,
            landscape: false,
            margin: Margins::default(),
            print_background: true,
            prefer_css_page_size: true,
            scale: 1.0,
            header: None,
            footer: None,
        }
    }
}

impl PrintOptions {
    /// A4 portrait with the defaults.
    pub fn a4() -> Self {
        Self::default()
    }

    /// US Letter portrait with the defaults.
    pub fn letter() -> Self {
        Self {
            paper: Paper::Letter,
            ..Self::default()
        }
    }

    /// The same settings in landscape.
    pub fn landscape(mut self) -> Self {
        self.landscape = true;
        self
    }

    /// The same settings with `margin`.
    pub fn margin(mut self, margin: Margins) -> Self {
        self.margin = margin;
        self
    }

    /// The same settings with a footer template (`<span class="pageNumber">`).
    pub fn footer(mut self, html: impl Into<String>) -> Self {
        self.footer = Some(html.into());
        self
    }

    /// The same settings with a header template.
    pub fn header(mut self, html: impl Into<String>) -> Self {
        self.header = Some(html.into());
        self
    }

    /// The `Page.printToPDF` parameters (sizes in inches).
    #[cfg(feature = "chrome")]
    pub(crate) fn to_cdp(&self) -> serde_json::Value {
        const MM_PER_INCH: f64 = 25.4;
        let (width, height) = self.paper.size_mm();
        let header_footer = self.header.is_some() || self.footer.is_some();
        serde_json::json!({
            "landscape": self.landscape,
            "printBackground": self.print_background,
            "preferCSSPageSize": self.prefer_css_page_size,
            "scale": self.scale.clamp(0.1, 2.0),
            "paperWidth": width / MM_PER_INCH,
            "paperHeight": height / MM_PER_INCH,
            "marginTop": self.margin.top / MM_PER_INCH,
            "marginRight": self.margin.right / MM_PER_INCH,
            "marginBottom": self.margin.bottom / MM_PER_INCH,
            "marginLeft": self.margin.left / MM_PER_INCH,
            "displayHeaderFooter": header_footer,
            // An empty template hides Chrome's default header or footer.
            "headerTemplate": self.header.clone().unwrap_or_else(|| "<span></span>".into()),
            "footerTemplate": self.footer.clone().unwrap_or_else(|| "<span></span>".into()),
            "transferMode": "ReturnAsBase64",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_sizes_in_millimetres() {
        assert_eq!(Paper::A4.size_mm(), (210.0, 297.0));
        assert_eq!(Paper::Letter.size_mm(), (215.9, 279.4));
        assert_eq!(
            Paper::Custom {
                width_mm: 100.0,
                height_mm: 50.0
            }
            .size_mm(),
            (100.0, 50.0)
        );
    }

    #[test]
    fn options_deserialize_from_a_partial_section() {
        let options: PrintOptions = serde_json::from_value(serde_json::json!({
            "paper": "letter",
            "margin": {"top": 20},
        }))
        .unwrap();
        assert_eq!(options.paper, Paper::Letter);
        assert_eq!(options.margin.top, 20.0);
        assert_eq!(options.margin.left, 10.0);
        assert!(options.print_background);
        let custom: Paper = serde_json::from_value(
            serde_json::json!({"custom": {"width_mm": 90, "height_mm": 50}}),
        )
        .unwrap();
        assert_eq!(custom.size_mm(), (90.0, 50.0));
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn cdp_parameters_are_in_inches() {
        let params = PrintOptions::letter()
            .landscape()
            .margin(Margins::all(25.4))
            .footer("<span class=\"pageNumber\"></span>")
            .to_cdp();
        assert_eq!(params["paperWidth"], 8.5);
        assert_eq!(params["paperHeight"], 11.0);
        assert_eq!(params["marginTop"], 1.0);
        assert_eq!(params["landscape"], true);
        assert_eq!(params["displayHeaderFooter"], true);
        assert_eq!(params["headerTemplate"], "<span></span>");
        let plain = PrintOptions::a4().to_cdp();
        assert_eq!(plain["displayHeaderFooter"], false);
    }
}
