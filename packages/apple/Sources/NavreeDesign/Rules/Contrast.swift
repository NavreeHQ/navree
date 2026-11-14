import Foundation

/// Words in a tone's colour that anyone can read. The system's green, orange
/// and yellow are bright enough for a symbol, a dot or a bar, but too pale
/// for words on a light ground; with Increase Contrast the system darkens
/// them, and Apple's apps write coloured words in those darker shades. Here
/// the shade is worked out: the least darkening that gives the words the
/// contrast Apple asks of text (4.5 to 1), on the grounds a screen has in the
/// light. In the dark the system's colours read as they are.
public enum ContrastRules {
    /// A colour as red, green and blue, 0 to 1, in sRGB.
    public struct RGB: Sendable, Equatable {
        public var red: Double
        public var green: Double
        public var blue: Double

        public init(_ red: Double, _ green: Double, _ blue: Double) {
            self.red = red
            self.green = green
            self.blue = blue
        }

        /// From `#RRGGBB`.
        public init?(hex: String) {
            let digits = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
            guard digits.count == 6, let value = UInt32(digits, radix: 16) else { return nil }
            self.init(Double((value >> 16) & 0xFF) / 255, Double((value >> 8) & 0xFF) / 255, Double(value & 0xFF) / 255)
        }
    }

    /// The contrast Apple asks of words (HIG, Color and Dark Mode: 4.5 to 1).
    public static let words = 4.5

    /// The lightest ground words stand on in the light (a card) and the
    /// darkest (the grouped ground around cards, the system's #F2F2F7).
    public static let lightGrounds = [RGB(1, 1, 1), RGB(hex: "#F2F2F7")!]

    /// A colour's relative luminance, as WCAG works it out.
    public static func luminance(_ colour: RGB) -> Double {
        func linear(_ channel: Double) -> Double {
            channel <= 0.04045 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(colour.red) + 0.7152 * linear(colour.green) + 0.0722 * linear(colour.blue)
    }

    /// How far apart two colours stand, 1 to 21.
    public static func contrast(_ a: RGB, _ b: RGB) -> Double {
        let (x, y) = (luminance(a), luminance(b))
        return (max(x, y) + 0.05) / (min(x, y) + 0.05)
    }

    /// `colour` taken `amount` of the way to black.
    public static func darkened(_ colour: RGB, by amount: Double) -> RGB {
        let keep = 1 - min(max(amount, 0), 1)
        return RGB(colour.red * keep, colour.green * keep, colour.blue * keep)
    }

    /// The least darkening, 0 to 1 in hundredths, that gives `colour` the
    /// contrast `target` against every ground in `grounds`.
    public static func shade(_ colour: RGB, grounds: [RGB] = lightGrounds, target: Double = words) -> Double {
        for step in 0...100 {
            let amount = Double(step) / 100
            let shaded = darkened(colour, by: amount)
            if grounds.allSatisfy({ contrast(shaded, $0) >= target }) { return amount }
        }
        return 1
    }

    /// The colour words take for a tone's colour: darkened as little as reads
    /// in the light, as it is in the dark.
    public static func words(_ colour: RGB, dark: Bool) -> RGB {
        dark ? colour : darkened(colour, by: shade(colour))
    }
}
