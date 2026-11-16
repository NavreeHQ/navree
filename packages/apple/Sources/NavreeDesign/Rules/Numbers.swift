import Foundation

/// Numbers a server sends, made safe before they reach a control, a chart or
/// a whole number. A page may say anything JSON can write: a value of
/// 9.3 × 10¹⁸, an axis whose top is under its foot, a slider from `NaN` to
/// `inf` with a step of nothing. Swift stops the app on a range whose low end
/// is over its high end and on a whole number made from what no `Int` holds,
/// so none of those is ever built from a page's numbers as they came.
public enum SafeNumbers {
    /// The magnitude past which a number is not drawn as itself: scales,
    /// sliders and written-out digits stop here.
    public static let most = 1e15

    /// `value` as a whole number: what is no number is 0, and a number past
    /// what an `Int` holds is the nearest one it holds.
    public static func int(_ value: Double) -> Int {
        guard !value.isNaN else { return 0 }
        // 2⁶³, the first number past the largest `Int`, is a `Double` exactly.
        let limit = 9_223_372_036_854_775_808.0
        if value >= limit { return Int.max }
        if value <= -limit { return Int.min }
        return Int(value)
    }

    /// `value` when it is a number within ``most``; `other` for what is no
    /// number, and the nearest end for one past it.
    public static func finite(_ value: Double, else other: Double = 0) -> Double {
        guard !value.isNaN else { return other }
        return Swift.min(Swift.max(value, -most), most)
    }

    /// A range a scale, a gauge or a slider can be given: both ends numbers
    /// within ``most``, the low one under the high one. Ends that came the
    /// wrong way round are put right, and ends that are the same (or no
    /// numbers at all) are set `least` apart.
    public static func range(_ low: Double, _ high: Double, least: Double = 1) -> ClosedRange<Double> {
        var low = finite(low)
        var high = finite(high, else: low)
        if low > high { swap(&low, &high) }
        if !(high > low) {
            // Far enough apart to be two numbers, however large they are.
            high = low + Swift.max(abs(least), abs(low) * 1e-9, 1e-9)
        }
        return low...high
    }

    /// `value` held within `range`; what is no number is the range's low end.
    public static func clamped(_ value: Double, to range: ClosedRange<Double>) -> Double {
        guard !value.isNaN else { return range.lowerBound }
        return Swift.min(Swift.max(value, range.lowerBound), range.upperBound)
    }
}

/// A slider's scale, from the words a page gives for its ends and its step.
public enum SliderRules {
    public struct Scale: Equatable, Sendable {
        public var range: ClosedRange<Double>
        public var step: Double
    }

    /// The most stops a slider has: a step that would make more is widened.
    public static let mostStops = 10_000.0

    /// A number a page wrote as text; `nil` for text that is none (`nan`,
    /// `inf` and `infinity` read as numbers, and are refused).
    public static func number(_ text: String?) -> Double? {
        guard let text, let value = Double(text.trimmingCharacters(in: .whitespaces)), value.isFinite else { return nil }
        return SafeNumbers.finite(value)
    }

    /// The scale: from `min` to `max` (0 and 100 where the page gives no
    /// number), the high end over the low one; a step above nothing, no wider
    /// than the scale, and no finer than ``mostStops`` stops along it.
    public static func scale(min: String?, max: String?, step: String?) -> Scale {
        let low = number(min) ?? 0
        let range = SafeNumbers.range(low, number(max) ?? Swift.max(100, low + 1))
        let width = range.upperBound - range.lowerBound
        var step = number(step) ?? 1
        if !(step > 0) { step = 1 }
        step = Swift.min(Swift.max(step, width / mostStops), width)
        return Scale(range: range, step: step)
    }

    /// The value a slider shows: the field's, held within the scale; the low
    /// end for a value that is no number.
    public static func value(_ text: String?, in scale: Scale) -> Double {
        SafeNumbers.clamped(number(text) ?? scale.range.lowerBound, to: scale.range)
    }

    /// A value the slider gives, as the form sends it: a whole number without
    /// a decimal point.
    public static func text(_ value: Double) -> String {
        let value = SafeNumbers.finite(value)
        return value == value.rounded() ? String(SafeNumbers.int(value)) : String(value)
    }
}
