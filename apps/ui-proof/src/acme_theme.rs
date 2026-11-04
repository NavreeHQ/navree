//! The "Acme" theme: the theme builder's Rust export, used as it comes. A
//! test in navree-ui writes it and keeps it current.

use navree::ui::theme::{Density, Font, Palette, Radius, Rgb, Theme, ThemeId, ThemeSpec};

/// The "Acme" theme.
pub fn acme_theme() -> Theme {
    let rgb = Rgb::hex;
    let spec = ThemeSpec {
        dark: Palette::from_array([
            rgb(0x050507), // ink-0
            rgb(0x0d0c10), // ink-1
            rgb(0x15151a), // ink-2
            rgb(0x1d1d22), // ink-3
            rgb(0x212127), // line
            rgb(0x313137), // line-2
            rgb(0x434349), // line-3
            rgb(0xededf3), // fg
            rgb(0xd5d5dc), // fg-1
            rgb(0xb1b1b8), // fg-2
            rgb(0x929299), // fg-3
            rgb(0x7c5cff), // accent
            rgb(0x000000), // accent-ink
            rgb(0x856eff), // accent-text
            rgb(0x7c5cff), // ring
            rgb(0x3dff8f), // success
            rgb(0xffbf5e), // amber
            rgb(0xff6b6b), // danger
            rgb(0x6cb6ff), // info
            rgb(0xb79cff), // violet
            rgb(0x4fd6e6), // cyan
            rgb(0xff8aa8), // rose
            rgb(0x64646a), // bar
            rgb(0x3987e5), // chart-1
            rgb(0xd95926), // chart-2
            rgb(0x199e70), // chart-3
            rgb(0xc98500), // chart-4
            rgb(0xd55181), // chart-5
            rgb(0x008300), // chart-6
            rgb(0x9085e9), // chart-7
            rgb(0xe66767), // chart-8
        ]),
        light: Palette::from_array([
            rgb(0xffffff), // ink-0
            rgb(0xf7f7fb), // ink-1
            rgb(0xf0f0f4), // ink-2
            rgb(0xe7e7ea), // ink-3
            rgb(0xe1e1e6), // line
            rgb(0xccccd1), // line-2
            rgb(0xb3b3ba), // line-3
            rgb(0x0d0c10), // fg
            rgb(0x212127), // fg-1
            rgb(0x46464c), // fg-2
            rgb(0x5d5d63), // fg-3
            rgb(0x7c5cff), // accent
            rgb(0x000000), // accent-ink
            rgb(0x684fd4), // accent-text
            rgb(0x7c5cff), // ring
            rgb(0x096835), // success
            rgb(0x7e5200), // amber
            rgb(0x9e2119), // danger
            rgb(0x0b5cad), // info
            rgb(0x6b46c1), // violet
            rgb(0x0a6b78), // cyan
            rgb(0xb3254f), // rose
            rgb(0x929299), // bar
            rgb(0x2a78d6), // chart-1
            rgb(0xdc5a24), // chart-2
            rgb(0x029566), // chart-3
            rgb(0xb07702), // chart-4
            rgb(0xca6089), // chart-5
            rgb(0x008300), // chart-6
            rgb(0x6250d6), // chart-7
            rgb(0xe34948), // chart-8
        ]),
        radius: Radius::ROUND,
        density: Density::Default,
        font: Font::System,
    };
    Theme::custom(ThemeId::parse("acme").expect("a theme id"), "Acme", spec)
        .expect("every text pair reaches WCAG AA")
}
