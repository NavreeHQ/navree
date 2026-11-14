import CoreGraphics
import Foundation

/// Where the app stands in the system outside its windows: the Dock and the
/// menu bar on a Mac, the count on its icon, its quick ways in, About and Help.
public struct Presence: Sendable, Equatable {
    /// Where a Mac shows the app when the person has not chosen.
    public var place: MacPlace
    /// The places Settings lets the person choose among. Fewer than two, or a
    /// menu-bar place without a status item, offers no choice.
    public var choices: [MacPlace]
    /// The app's item in the menu bar. Without one the app stands in the Dock
    /// only, whatever `place` says.
    public var statusItem: StatusItem?
    /// For an app in the menu bar only: whether its Dock icon and its menus come
    /// while one of its main windows is open, so that every command of the
    /// window is also in the menu bar, and go when the last one closes.
    public var dockWhileWindowOpen: Bool
    /// Whether Settings offers to open the app when the person logs in (Mac).
    /// The person turns it on; the app never does.
    public var offersOpenAtLogin: Bool
    /// The quick ways in from outside the app: Home Screen quick actions, the
    /// Dock menu.
    public var shortcuts: Shortcuts
    /// What the count on the app's icon follows.
    public var badge: BadgeSource
    /// What the About window says beside the app's name and version (Mac).
    public var about: About
    /// What the Help menu holds (Mac, and the iPad's menu bar).
    public var help: HelpMenu

    public init(
        place: MacPlace = .dock, choices: [MacPlace] = [], statusItem: StatusItem? = nil,
        dockWhileWindowOpen: Bool = true, offersOpenAtLogin: Bool = false, shortcuts: Shortcuts = .off,
        badge: BadgeSource = .calls, about: About = About(), help: HelpMenu = .system
    ) {
        self.place = place
        self.choices = choices
        self.statusItem = statusItem
        self.dockWhileWindowOpen = dockWhileWindowOpen
        self.offersOpenAtLogin = offersOpenAtLogin
        self.shortcuts = shortcuts
        self.badge = badge
        self.about = about
        self.help = help
    }
}

/// Where a Mac shows the app.
public enum MacPlace: String, Sendable, Equatable, CaseIterable, Codable {
    /// In the Dock and the app switcher, with its menus: an ordinary app.
    case dock
    /// In the menu bar only, as its status item: no Dock icon and no menus of
    /// its own. The app's Info.plist says `LSUIElement`.
    case menuBar
    /// In the Dock and in the menu bar.
    case both
}

/// The app's item in the menu bar.
public struct StatusItem: Sendable, Equatable {
    /// What it shows: a system symbol, or a template image of the app's assets.
    public var image: StatusImage
    /// Words beside the image. None by default: the menu bar is narrow, and the
    /// system hides items when it is crowded.
    public var title: String?
    /// Whether it shows the app's badge count beside its image while the app is
    /// not in the Dock.
    public var showsCount: Bool
    /// What a click opens.
    public var content: StatusContent
    /// What the item says when the pointer rests on it.
    public var tooltip: String?

    public init(
        image: StatusImage, title: String? = nil, showsCount: Bool = true, content: StatusContent = .menu,
        tooltip: String? = nil
    ) {
        self.image = image
        self.title = title
        self.showsCount = showsCount
        self.content = content
        self.tooltip = tooltip
    }
}

/// What the app's item in the menu bar shows.
public enum StatusImage: Sendable, Equatable {
    /// A system symbol by name, such as `shippingbox`.
    case symbol(String)
    /// A template image in the app's asset catalog, drawn in the menu bar's colour.
    case asset(String)
}

/// What a click on the app's item in the menu bar opens.
public enum StatusContent: Sendable, Equatable {
    /// A menu: the app's destinations, Settings… and Quit.
    case menu
    /// A small window showing one destination's screen (`/today`), for what a
    /// menu cannot hold.
    case screen(String)
    /// The whole app in a window that opens under the item, as the system's
    /// own items' windows do, for an app in the menu bar only: its one
    /// session, shown in a full window too when the person asks. A right
    /// click opens a menu of the app's destinations, Open in Window,
    /// Settings…, About and Quit.
    case app(StatusWindow = StatusWindow())
}

/// The window an item in the menu bar opens with the whole app in it.
public struct StatusWindow: Sendable, Equatable {
    /// The size it opens at.
    public var size: CGSize
    /// The smallest the person may make it, when it is resizable.
    public var minimumSize: CGSize
    /// Whether the person may change its size.
    public var resizable: Bool
    /// Whether it stays open, above other windows, when the person clicks
    /// elsewhere; else it closes, as a menu does.
    public var pinned: Bool
    /// Whether it opens as the app starts.
    public var opensAtLaunch: Bool

    public init(
        size: CGSize = CGSize(width: 400, height: 560), minimumSize: CGSize = CGSize(width: 320, height: 400),
        resizable: Bool = false, pinned: Bool = false, opensAtLaunch: Bool = false
    ) {
        self.size = size
        self.minimumSize = minimumSize
        self.resizable = resizable
        self.pinned = pinned
        self.opensAtLaunch = opensAtLaunch
    }
}

/// Quick ways into the app from outside it.
public enum Shortcuts: Sendable, Equatable {
    /// None: no Home Screen quick actions, no items of the app's own in the Dock menu.
    case off
    /// The server's shortcuts when its shell names them, else its first
    /// destinations; at most `limit` (the Home Screen shows four).
    case on(limit: Int = 4)
}

/// What the count on the app's icon follows.
public enum BadgeSource: Sendable, Equatable {
    /// The server's `set_badge` calls.
    case calls
    /// One destination's badge, by its address (`/inbox`), as Mail counts its
    /// inbox. A `set_badge` call still sets it until the destination's badge changes.
    case destination(String)
}

/// What the About window adds to the app's name, icon and version.
public struct About: Sendable, Equatable {
    /// Lines under the version, as Markdown: who made it, with links.
    public var credits: String?
    /// The copyright line; the Info.plist's `NSHumanReadableCopyright` when nil.
    public var copyright: String?

    public init(credits: String? = nil, copyright: String? = nil) {
        self.credits = credits
        self.copyright = copyright
    }
}

/// What the Help menu holds.
public enum HelpMenu: Sendable, Equatable {
    /// The system's own item, which opens the app's help book if it has one.
    case system
    /// "<App> Help" opens a page of the app, as a link from outside would (`/help`).
    case page(String)
    /// "<App> Help" opens a site in the person's browser.
    case site(URL)
    /// No help item; the menu keeps the system's search field.
    case hidden
}
