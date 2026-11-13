//! The parts a chart is made of, and the kinds of chart a health or fitness
//! screen draws: rings, a day on a clock, a bar of segments, a strip of cells,
//! a scale, a calendar of days, a matrix, a cloud of points.
//!
//! Every kind here says what its numbers and words *mean* and never how the
//! web draws them: numbers are numbers, the words a person reads beside them
//! (the value as shown, its unit, a label, a caption) come with them, and one
//! sentence says the whole chart to a reader who can't see it. A kind never
//! carries a size in points, a stroke width, a glow, the angle of a gradient
//! or an animation. Where a choice of size says something (a hero ring against
//! a small one) a closed word says it: [`Prominence`](crate::Prominence).
//!
//! Colour is data, by name. A part that has a colour of its own names a
//! [`Hue`] from one closed list, so each system draws its own purple, red,
//! green, orange and blue; [`Colour`] adds the second hue of a blend and the
//! theme's own colours as a fallback. A hue this build does not know is read as
//! [`Hue::Neutral`].
//!
//! The chart kinds live in [`node`](crate::node) as well: they are the parts a
//! [`Node`](crate::Node) holds.

use serde::{Deserialize, Serialize};

use crate::node::is_default;

/// A colour of data by name. A client draws each as its own system's colour of
/// that name, so a pillar that is violet is the system's violet on iPhone and
/// on Android, in light and in dark.
///
/// The status tones keep their names beside the plain hues: `danger` says
/// that red means trouble, `red` that it is only the colour of a heart rate. A
/// client may draw both with the same red, and a chart never uses a status
/// tone for a colour that carries no meaning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Hue {
    /// The quiet ink: what is not the subject of the chart, a track, the rest.
    /// Also what a name this build does not know is read as.
    #[default]
    Neutral,
    /// The app's accent.
    Accent,
    /// It worked, or it is well.
    Success,
    /// It needs attention.
    Warning,
    /// It failed, or it is dangerous.
    Danger,
    /// For your information.
    Info,
    /// Blue.
    Blue,
    /// Green.
    Green,
    /// Amber: yellow turning to orange.
    Amber,
    /// Orange.
    Orange,
    /// Red.
    Red,
    /// Rose: a red that leans to pink.
    Rose,
    /// Violet: the system's purple.
    Violet,
    /// Cyan: a blue that leans to green.
    Cyan,
    /// Slate: a grey that leans to blue.
    Slate,
}

/// The colour of one part of a chart: a hue by name, and where it helps, the
/// hue it blends into and the theme's own colours.
///
/// A client draws `hue` in its system's colour of that name. A part that names
/// no colour is drawn in the colours the client takes in turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Colour {
    /// The hue.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub hue: Hue,
    /// The hue it blends into, along the part: a ring that turns from cyan to
    /// violet is `hue` cyan and `to` violet. A blend with more stops than two
    /// is carried by its first and its last. A name this build does not know
    /// leaves the part one colour.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub to: Option<Hue>,
    /// The theme's own colour for `hue` on a light ground, as `#rrggbb`: a
    /// fallback for a client that does not know the hue. A client that does
    /// ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
    /// The theme's own colour for `hue` on a dark ground, as `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
}

impl Colour {
    /// The hue `hue`, with nothing beside it.
    pub const fn new(hue: Hue) -> Self {
        Self {
            hue,
            to: None,
            light: None,
            dark: None,
        }
    }

    /// A blend from `hue` into `to`.
    pub const fn blend(hue: Hue, to: Hue) -> Self {
        Self {
            hue,
            to: Some(to),
            light: None,
            dark: None,
        }
    }
}

impl Default for Colour {
    fn default() -> Self {
        Self::new(Hue::Neutral)
    }
}

impl From<Hue> for Colour {
    fn from(hue: Hue) -> Self {
        Self::new(hue)
    }
}

/// Which mark a hint wears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HintMark {
    /// A question mark: how to read what it is beside.
    #[default]
    Question,
    /// An "i": more about what it is beside.
    Info,
}

/// A note about the part it is on: how to read a chart, what a score is made
/// of. A client shows it when the person asks, as its system shows a hint (a
/// popover, a tooltip, a sheet), by a mark beside the part; never in the page.
///
/// A part that can have one carries it as `hint`: a chart, a figure, a section
/// beside its heading. It is not a kind of its own.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hint {
    /// What it is about: the name of the thing it explains, `the sleep score`.
    /// The mark is named after it for assistive technology: "About the sleep
    /// score".
    pub about: String,
    /// A heading for the note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The note.
    pub text: String,
    /// The mark that opens it.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub mark: HintMark,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_is_a_hue_by_name_and_writes_only_what_it_holds() {
        let plain = Colour::new(Hue::Violet);
        assert_eq!(serde_json::to_string(&plain).unwrap(), r#"{"hue":"violet"}"#);
        let blend = Colour::blend(Hue::Cyan, Hue::Violet);
        assert_eq!(serde_json::to_string(&blend).unwrap(), r#"{"hue":"cyan","to":"violet"}"#);
        let themed: Colour = serde_json::from_str(
            r##"{"hue":"rose","light":"#b3254f","dark":"#ff8aa8"}"##,
        )
        .unwrap();
        assert_eq!(themed.hue, Hue::Rose);
        assert_eq!(themed.light.as_deref(), Some("#b3254f"));
        assert_eq!(themed.dark.as_deref(), Some("#ff8aa8"));
        assert_eq!(Colour::from(Hue::Slate), Colour::new(Hue::Slate));
    }

    #[test]
    fn a_hue_this_build_does_not_know_is_neutral_and_a_blend_into_one_is_no_blend() {
        let unknown: Colour = serde_json::from_str(r#"{"hue":"chartreuse","to":"teal"}"#).unwrap();
        assert_eq!(unknown.hue, Hue::Neutral);
        assert_eq!(unknown.to, None);
        // A known blend is read as it was written; only a word is forgiven.
        let blend: Colour = serde_json::from_str(r#"{"hue":"green","to":"cyan"}"#).unwrap();
        assert_eq!((blend.hue, blend.to), (Hue::Green, Some(Hue::Cyan)));
        assert!(serde_json::from_str::<Colour>(r#"{"hue":7}"#).is_err());
        assert!(serde_json::from_str::<Colour>(r#"{}"#).is_err());
    }

    #[test]
    fn a_hint_has_what_it_is_about_and_its_words() {
        let hint: Hint = serde_json::from_str(
            r#"{"about":"the sleep score","title":"How it is made","text":"Duration, stages and regularity."}"#,
        )
        .unwrap();
        assert_eq!(hint.mark, HintMark::Question);
        assert_eq!(hint.title.as_deref(), Some("How it is made"));
        let info: Hint =
            serde_json::from_str(r#"{"about":"x","text":"y","mark":"info"}"#).unwrap();
        assert_eq!(info.mark, HintMark::Info);
        // A mark this build does not know is the usual one; a hint with no words is refused.
        let newer: Hint =
            serde_json::from_str(r#"{"about":"x","text":"y","mark":"star"}"#).unwrap();
        assert_eq!(newer.mark, HintMark::Question);
        assert!(serde_json::from_str::<Hint>(r#"{"about":"x"}"#).is_err());
        assert!(!serde_json::to_string(&Hint::default()).unwrap().contains("mark"));
    }
}
