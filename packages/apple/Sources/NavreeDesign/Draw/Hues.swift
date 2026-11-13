import NavreeProtocol
import SwiftUI

extension SystemHue {
    /// The system's own colour.
    public var color: Color {
        switch self {
        case .accent: .accentColor
        case .secondary: .secondary
        case .red: .red
        case .orange: .orange
        case .yellow: .yellow
        case .green: .green
        case .mint: .mint
        case .teal: .teal
        case .cyan: .cyan
        case .blue: .blue
        case .indigo: .indigo
        case .purple: .purple
        case .pink: .pink
        case .brown: .brown
        case .gray: .gray
        }
    }
}

/// The colours of a part, by the names the page sends.
enum Hues {
    /// A hue as the system draws it.
    static func color(_ hue: Navree.Hue) -> Color {
        HueRules.system(hue).color
    }

    /// A part's colour, or `fallback` when the page names none. A colour this
    /// client read as `neutral` but that came with the theme's own value is
    /// drawn in that value (a hue newer than this client).
    static func color(_ colour: Navree.Colour?, fallback: Color = .accentColor, scheme: ColorScheme = .light) -> Color {
        guard let colour else { return fallback }
        if let rgb = HueRules.fallback(colour, dark: scheme == .dark) {
            return Color(red: rgb.red, green: rgb.green, blue: rgb.blue)
        }
        return color(colour.hue)
    }

    /// The colour a blend ends in: `to`, or the colour itself.
    static func end(_ colour: Navree.Colour?, fallback: Color = .accentColor, scheme: ColorScheme = .light) -> Color {
        guard let to = colour?.to else { return color(colour, fallback: fallback, scheme: scheme) }
        return color(to)
    }

    /// A part's colour where it colours words: darkened on a light ground
    /// where the hue alone is too pale to read.
    static func text(_ colour: Navree.Colour?, fallback: Color = .accentColor, scheme: ColorScheme) -> Color {
        let base = color(colour, fallback: fallback, scheme: scheme)
        guard let colour, HueRules.fallback(colour, dark: scheme == .dark) == nil else { return base }
        let shade = HueRules.textShade(HueRules.system(colour.hue), dark: scheme == .dark)
        return shade > 0 ? base.mix(with: .black, by: shade) : base
    }

    /// The colour of a tone.
    static func color(_ tone: Navree.Tone?) -> Color {
        HueRules.system(tone).color
    }
}

/// A track a mark runs along: the mark's own hue, faint, as the system's
/// activity rings and gauges draw theirs.
extension Color {
    func track(_ scheme: ColorScheme) -> Color {
        opacity(scheme == .dark ? 0.26 : 0.18)
    }
}
