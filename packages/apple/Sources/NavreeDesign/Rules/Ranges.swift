import Foundation
import NavreeProtocol

/// A span of two ends, as one field.
public enum RangeRules {
    /// The words beside each end, as Calendar has them.
    public static let startWords = "Starts"
    public static let endWords = "Ends"

    /// The two ends after one was set: an end set before the start moves to
    /// the start, so the span never runs backwards.
    public static func values(from: String, to: String, kind: Navree.DateKind) -> [String] {
        guard let start = WebDate.date(from, kind: kind), let end = WebDate.date(to, kind: kind), end < start else {
            return [from, to]
        }
        return [from, from]
    }
}
