#if os(macOS)
import AppKit
import UserNotifications

/// What the system tells a Mac app outside any window, passed on to the
/// shell: the Dock menu, opening the app again with no window showing, and a
/// notification the person clicked. An app adopts it in its `App`:
///
/// ```swift
/// @NSApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
/// ```
@MainActor
open class NavreeAppDelegate: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    open func applicationDidFinishLaunching(_ notification: Notification) {
        if PresenceModel.isApp { UNUserNotificationCenter.current().delegate = self }
        PresenceModel.shared.launch()
    }

    /// The app's own part of the Dock menu: New Window and the shortcuts,
    /// above the system's own items; none without shortcuts.
    open func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
        DockMenu.shared.menu()
    }

    /// An app in the menu bar only, opened again with no window showing (a
    /// double-click in the Finder, Spotlight), opens its main window; any
    /// other is left to the system, which opens one as it always has.
    open func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        let model = PresenceModel.shared
        guard PresenceRules.reopenOpensWindow(place: model.place, visibleWindows: hasVisibleWindows) else { return true }
        model.openMainWindow()
        return false
    }

    /// An app outlives its last window: one in the menu bar must, and a Mac
    /// app's windows come and go.
    open func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    /// A notification clicked: its address opens in the window in front, or a new one.
    nonisolated open func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse
    ) async {
        guard let href = AppEvents.href(response.notification.request.content.userInfo) else { return }
        await MainActor.run { PresenceModel.shared.open(href) }
    }

    /// A notification that arrives while the app is in front is shown as the
    /// system shows it, not swallowed.
    nonisolated open func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .list, .sound]
    }
}

/// The app's own part of the Dock menu, built each time the Dock asks.
@MainActor
final class DockMenu: NSObject {
    static let shared = DockMenu()

    /// What the Dock menu holds now: nothing of the app's own without shortcuts.
    func entries() -> [DockEntry] {
        let model = PresenceModel.shared
        return ShortcutRules.dockMenu(
            entries: ShortcutRules.entries(source: model.presence.shortcuts, destinations: model.destinations)
        )
    }

    func menu() -> NSMenu? {
        let entries = entries()
        guard !entries.isEmpty else { return nil }
        let menu = NSMenu()
        for entry in entries {
            switch entry {
            case .newWindow:
                let item = NSMenuItem(title: String(localized: "New Window"), action: #selector(newWindow(_:)), keyEquivalent: "")
                item.target = self
                menu.addItem(item)
                menu.addItem(.separator())
            case .shortcut(let shortcut):
                let item = NSMenuItem(title: shortcut.label, action: #selector(open(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = shortcut.href
                menu.addItem(item)
            }
        }
        return menu
    }

    @objc private func newWindow(_ sender: NSMenuItem) {
        PresenceModel.shared.newWindow()
    }

    @objc private func open(_ sender: NSMenuItem) {
        guard let href = sender.representedObject as? String else { return }
        PresenceModel.shared.open(href)
    }
}
#endif
