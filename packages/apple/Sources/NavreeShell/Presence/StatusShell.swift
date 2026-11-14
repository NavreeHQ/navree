#if os(macOS)
import AppKit
import NavreeDesign
import NavreeProtocol

/// The app's item in the menu bar opening a menu: the system's `NSStatusItem`,
/// made while the app stands in the menu bar and taken away when it does not.
/// Its menu is drawn from ``StatusRules/menu(destinations:appName:place:user:)``
/// each time it opens, and what is chosen opens in the main window.
@MainActor
final class StatusItemController: NSObject, NSMenuDelegate {
    /// The item, while the app stands in the menu bar.
    private(set) var item: NSStatusItem?
    private var visibility: NSKeyValueObservation?

    /// Puts the item in the menu bar or takes it out, as the place says, and
    /// draws its button.
    func apply(_ model: PresenceModel) {
        let wanted = StatusRules.inserted(style: .menu, item: model.presence.statusItem, place: model.place)
        guard wanted else { return remove() }
        let item = self.item ?? make()
        StatusButton.behave(item, model)
        StatusButton.draw(item, model)
    }

    private func make() -> NSStatusItem {
        let item = StatusButton.make()
        let menu = NSMenu()
        menu.delegate = self
        item.menu = menu
        self.item = item
        visibility = StatusButton.watch(item)
        return item
    }

    private func remove() {
        visibility = nil
        if let item { NSStatusBar.system.removeStatusItem(item) }
        item = nil
    }

    /// Drawn anew each time it opens, from what the window in front holds then.
    func menuNeedsUpdate(_ menu: NSMenu) {
        let model = PresenceModel.shared
        StatusMenus.fill(menu, StatusRules.menu(
            destinations: model.destinations, appName: PresenceModel.appName, place: model.place, user: model.user
        ))
    }
}

/// What the item's button shows and how it behaves, for either kind of item.
@MainActor
enum StatusButton {
    static func make() -> NSStatusItem {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        // Where the person put it stays; that it shows is the person's choice in Settings.
        item.autosaveName = "navree.status"
        item.isVisible = true
        return item
    }

    /// A person who takes the item out of the menu bar (⌘-drag) leaves an app
    /// in both places in the Dock, and ends an app in the menu bar only.
    static func behave(_ item: NSStatusItem, _ model: PresenceModel) {
        item.behavior = model.place == .menuBar ? [.removalAllowed, .terminationOnRemoval] : [.removalAllowed]
    }

    static func watch(_ item: NSStatusItem) -> NSKeyValueObservation {
        item.observe(\.isVisible, options: [.new]) { item, _ in
            let shown = item.isVisible
            Task { @MainActor in if !shown { PresenceModel.shared.removedFromMenuBar() } }
        }
    }

    /// The image (a template, named for VoiceOver by the item's name), the
    /// title, the count while the app is in the menu bar only, the tooltip.
    static func draw(_ item: NSStatusItem, _ model: PresenceModel) {
        guard let button = item.button, let status = model.presence.statusItem else { return }
        let parts = StatusRules.label(item: status, count: model.badge.count, place: model.place, appName: PresenceModel.appName)
        button.image = image(parts)
        button.imagePosition = .imageLeading
        button.title = [parts.title, parts.count].compactMap(\.self).joined(separator: " ")
        button.toolTip = status.tooltip
        button.setAccessibilityLabel(parts.accessibilityLabel)
    }

    static func image(_ parts: StatusLabelParts) -> NSImage? {
        let image: NSImage? = switch parts.image {
        case .symbol(let name): NSImage(systemSymbolName: name, accessibilityDescription: parts.name)
        case .asset(let name): NSImage(named: name)?.copy() as? NSImage
        }
        image?.isTemplate = true
        image?.accessibilityDescription = parts.name
        return image
    }
}

/// The items of an item's menu, and what each does.
@MainActor
final class StatusMenus: NSObject {
    static let shared = StatusMenus()

    static func fill(_ menu: NSMenu, _ entries: [StatusEntry]) {
        menu.removeAllItems()
        for entry in entries { menu.addItem(shared.item(entry)) }
    }

    private func item(_ entry: StatusEntry) -> NSMenuItem {
        switch entry {
        case .destination(let destination):
            let item = NSMenuItem(title: destination.label, action: #selector(open(_:)), keyEquivalent: "")
            item.representedObject = destination.href
            item.image = destination.symbol.flatMap { NSImage(systemSymbolName: $0, accessibilityDescription: nil) }
            switch BadgeRules.tabBadgeKind(destination.badge) {
            case .count(let count): item.badge = NSMenuItemBadge(count: count)
            case .word(let word): item.badge = NSMenuItemBadge(string: word)
            case .none: break
            }
            item.target = self
            return item
        case .open(let words):
            return item(words, #selector(openApp(_:)))
        case .account(let name):
            return NSMenuItem.sectionHeader(title: name)
        case .accountItem(let label, let index, _):
            let item = item(label, #selector(account(_:)))
            item.tag = index
            return item
        case .openInWindow:
            return item(String(localized: "Open in Window"), #selector(openInWindow(_:)))
        case .settings:
            return item(String(localized: "Settings…"), #selector(settings(_:)), key: ",")
        case .about(let words):
            return item(words, #selector(about(_:)))
        case .quit(let words):
            return NSMenuItem(title: words, action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        case .divider:
            return .separator()
        }
    }

    private func item(_ title: String, _ action: Selector, key: String = "") -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.target = self
        return item
    }

    @objc private func open(_ sender: NSMenuItem) {
        guard let href = sender.representedObject as? String else { return }
        PresenceModel.shared.open(href)
    }

    @objc private func openApp(_ sender: NSMenuItem) {
        PresenceModel.shared.openMainWindow()
    }

    @objc private func account(_ sender: NSMenuItem) {
        PresenceModel.shared.account(sender.tag)
    }

    @objc private func openInWindow(_ sender: NSMenuItem) {
        PresenceModel.shared.openMainWindow()
    }

    /// Settings…, as the App menu's own item opens it, with the app brought
    /// forward first so the window opens in front.
    @objc private func settings(_ sender: NSMenuItem) {
        Self.appMenuItem { $0.keyEquivalent == "," && $0.keyEquivalentModifierMask == .command }
    }

    /// About, as the App menu's own item opens it: the app's additions too.
    @objc private func about(_ sender: NSMenuItem) {
        Self.appMenuItem { $0.title.hasPrefix(String(localized: "About")) }
    }

    private static func appMenuItem(_ matching: (NSMenuItem) -> Bool) {
        NSApp.activate()
        guard let menu = NSApp.mainMenu?.items.first?.submenu, let index = menu.items.firstIndex(where: matching) else { return }
        menu.performActionForItem(at: index)
    }
}
#endif
