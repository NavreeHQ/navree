import Foundation
import NavreeProtocol

/// The shell's route rules: how the screens under an address are presented.
public struct RouteRules: Sendable, Equatable {
    public let routes: [Navree.Route]

    public init(_ routes: [Navree.Route]) {
        self.routes = routes
    }

    /// The first rule whose pattern matches the path of `address`; the query is
    /// not part of the match. `*` stands for exactly one segment.
    public func rule(for address: String) -> Navree.Route? {
        let segments = Self.segments(of: address)
        return routes.first { Self.segments(of: $0.match).matches(segments) }
    }

    /// How a link to `address` is presented: what the link asked for, unless a
    /// rule says otherwise for a plain push. A link that leaves it to the app
    /// (`auto`) is a plain push.
    public func presentation(of address: String, asked: Navree.Present = .auto) -> Navree.Present {
        let asked = asked == .auto ? .push : asked
        guard asked == .push, let present = rule(for: address)?.present, present != .auto else { return asked }
        return present
    }

    /// Whether the app's navigation stays around the screen at `address`.
    public func keepsChrome(_ address: String) -> Bool {
        rule(for: address)?.chrome ?? true
    }

    static func segments(of address: String) -> [Substring] {
        let path = address.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
        return path.split(separator: "/", omittingEmptySubsequences: true)
    }
}

extension Array where Element == Substring {
    fileprivate func matches(_ path: [Substring]) -> Bool {
        count == path.count && zip(self, path).allSatisfy { pattern, segment in pattern == "*" || pattern == segment }
    }
}
