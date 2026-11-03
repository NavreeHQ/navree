//! Icons: every [Lucide](https://lucide.dev) icon as a constant, and your own.
//!
//! An icon is a `const`, so an app's binary holds the icons its code names and
//! no others. Three ways to name one:
//!
//! - `icons::MAIL_OPEN`: a constant. Your editor completes it, and a name that
//!   does not exist does not compile.
//! - `lucide!("mail-open")`: the name as lucide.dev writes it, checked while
//!   the app compiles. Renamed icons keep working under their old names.
//! - [`Icon::lookup`]: a name that arrives at run time. This is the one call
//!   that puts all of the icons into a binary.
//!
//! Your own SVG files become icons the same way, with `icon_file!` and
//! `icon_set!`: only shapes and safe attributes survive, and anything else
//! stops the build. See [`components::icon`](crate::components::icon) to draw
//! one.
//!
//! The icons are generated from a pinned Lucide release by `runtime/icons.mjs`;
//! Lucide is under the ISC licence, with the MIT licence for the icons derived
//! from Feather (see THIRD_PARTY.md).

use core::cmp::Ordering;

#[rustfmt::skip]
mod lucide;

pub use lucide::*;

/// An icon: its SVG shapes and the box they are drawn in. Compile-time data, so
/// no icon comes from a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Icon {
    shapes: &'static str,
    view_box: &'static str,
}

/// The box every Lucide icon is drawn in.
const LUCIDE_BOX: &str = "0 0 24 24";

impl Icon {
    /// A Lucide icon: shapes drawn in a 24 by 24 box. For the generated constants.
    pub(crate) const fn new(shapes: &'static str) -> Self {
        Self {
            shapes,
            view_box: LUCIDE_BOX,
        }
    }

    /// An icon the compiler has checked, for `icon_file!` and `icon_set!`. Do
    /// not call it with markup of your own: use those macros, which refuse
    /// anything but shapes.
    #[doc(hidden)]
    pub const fn __checked(shapes: &'static str, view_box: &'static str) -> Self {
        Self { shapes, view_box }
    }

    /// The SVG shapes, without an `<svg>` around them.
    pub const fn shapes(self) -> &'static str {
        self.shapes
    }

    /// The `viewBox` the shapes are drawn in, such as `0 0 24 24`.
    pub const fn view_box(self) -> &'static str {
        self.view_box
    }

    /// The icon with this Lucide name, such as `mail-open`, or an old name Lucide
    /// has renamed. `None` when there is none.
    ///
    /// This is a `const fn`, and in a `const` it is checked while the app
    /// compiles: [`lucide!`](crate::lucide) does that. Called at run time it puts
    /// every icon into the binary, since the name could be any of them. Reach
    /// for a constant or the macro when the name is in the code.
    pub const fn lookup(name: &str) -> Option<Icon> {
        if let Some(icon) = find(lucide::ALL, name) {
            return Some(icon);
        }
        // An old name: look up the name it became.
        let (mut low, mut high) = (0, lucide::RENAMED.len());
        while low < high {
            let mid = low + (high - low) / 2;
            let (old, current) = lucide::RENAMED[mid];
            match compare(name, old) {
                Ordering::Equal => return find(lucide::ALL, current),
                Ordering::Less => high = mid,
                Ordering::Greater => low = mid + 1,
            }
        }
        None
    }
}

/// Binary search of the sorted table.
const fn find(table: &[(&'static str, Icon)], name: &str) -> Option<Icon> {
    let (mut low, mut high) = (0, table.len());
    while low < high {
        let mid = low + (high - low) / 2;
        let (key, icon) = table[mid];
        match compare(name, key) {
            Ordering::Equal => return Some(icon),
            Ordering::Less => high = mid,
            Ordering::Greater => low = mid + 1,
        }
    }
    None
}

/// Bytewise order, in a `const fn`.
pub(crate) const fn compare(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let shortest = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0;
    while i < shortest {
        if a[i] != b[i] {
            return if a[i] < b[i] {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        i += 1;
    }
    if a.len() == b.len() {
        Ordering::Equal
    } else if a.len() < b.len() {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// A Lucide icon by the name lucide.dev gives it, checked while the app compiles.
///
/// ```
/// use navree_ui::{icons, lucide};
///
/// let mail = lucide!("mail-open");
/// assert_eq!(mail, icons::MAIL_OPEN);
/// ```
///
/// A name that is not an icon stops the build and names the icon it could not
/// find:
///
/// ```compile_fail
/// let _ = navree_ui::lucide!("mail-opne");
/// ```
///
/// Only the icon named goes into the binary.
#[macro_export]
macro_rules! lucide {
    ($name:literal) => {
        const {
            match $crate::icons::Icon::lookup($name) {
                Some(icon) => icon,
                None => panic!(concat!("no Lucide icon is named ", $name)),
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_so_the_search_finds_every_icon() {
        for pair in ALL.windows(2) {
            assert_eq!(compare(pair[0].0, pair[1].0), Ordering::Less, "{}", pair[1].0);
        }
        for pair in RENAMED.windows(2) {
            assert_eq!(compare(pair[0].0, pair[1].0), Ordering::Less, "{}", pair[1].0);
        }
        for (name, icon) in ALL {
            assert_eq!(Icon::lookup(name), Some(*icon), "{name}");
        }
    }

    #[test]
    fn there_are_more_than_two_thousand_icons() {
        assert!(ALL.len() > 2000, "{}", ALL.len());
    }

    #[test]
    fn a_name_that_is_not_an_icon_is_none() {
        for name in ["", "nope", "Mail-Open", "mail_open", "mail-open ", "mail-opne", "\0", "é"] {
            assert_eq!(Icon::lookup(name), None, "{name:?}");
        }
    }

    #[test]
    fn old_names_find_the_icon_they_became() {
        assert_eq!(Icon::lookup("alert-circle"), Some(CIRCLE_ALERT));
        assert_eq!(Icon::lookup("home"), Some(HOUSE));
        for (old, current) in RENAMED {
            assert_eq!(Icon::lookup(old), Icon::lookup(current), "{old}");
            assert!(Icon::lookup(current).is_some(), "{current} is not an icon");
        }
    }

    #[test]
    fn the_macro_checks_a_name_while_compiling() {
        const MAIL: Icon = lucide!("mail-open");
        assert_eq!(MAIL, MAIL_OPEN);
        assert_eq!(lucide!("alert-circle"), CIRCLE_ALERT);
    }

    #[test]
    fn every_icon_is_shapes_in_a_24_box() {
        for (name, icon) in ALL {
            assert_eq!(icon.view_box(), "0 0 24 24", "{name}");
            assert!(icon.shapes().starts_with('<'), "{name}");
            assert!(!icon.shapes().is_empty(), "{name}");
        }
    }
}
