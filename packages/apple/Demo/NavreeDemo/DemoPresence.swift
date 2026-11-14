import Foundation
import NavreeShell
import UserNotifications

/// Where the demo stands in the system outside its windows, as its launch
/// arguments say: by default an ordinary app in the Dock.
///
/// - `-NavreePresence dock|menuBar|both` (or the Info.plist's `NavreePresence`):
///   where it stands, with an item in the menu bar showing `shippingbox`
///   wherever the menu bar is among its places;
/// - `-NavreeChoices "dock;menuBar;both"`: the places Settings offers;
/// - `-NavreeStatus menu|/today`: what the item opens (a small window showing
///   a destination is not built yet);
/// - `-NavreeDockWhileWindowOpen NO`: a menu-bar app stays out of the Dock
///   while its window is open;
/// - `-NavreeShortcuts 4`: quick actions and the Dock menu, from the first
///   destinations;
/// - `-NavreeBadgeFollows /orders`: the app's count follows that destination's badge;
/// - `-NavreeBadge 3`: sets the count at launch as a server's `set_badge` would;
/// - `-NavreeHelp /help|hidden|https://…`: what the Help menu holds;
/// - `-NavreeAbout YES`: sample credits with a link, and a copyright line;
/// - `-NavreeOpenAtLogin YES`: Settings offers to open the app at login.
enum DemoPresence {
    static func make(_ defaults: UserDefaults) -> Presence {
        let info = Bundle.main.object(forInfoDictionaryKey: "NavreePresence") as? String
        let place = (defaults.string(forKey: "NavreePresence") ?? info).flatMap(MacPlace.init(rawValue:)) ?? .dock
        let choices = (defaults.string(forKey: "NavreeChoices") ?? "")
            .split(separator: ";").compactMap { MacPlace(rawValue: String($0)) }
        let status = defaults.string(forKey: "NavreeStatus")
        let content: StatusContent = status.flatMap { $0.hasPrefix("/") ? .screen($0) : nil } ?? .menu
        let inMenuBar = place != .dock || choices.contains { $0 != .dock }
        var presence = Presence(
            place: place, choices: choices,
            statusItem: inMenuBar ? StatusItem(image: .symbol("shippingbox"), content: content) : nil,
            dockWhileWindowOpen: defaults.object(forKey: "NavreeDockWhileWindowOpen") == nil
                || defaults.bool(forKey: "NavreeDockWhileWindowOpen"),
            offersOpenAtLogin: defaults.bool(forKey: "NavreeOpenAtLogin")
        )
        let shortcuts = defaults.integer(forKey: "NavreeShortcuts")
        if shortcuts > 0 { presence.shortcuts = .on(limit: shortcuts) }
        if let followed = defaults.string(forKey: "NavreeBadgeFollows") { presence.badge = .destination(followed) }
        if defaults.bool(forKey: "NavreeAbout") {
            presence.about = About(
                credits: "Made in Ludhiana for the shops of Punjab, on [Navree](https://github.com/NavreeHQ/navree).",
                copyright: "© 2026 Charkha Traders, Amritsar"
            )
        }
        switch defaults.string(forKey: "NavreeHelp") {
        case "hidden"?: presence.help = .hidden
        case let help? where help.hasPrefix("/"): presence.help = .page(help)
        case let help? where help.hasPrefix("https://"): presence.help = URL(string: help).map(HelpMenu.site) ?? .system
        default: break
        }
        if defaults.object(forKey: "NavreeBadge") != nil {
            let count = defaults.integer(forKey: "NavreeBadge")
            Task { @MainActor in await badge(count) }
        }
        #if os(macOS)
        // An app in the menu bar only opens no window to start its pictures from.
        if defaults.string(forKey: "NavreePictures") != nil,
            PresenceRules.place(presence, chosen: PresenceModel.storedPlace) == .menuBar
        {
            Task { @MainActor in
                try? await Task.sleep(for: .seconds(1))
                await WindowPictures.run()
            }
        }
        #endif
        return presence
    }

    /// The count at launch, as a server's `set_badge` sets it. The Home
    /// Screen shows it only once notifications are allowed: the demo asks.
    @MainActor
    static func badge(_ count: Int) async {
        #if os(iOS)
        _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.badge])
        #endif
        PresenceModel.shared.call(badge: count)
    }
}
