import Foundation

/// How a Mac app stands, in the system's two words for an app with windows:
/// `NSApplication.ActivationPolicy.regular` and `.accessory`.
public enum AppPolicy: String, Sendable, Equatable {
    /// In the Dock and the app switcher, with its menus: an ordinary app.
    case regular
    /// No Dock icon and no menus of its own; its windows and its item in the
    /// menu bar still come forward when clicked.
    case accessory
}

/// Whether a main window opens as the app starts: the system's
/// `SceneLaunchBehavior`, in the two values the app chooses between.
public enum WindowLaunch: String, Sendable, Equatable {
    /// As for any app: a window opens unless the system restores others.
    case automatic
    /// None: the app waits in the menu bar until the person opens one.
    case suppressed
}

/// A destination as the app's ways in from outside its windows show it: the
/// status menu, the Dock menu, the Home Screen's quick actions, the badge.
/// A mirror of what the window in front's core holds, never a choice of its own.
public struct PresenceDestination: Sendable, Equatable {
    public var label: String
    public var href: String
    /// Its system symbol, when its icon has one.
    public var symbol: String?
    /// Its badge, in the page's words.
    public var badge: String?

    public init(label: String, href: String, symbol: String? = nil, badge: String? = nil) {
        self.label = label
        self.href = href
        self.symbol = symbol
        self.badge = badge
    }
}

/// Where a Mac app stands, as plain rules: the place it takes, whether it is
/// in the Dock, whether a window opens at launch.
public enum PresenceRules {
    /// Where the person's choice of place is kept.
    public static let placeKey = "navree.place"

    /// The place the app takes: the person's choice among the offered places,
    /// else the build's. An app whose status item cannot stand in the menu
    /// bar stands in the Dock, so it can always be reached.
    public static func place(_ presence: Presence, chosen: MacPlace?) -> MacPlace {
        guard StatusRules.style(presence.statusItem) != nil else { return .dock }
        let place = chosen.flatMap { choices(presence).contains($0) ? $0 : nil } ?? presence.place
        // An item holding the whole app is the app's one window in the menu
        // bar: beside a Dock app's own windows it would be a second session.
        return place == .both && StatusRules.holdsApp(presence.statusItem) ? .menuBar : place
    }

    /// The places Settings offers, in the build's order: none when fewer than
    /// two can stand. The menu bar is never offered without a status item.
    public static func choices(_ presence: Presence) -> [MacPlace] {
        let item = StatusRules.style(presence.statusItem) != nil
        let app = StatusRules.holdsApp(presence.statusItem)
        var places: [MacPlace] = []
        for place in presence.choices where !places.contains(place) && (place == .dock || item) && !(app && place == .both) {
            places.append(place)
        }
        return places.count < 2 ? [] : places
    }

    /// Whether the app is in the Dock: always, unless it stands in the menu
    /// bar only; then only while one of its main windows is open, when the
    /// app asks for its Dock icon and menus to come with a window.
    public static func policy(place: MacPlace, mainWindows: Int, dockWhileWindowOpen: Bool) -> AppPolicy {
        switch place {
        case .dock, .both: .regular
        case .menuBar: mainWindows > 0 && dockWhileWindowOpen ? .regular : .accessory
        }
    }

    /// Whether a main window opens at launch: not for an app in the menu bar
    /// only, which waits for the person to open one.
    public static func launch(place: MacPlace) -> WindowLaunch {
        place == .menuBar ? .suppressed : .automatic
    }

    /// The place after the person took the item out of the menu bar (⌘-drag):
    /// an app in both places stays in the Dock; an app in the menu bar only is
    /// ended by the system (nil).
    public static func placeAfterRemoval(_ place: MacPlace) -> MacPlace? {
        switch place {
        case .dock, .both: .dock
        case .menuBar: nil
        }
    }

    /// Whether the app's Info.plist must say `LSUIElement`: when it may ever
    /// start in the menu bar only, so its Dock icon never shows at launch.
    public static func needsUIElement(_ presence: Presence) -> Bool {
        place(presence, chosen: nil) == .menuBar || choices(presence).contains(.menuBar)
    }

    /// What is wrong when the Info.plist and the configuration disagree, in
    /// words for the developer's log; nil when they agree.
    public static func infoPlistFault(presence: Presence, uiElement: Bool) -> String? {
        switch (needsUIElement(presence), uiElement) {
        case (true, false):
            "The app may start in the menu bar only, but its Info.plist does not say LSUIElement: its Dock icon shows for a moment at every such launch."
        case (false, true):
            "The app's Info.plist says LSUIElement, but the app never stands in the menu bar only: it starts with no Dock icon and comes into the Dock a moment later."
        default:
            nil
        }
    }

    /// Whether the app opens a main window itself when the person opens it
    /// again (a double-click in the Finder, Spotlight) with none showing: an
    /// app in the menu bar only does; a Dock app is left to the system.
    public static func reopenOpensWindow(place: MacPlace, visibleWindows: Bool) -> Bool {
        place == .menuBar && !visibleWindows
    }
}
