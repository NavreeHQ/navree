import Foundation
import SwiftUI

/// Where the app's count is shown.
public enum BadgeTarget: String, Sendable, Equatable, Hashable {
    /// The app's icon in the Dock (a Mac) or on the Home Screen.
    case appIcon
    /// The app's item in the menu bar.
    case statusItem
}

/// A destination's badge in the tab bar and the sidebar.
public enum TabBadge: Sendable, Equatable {
    case none
    /// A count, drawn as the system draws a count.
    case count(Int)
    /// Words, such as "New", drawn as they are.
    case word(String)
}

/// The count on the app's icon, as plain rules: what it says, what sets it,
/// where it shows; and a destination's badge.
public enum BadgeRules {
    /// The badge's words: the count's digits; nothing for no count or zero.
    public static func label(_ count: Int?) -> String? {
        guard let count, count > 0 else { return nil }
        return String(count)
    }

    /// A destination's badge as a count: only when it is a number written in
    /// digits; words count for nothing on the app's icon.
    public static func count(destinationBadge badge: String?) -> Int? {
        guard let text = badge?.trimmingCharacters(in: .whitespaces), !text.isEmpty,
            text.allSatisfy({ $0.isASCII && $0.isNumber })
        else { return nil }
        return Int(text)
    }

    /// What sets the count: a `set_badge` call always; the followed
    /// destination's badge whenever it changes.
    public struct State: Sendable, Equatable {
        public var count: Int?
        /// The followed destination's badge when it was last seen.
        var seen: Seen = .never

        enum Seen: Sendable, Equatable {
            case never
            case badge(String?)
        }

        public init(count: Int? = nil) {
            self.count = count
        }
    }

    /// The state after a server's `set_badge` call: its count, whatever the
    /// app follows.
    public static func called(_ state: State, count: Int?) -> State {
        var state = state
        state.count = count.flatMap { $0 > 0 ? $0 : nil }
        return state
    }

    /// The state after the window in front's destinations changed: an app
    /// that follows a destination takes that destination's badge when it
    /// changed since it was last seen; a call's count stands until then.
    public static func destinationsChanged(
        _ state: State, source: BadgeSource, destinations: [PresenceDestination]
    ) -> State {
        guard case .destination(let href) = source else { return state }
        let badge = destinations.first { $0.href == href }?.badge
        guard state.seen != .badge(badge) else { return state }
        var state = state
        state.seen = .badge(badge)
        state.count = count(destinationBadge: badge).flatMap { $0 > 0 ? $0 : nil }
        return state
    }

    /// Where the count shows on a Mac: on the Dock icon, which the system
    /// shows whenever the app is in the Dock; and, for an app in the menu bar
    /// only whose item shows it, on the item too.
    public static func targets(place: MacPlace, statusShowsCount: Bool) -> Set<BadgeTarget> {
        place == .menuBar && statusShowsCount ? [.appIcon, .statusItem] : [.appIcon]
    }

    /// A destination's badge, as the tab bar and the sidebar draw it: a
    /// number as a count (none for zero), a word as its words.
    public static func tabBadgeKind(_ badge: String?) -> TabBadge {
        guard let text = badge?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty else { return .none }
        if let count = count(destinationBadge: text) { return count > 0 ? .count(count) : .none }
        return .word(text)
    }

    /// A destination's badge in the tab bar and the sidebar, for `.badge(_:)`.
    public static func tabBadge(_ badge: String?) -> Text? {
        switch tabBadgeKind(badge) {
        case .none: nil
        case .count(let count): Text(count, format: .number)
        case .word(let word): Text(word)
        }
    }
}
