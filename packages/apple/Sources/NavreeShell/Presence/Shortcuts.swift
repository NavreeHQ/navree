import Foundation

/// A quick way into the app from outside it: a Home Screen quick action, an
/// item of the Dock menu.
public struct ShortcutEntry: Sendable, Equatable {
    /// Its words: where it leads, without the app's name.
    public var label: String
    /// The address it opens.
    public var href: String
    /// Its system symbol.
    public var symbol: String?

    public init(label: String, href: String, symbol: String? = nil) {
        self.label = label
        self.href = href
        self.symbol = symbol
    }
}

/// A Home Screen quick action, in the terms of `UIApplicationShortcutItem`.
public struct QuickAction: Sendable, Equatable {
    public var type: String
    public var title: String
    public var symbol: String?
    /// The address it opens, kept in its `userInfo` under `href`.
    public var href: String
}

/// One item of the app's own part of the Dock menu.
public enum DockEntry: Sendable, Equatable {
    /// New Window.
    case newWindow
    /// A shortcut, opened in the window in front or a new one.
    case shortcut(ShortcutEntry)
}

/// The app's quick ways in from outside it, as plain rules.
public enum ShortcutRules {
    /// The type of every quick action the app opens: the dynamic ones it sets,
    /// and any static one an app writes in its Info.plist
    /// (`UIApplicationShortcutItems`) with an `href` in its `UIApplicationShortcutItemUserInfo`.
    public static let quickActionType = "navree.open"

    /// The shortcuts: none unless the app turns them on; then the app's first
    /// destinations, up to the limit (the Home Screen shows four). When the
    /// server's shell names shortcuts of its own (a protocol proposal), they
    /// are to come first.
    public static func entries(source: Shortcuts, destinations: [PresenceDestination]) -> [ShortcutEntry] {
        guard case .on(let limit) = source, limit > 0 else { return [] }
        return destinations
            .filter { OpeningRules.address($0.href) != nil }
            .prefix(limit)
            .map { ShortcutEntry(label: $0.label, href: $0.href, symbol: $0.symbol) }
    }

    /// A shortcut as a Home Screen quick action.
    public static func item(_ entry: ShortcutEntry) -> QuickAction {
        QuickAction(type: quickActionType, title: entry.label, symbol: entry.symbol, href: entry.href)
    }

    /// The address a quick action opens: one of the app's own type, carrying
    /// an address of its origin; nothing for any other.
    public static func href(type: String, userInfo: [String: Any]?) -> String? {
        guard type == quickActionType, let href = userInfo?["href"] as? String else { return nil }
        return OpeningRules.address(href)
    }

    /// The app's own part of the Dock menu: New Window, then the shortcuts;
    /// nothing when it has no shortcuts, so the system's items stand alone.
    public static func dockMenu(entries: [ShortcutEntry]) -> [DockEntry] {
        entries.isEmpty ? [] : [.newWindow] + entries.map(DockEntry.shortcut)
    }
}

/// Which window an address from outside the windows opens in.
public enum OpenTarget: Sendable, Equatable {
    /// The window of that name.
    case window(String)
    /// A new main window, which takes the address as it begins.
    case newWindow
}

/// Where an address from outside the app's windows goes (a quick action,
/// the Dock menu, the status menu, a notification, Help), as plain rules.
/// What it then opens is the core's to say: a destination's address selects
/// that destination, another is presented as the shell's routes say.
public enum OpeningRules {
    /// The window in front; else the window shown last; with none, a new one.
    public static func target(front: String?, windows: [String]) -> OpenTarget {
        if let front, windows.contains(front) { return .window(front) }
        if let last = windows.last { return .window(last) }
        return .newWindow
    }

    /// The address told to the core as a link from outside: only a path of
    /// the app's own origin (a single `/` first), never another site's.
    public static func address(_ href: String) -> String? {
        let text = href.trimmingCharacters(in: .whitespaces)
        guard text.hasPrefix("/"), !text.hasPrefix("//"), !text.contains("\\") else { return nil }
        return text
    }
}
