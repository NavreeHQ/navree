import Foundation
import NavreeProtocol

/// How a chart's values are written on its axis: in the page's own way, as
/// its words for each value show it (`₹31,55,138.50`): the same sign before
/// or unit after, the same grouping (Indian lakhs and crores for rupees),
/// never the computer's `4.0E6`.
public struct ValueStyle: Sendable, Equatable {
    public var prefix: String
    public var suffix: String
    /// Groups of two after the first three, as India writes numbers.
    public var indian: Bool

    public init(prefix: String = "", suffix: String = "", indian: Bool = false) {
        self.prefix = prefix
        self.suffix = suffix
        self.indian = indian
    }
}

public enum ValueWords {
    /// The style of a chart's values: from the page's words for them, else
    /// the unit its value axis or series names.
    public static func style(_ chart: Navree.Chart) -> ValueStyle {
        let shown = chart.series.lazy.flatMap(\.shown).first { $0.contains(where: \.isNumber) }
        if let shown { return style(of: shown) }
        let unit = chart.y?.unit ?? chart.series.lazy.compactMap(\.unit).first
        guard let unit, !unit.isEmpty else { return ValueStyle() }
        return unit == "₹" || unit == "INR" ? ValueStyle(prefix: "₹", indian: true) : ValueStyle(suffix: unit == "%" ? "%" : " " + unit)
    }

    /// The style of one value as the page wrote it.
    public static func style(of words: String) -> ValueStyle {
        guard let first = words.firstIndex(where: \.isNumber), let last = words.lastIndex(where: \.isNumber) else {
            return ValueStyle()
        }
        var prefix = String(words[..<first])
        if prefix.hasSuffix("-") || prefix.hasSuffix("−") { prefix.removeLast() }
        let suffix = String(words[words.index(after: last)...])
        let number = String(words[first...last])
        let groups = number.split(separator: ".").first?.split(separator: ",").map(\.count) ?? []
        // Indian grouping: the last group of three, those before it of two.
        let indian = groups.count > 2 ? groups.dropLast().dropFirst().allSatisfy { $0 == 2 } && groups.last == 3
            : prefix.contains("₹")
        return ValueStyle(prefix: prefix, suffix: suffix, indian: indian)
    }

    /// A value on the axis, in the style: whole numbers grouped, a fraction
    /// to one place.
    public static func text(_ value: Double, _ style: ValueStyle) -> String {
        let negative = value < 0
        let magnitude = abs(value)
        let whole = magnitude.rounded(.towardZero)
        let fraction = magnitude - whole
        var digits = String(Int(whole))
        digits = style.indian ? indianGroups(digits) : westernGroups(digits)
        if fraction >= 0.05 {
            digits += "." + String(Int((fraction * 10).rounded()) % 10)
        }
        return (negative ? "−" : "") + style.prefix + digits + style.suffix
    }

    static func westernGroups(_ digits: String) -> String {
        var groups: [String] = []
        var rest = Substring(digits)
        while rest.count > 3 {
            groups.insert(String(rest.suffix(3)), at: 0)
            rest = rest.dropLast(3)
        }
        groups.insert(String(rest), at: 0)
        return groups.joined(separator: ",")
    }

    static func indianGroups(_ digits: String) -> String {
        guard digits.count > 3 else { return digits }
        var groups = [String(digits.suffix(3))]
        var rest = Substring(digits.dropLast(3))
        while rest.count > 2 {
            groups.insert(String(rest.suffix(2)), at: 0)
            rest = rest.dropLast(2)
        }
        if !rest.isEmpty { groups.insert(String(rest), at: 0) }
        return groups.joined(separator: ",")
    }
}
