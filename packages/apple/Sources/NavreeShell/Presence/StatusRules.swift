import CoreGraphics
import Foundation
import NavreeDesign
import NavreeProtocol
import NavreeRuntime

/// How the app's item in the menu bar opens: the system's `MenuBarExtraStyle`.
public enum StatusStyle: String, Sendable, Equatable {
    /// A menu that pulls down from the item.
    case menu
    /// A small window under the item.
    case window
}

/// One entry of the status item's menu, in order.
public enum StatusEntry: Sendable, Equatable {
    /// A destination, opened in the main window.
    case destination(PresenceDestination)
    /// "Open <App>": the main window, before any has shown the app's destinations.
    case open(String)
    /// The person signed in, heading their menu.
    case account(String)
    /// An entry of the person's menu, by its place in it.
    case accountItem(label: String, index: Int, destructive: Bool)
    /// Open in Window: the app's window, out of the menu bar.
    case openInWindow
    /// Settings… (⌘,).
    case settings
    /// "About <App>".
    case about(String)
    /// "Quit <App>" (⌘Q).
    case quit(String)
    case divider
}

/// What the status item's button shows.
public struct StatusLabelParts: Sendable, Equatable {
    public var image: StatusImage
    public var title: String?
    /// The count beside the image, as digits.
    public var count: String?
    /// What VoiceOver calls the item.
    public var accessibilityLabel: String
}

/// The app's item in the menu bar, as plain rules.
public enum StatusRules {
    /// The bar of a window: the system's unified toolbar, or its compact one.
    public enum WindowBar: Sendable, Equatable {
        case unified
        case compact
    }

    /// The bar of the window under an item in the menu bar is the system's
    /// compact toolbar, the one AppKit gives a narrow utility window: its
    /// controls 24 points tall (the unified bar's are 36-point circles, heavy
    /// in a window 400 points wide), its height 40 points. The content
    /// starts under it with a grouped form's room
    /// (``CompositionRules/blockRoom(bar:)``).
    public static let windowBar = WindowBar.compact

    /// The style the item opens in: a menu. A small window showing one
    /// destination's screen is not built yet (it needs one session shared by
    /// the app's cores), so an item that asks for one has no style and does
    /// not stand in the menu bar.
    public static func style(_ item: StatusItem?) -> StatusStyle? {
        switch item?.content {
        case .menu?: .menu
        case .app?: .window
        case .screen?, nil: nil
        }
    }

    /// Whether the item's window holds the whole app: then it is the app's
    /// one window in the menu bar, with the app's one session.
    public static func holdsApp(_ item: StatusItem?) -> Bool {
        if case .app? = item?.content { true } else { false }
    }

    /// Where the item's window stands: under the item, centred on it, kept on
    /// the item's screen, its top a few points under the menu bar.
    public static func windowFrame(item: CGRect, size: CGSize, screen: CGRect) -> CGRect {
        let margin: CGFloat = 8, gap: CGFloat = 4
        var x = item.midX - size.width / 2
        x = min(max(x, screen.minX + margin), screen.maxX - margin - size.width)
        let top = min(item.minY, screen.maxY) - gap
        let height = min(size.height, top - screen.minY - margin)
        return CGRect(x: x, y: top - height, width: size.width, height: height)
    }

    /// The width the window's core is told: the window under an item in the
    /// menu bar is a compact window, laid out as one column with its
    /// destinations' sidebar folded away; any other window by its width.
    public static func width(compactWindow: Bool, sizeClass: WidthClass?, points: CGFloat) -> Width {
        compactWindow ? .compact : WindowRules.width(sizeClass: sizeClass, points: points)
    }

    /// The menu a right click on an item holding the whole app opens: the
    /// destinations, the person's menu, Open in Window, Settings…, About and Quit.
    public static func contextMenu(
        destinations: [PresenceDestination], appName: String, user: Navree.User?
    ) -> [StatusEntry] {
        var entries = menu(destinations: destinations, appName: appName, place: .menuBar, user: user)
            .filter { if case .open = $0 { false } else { true } }
        if entries.first == .divider { entries.removeFirst() }
        let settings = entries.firstIndex(of: .settings) ?? entries.endIndex
        entries.insert(.openInWindow, at: settings)
        entries.insert(.about(String(localized: "About \(appName)")), at: settings + 2)
        return entries
    }

    /// Whether the scene of one style is in the menu bar: only while the app
    /// stands there, and only the scene of the item's own style.
    public static func inserted(style: StatusStyle, item: StatusItem?, place: MacPlace) -> Bool {
        place != .dock && self.style(item) == style
    }

    /// The menu's entries: the destinations (or, before a window has shown
    /// them, one entry to open the app), the person's menu when the app is in
    /// the menu bar only and so has no App menu, Settings…, and Quit.
    public static func menu(
        destinations: [PresenceDestination], appName: String, place: MacPlace, user: Navree.User?
    ) -> [StatusEntry] {
        var entries: [StatusEntry] = destinations.isEmpty
            ? [.open(String(localized: "Open \(appName)"))]
            : destinations.map(StatusEntry.destination)
        entries.append(.divider)
        if place == .menuBar, let user {
            entries.append(.account(user.name))
            let settings = SettingsRules.destination(in: user.menu)?.index
            for (index, node) in user.menu.enumerated() where index != settings {
                switch node {
                case .link(let link):
                    entries.append(.accountItem(label: CommandRules.title(link.label), index: index, destructive: false))
                case .button(let button):
                    entries.append(.accountItem(
                        label: CommandRules.title(button.label), index: index, destructive: button.role == .destructive
                    ))
                default:
                    break
                }
            }
            entries.append(.divider)
        }
        entries.append(.settings)
        entries.append(.divider)
        entries.append(.quit(String(localized: "Quit \(appName)")))
        return entries
    }

    /// The button: the image, the title when the app gives one, and the count
    /// while the app stands in the menu bar only (in the Dock the count is the
    /// Dock icon's). VoiceOver names it by its title, else by the app.
    public static func label(item: StatusItem, count: Int?, place: MacPlace, appName: String) -> StatusLabelParts {
        let shown = item.showsCount && place == .menuBar ? BadgeRules.label(count) : nil
        let title = item.title.flatMap { $0.trimmingCharacters(in: .whitespaces).isEmpty ? nil : $0 }
        return StatusLabelParts(image: item.image, title: title, count: shown, accessibilityLabel: title ?? appName)
    }
}
