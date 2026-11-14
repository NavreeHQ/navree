import Foundation
import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import Observation
import SwiftUI
import UserNotifications
import os

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// What the app's place in the system needs of its windows, once for the
/// process: the destinations of the window in front, the count on the app's
/// icon, the windows themselves, and on a Mac where the app stands. All of it
/// mirrors what the windows' cores hold, what the person chose or what the
/// system says: where anything leads stays the cores' to say.
@MainActor
@Observable
public final class PresenceModel {
    /// The one model of the process.
    public static let shared = PresenceModel()

    nonisolated static let log = Logger(subsystem: "com.navree.native", category: "presence")

    /// The app's presence, as its configuration says.
    public private(set) var presence = Presence()
    /// The person's choice of place, from Settings.
    public private(set) var chosen: MacPlace? = PresenceModel.storedPlace
    /// The window in front's destinations.
    public private(set) var destinations: [PresenceDestination] = []
    /// The person signed in, in the window in front.
    public private(set) var user: Navree.User?
    /// The count on the app's icon, and what set it.
    public private(set) var badge = BadgeRules.State()
    /// The window in front, by its scene's name.
    public private(set) var front: String?
    /// How many main windows are open (a Mac's, minimised ones included).
    public private(set) var mainWindows = 0
    /// Whether the window under the app's item in the menu bar is open.
    public private(set) var statusWindowShown = false
    /// Whether the shell the app's item holds stands under the item, not in a full window.
    var shellUnderItem = true

    /// Where the app stands on a Mac.
    public var place: MacPlace { PresenceRules.place(presence, chosen: chosen) }

    /// Each window's session, by its scene's name.
    @ObservationIgnored var sessions: [String: Weak<Session>] = [:]
    /// The windows' names, the one shown last at the end.
    @ObservationIgnored var order: [String] = []
    /// What each window last heard of its destinations and its person.
    @ObservationIgnored private var heard: [String: (destinations: [PresenceDestination], user: Navree.User?)] = [:]
    /// An address waiting for the next window that becomes ready.
    @ObservationIgnored private var pending: String?
    /// What opens a new main window, kept from a view that had it.
    @ObservationIgnored private var openWindow: OpenWindowAction?
    @ObservationIgnored private var configured = false
    @ObservationIgnored private var launched = false
    /// The quick actions last given to the system.
    @ObservationIgnored private var quickActions: [QuickAction]?
    #if os(macOS)
    /// The app's item in the menu bar, opening a menu.
    @ObservationIgnored private let statusItem = StatusItemController()
    /// The app's item in the menu bar, holding the whole app.
    @ObservationIgnored private let statusPanel = StatusPanelController()
    /// The main windows, by their scene's name.
    @ObservationIgnored private var windows: [String: Weak<NSWindow>] = [:]
    @ObservationIgnored private var watching: [String: [any NSObjectProtocol]] = [:]
    #endif

    init() {}

    /// The app's name, as the system shows it in its menus.
    public static var appName: String {
        let info = Bundle.main.infoDictionary ?? [:]
        return info["CFBundleDisplayName"] as? String ?? info["CFBundleName"] as? String
            ?? ProcessInfo.processInfo.processName
    }

    /// The person's choice of place, as the system's defaults keep it.
    public static var storedPlace: MacPlace? {
        UserDefaults.standard.string(forKey: PresenceRules.placeKey).flatMap(MacPlace.init(rawValue:))
    }

    /// Whether the process is an app, whose icon and notifications the system
    /// serves: not a test runner.
    nonisolated static var isApp: Bool { Bundle.main.bundleURL.pathExtension == "app" }

    /// Takes the app's whole configuration, for an item in the menu bar
    /// holding the whole app; the first one given stands.
    func configure(_ configuration: ShellConfiguration) {
        #if os(macOS)
        if statusPanel.configuration == nil { statusPanel.configuration = configuration }
        #endif
        configure(configuration.presence)
    }

    /// Takes the app's configuration; the first one given stands.
    func configure(_ presence: Presence) {
        guard !configured else { return }
        configured = true
        self.presence = presence
        #if os(macOS)
        // Also for an app that declares its scenes without the library's delegate.
        if Self.isApp {
            launching = NotificationCenter.default.addObserver(
                forName: NSApplication.didFinishLaunchingNotification, object: nil, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.launch() }
            }
        }
        #endif
        applyPolicy()
    }

    @ObservationIgnored private var launching: (any NSObjectProtocol)?

    /// The system finished launching the app: from now on the app's place is
    /// the system's to show.
    func launch() {
        guard !launched else { return }
        launched = true
        #if os(macOS)
        let uiElement = Bundle.main.object(forInfoDictionaryKey: "LSUIElement").map { "\($0)" }
        let fault = PresenceRules.infoPlistFault(
            presence: presence, uiElement: ["1", "true", "YES"].contains(uiElement ?? "")
        )
        if let fault { Self.log.fault("\(fault, privacy: .public)") }
        if case .screen? = presence.statusItem?.content {
            Self.log.fault("A status item showing a screen is not built yet: the app stands in the Dock.")
        }
        #endif
        applyPolicy()
    }

    // MARK: - Where the app stands

    /// The person chose where the app stands, in Settings.
    public func choose(_ place: MacPlace) {
        chosen = place
        UserDefaults.standard.set(place.rawValue, forKey: PresenceRules.placeKey)
        applyPolicy()
    }

    /// The person took the app's item out of the menu bar: an app in both
    /// places stays in the Dock; the system ends an app in the menu bar only.
    func removedFromMenuBar() {
        if let next = PresenceRules.placeAfterRemoval(place), next != place { choose(next) }
    }

    /// The system's activation policy, as the rule says for the place and the
    /// open windows; set only when it differs, and the app brought forward
    /// when it comes into the Dock.
    private func applyPolicy() {
        #if os(macOS)
        applyStatusItem()
        guard launched, Self.isApp else { return }
        let wanted = PresenceRules.policy(
            place: place, mainWindows: mainWindows, dockWhileWindowOpen: presence.dockWhileWindowOpen
        )
        let policy: NSApplication.ActivationPolicy = wanted == .regular ? .regular : .accessory
        guard NSApp.activationPolicy() != policy else { return }
        let changed = NSApp.setActivationPolicy(policy)
        Self.log.info("activation policy \(wanted.rawValue, privacy: .public): \(changed ? "set" : "refused", privacy: .public)")
        if changed, policy == .regular { NSApp.activate() }
        #endif
    }

    /// The item in the menu bar, as the place says, with the count.
    private func applyStatusItem() {
        #if os(macOS)
        guard launched, Self.isApp else { return }
        statusItem.apply(self)
        statusPanel.apply(self)
        #endif
    }

    /// Whether the app's item in the menu bar holds the whole app: an app in
    /// the menu bar only whose item opens a window with the app in it.
    public var holdsApp: Bool {
        place == .menuBar && StatusRules.holdsApp(presence.statusItem)
    }

    /// Opens the window under the app's item in the menu bar, when it has one
    /// holding the whole app.
    public func showStatusWindow() {
        #if os(macOS)
        statusPanel.show()
        #endif
    }

    /// Closes the window under the app's item in the menu bar.
    public func hideStatusWindow() {
        #if os(macOS)
        statusPanel.hide()
        #endif
    }

    /// The app held by its item in the menu bar, in a full window, with the
    /// app in the Dock while it is open.
    public func openInWindow() {
        #if os(macOS)
        statusPanel.openInWindow()
        #endif
    }

    func statusWindowChanged(shown: Bool) {
        statusWindowShown = shown
    }

    /// How many of the system's own sheets (signing in) stand over the app's
    /// windows: while one does, the window under the app's item stays open.
    @ObservationIgnored private(set) var systemSheets = 0

    /// One of the system's own sheets opened or closed.
    func systemSheet(opened: Bool) {
        systemSheets = max(0, systemSheets + (opened ? 1 : -1))
    }

    #if os(macOS)
    /// The window the system's own sheets (signing in) stand over while the
    /// app is held by its item in the menu bar: the window holding it, the one
    /// under the item opened for the sheet; none otherwise, and the key window
    /// stands.
    func windowForSystemSheets() -> NSWindow? {
        holdsApp ? statusPanel.windowForSystemSheets() : nil
    }
    #endif

    #if os(macOS)
    /// The app's item in the menu bar while it has one: its button, and its
    /// menu as it is drawn when it opens.
    public var statusItemParts: (button: NSStatusBarButton?, menu: NSMenu?)? {
        if let item = statusPanel.item { return (item.button, statusPanel.menu()) }
        guard let item = statusItem.item else { return nil }
        if let menu = item.menu { statusItem.menuNeedsUpdate(menu) }
        return (item.button, item.menu)
    }

    /// The window under the app's item holding the whole app, and the full
    /// window the person opened it in.
    public var statusWindows: (panel: NSWindow?, window: NSWindow?) {
        (statusPanel.panel, statusPanel.window)
    }
    #endif

    // MARK: - The windows

    /// A window began, or took another name.
    func attach(scene: String, session: Session) {
        sessions[scene] = Weak(session)
        if !order.contains(scene) { order.append(scene) }
        if front == nil { show(scene) }
    }

    /// A window went.
    func detach(scene: String) {
        sessions[scene] = nil
        heard[scene] = nil
        order.removeAll { $0 == scene }
        if front == scene { front = order.last }
        if let front, let mirror = heard[front] { publish(mirror.destinations, user: mirror.user) }
        // The person's menu acts on a window's session: with none, it goes.
        // The destinations stay, to open a window at one of them.
        if order.isEmpty { user = nil }
        // Its system window is let go when it closes: a shell moved to
        // another window disappears from the first without its window closing.
    }

    /// A window came in front.
    func show(_ scene: String) {
        guard sessions[scene] != nil else { return }
        front = scene
        order.removeAll { $0 == scene }
        order.append(scene)
        if let mirror = heard[scene] { publish(mirror.destinations, user: mirror.user) }
    }

    /// What a window's core now holds of its destinations and its person.
    func heard(scene: String, destinations: [PresenceDestination], user: Navree.User?) {
        heard[scene] = (destinations, user)
        if front == nil { show(scene) }
        if front == scene { publish(destinations, user: user) }
    }

    /// Keeps what opens a new main window, for when no view is at hand (the
    /// Dock menu, a notification tapped).
    func keep(_ action: OpenWindowAction) {
        openWindow = action
    }

    #if os(macOS)
    /// A main window's system window: counted, and watched for coming in
    /// front and for closing.
    func found(_ window: NSWindow, scene: String) {
        // The window under the item in the menu bar is no main window.
        guard windows[scene]?.object !== window, !(window is StatusPanel) else { return }
        forget(window: scene)
        windows[scene] = Weak(window)
        let center = NotificationCenter.default
        watching[scene] = [
            center.addObserver(forName: NSWindow.didBecomeKeyNotification, object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.show(scene) }
            },
            center.addObserver(forName: NSWindow.willCloseNotification, object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.forget(window: scene) }
            },
        ]
        count()
    }

    private func forget(window scene: String) {
        for token in watching.removeValue(forKey: scene) ?? [] { NotificationCenter.default.removeObserver(token) }
        guard windows.removeValue(forKey: scene) != nil else { return }
        count()
    }

    private func count() {
        let open = windows.values.filter { $0.object != nil }.count
        guard open != mainWindows else { return }
        mainWindows = open
        applyPolicy()
    }
    #endif

    // MARK: - Opening from outside the windows

    /// Opens an address from outside the windows (the Dock menu, the status
    /// menu, a quick action, a notification, Help) in the window in front,
    /// brought forward; with none, in a new window, which takes it as it
    /// becomes ready. The window's core says what the address opens.
    public func open(_ href: String, with action: OpenWindowAction? = nil) {
        guard let address = OpeningRules.address(href) else { return }
        if let action { keep(action) }
        switch OpeningRules.target(front: front, windows: order) {
        case .window(let scene):
            sessions[scene]?.object?.client?.send(.openLink(url: address))
            bringForward(scene)
        case .newWindow:
            pending = address
            #if os(macOS)
            holdsApp ? showStatusWindow() : newWindow()
            #endif
        }
    }

    /// Brings the main window forward, opening one when none is open.
    public func openMainWindow(with action: OpenWindowAction? = nil) {
        if let action { keep(action) }
        if holdsApp { return openInWindow() }
        switch OpeningRules.target(front: front, windows: order) {
        case .window(let scene): bringForward(scene)
        case .newWindow: newWindow()
        }
    }

    /// A new main window, as File > New Window opens one.
    public func newWindow(with action: OpenWindowAction? = nil) {
        if let action { keep(action) }
        #if os(macOS)
        // One session: the app held by its item comes into a window, never a second one.
        if holdsApp { return openInWindow() }
        NSApp.activate()
        if let openWindow {
            openWindow(id: NavreeScene.main)
        } else if let item = Self.item(titled: String(localized: "New Window"), in: NSApp.mainMenu),
            let menu = item.menu
        {
            menu.performActionForItem(at: menu.index(of: item))
        } else {
            Self.log.error("no way to open a main window was found")
        }
        #else
        openWindow?(id: NavreeScene.main)
        #endif
    }

    /// An entry of the person's menu chosen outside the windows (the status
    /// menu of an app in the menu bar only): carried out by the window in
    /// front's core, as the App menu's entry would be; with no window, the
    /// main window opens instead.
    func account(_ index: Int) {
        guard let user, user.menu.indices.contains(index), case .window(let scene) = OpeningRules.target(front: front, windows: order),
            let client = sessions[scene]?.object?.client
        else { return openMainWindow() }
        let now = Int(Date().timeIntervalSince1970)
        switch user.menu[index] {
        case .link(let link):
            client.send(.act(.init(screen: nil, action: link.action, ask: nil, now: now)))
        case .button(let button):
            guard let action = button.action else { return }
            client.send(.act(.init(screen: nil, action: action, ask: PresentationRules.question(button), now: now)))
        default:
            return
        }
        bringForward(scene)
    }

    /// The address waiting for the next window that becomes ready, taken.
    func takePending() -> String? {
        defer { pending = nil }
        return pending
    }

    private func bringForward(_ scene: String) {
        #if os(macOS)
        guard let window = windows[scene]?.object else {
            if holdsApp { showStatusWindow() }
            return
        }
        NSApp.activate()
        if window.isMiniaturized { window.deminiaturize(nil) }
        window.makeKeyAndOrderFront(nil)
        #endif
    }

    #if os(macOS)
    private static func item(titled title: String, in menu: NSMenu?) -> NSMenuItem? {
        for item in menu?.items ?? [] {
            if item.title == title { return item }
            if let found = self.item(titled: title, in: item.submenu) { return found }
        }
        return nil
    }
    #endif

    // MARK: - The window in front's destinations

    private func publish(_ destinations: [PresenceDestination], user: Navree.User?) {
        if self.destinations != destinations { self.destinations = destinations }
        if self.user != user { self.user = user }
        let followed = BadgeRules.destinationsChanged(badge, source: presence.badge, destinations: destinations)
        if followed != badge {
            badge = followed
            applyBadge()
        }
        applyQuickActions()
    }

    // MARK: - The count on the app's icon

    /// A server's `set_badge`: the count on the app's icon, or none.
    public func call(badge count: Int?) {
        badge = BadgeRules.called(badge, count: count)
        applyBadge()
    }

    /// The count, where the system shows it: the app's icon on the Home
    /// Screen, as the person's notification settings allow; the Dock icon on
    /// a Mac. The status item reads it from here.
    private func applyBadge() {
        guard Self.isApp else { return }
        #if canImport(UIKit)
        UNUserNotificationCenter.current().setBadgeCount(badge.count ?? 0) { error in
            if let error { Self.log.error("the badge was not set: \(error.localizedDescription, privacy: .public)") }
        }
        #elseif canImport(AppKit)
        NSApp.dockTile.badgeLabel = BadgeRules.label(badge.count)
        applyStatusItem()
        #endif
    }

    // MARK: - Quick actions

    /// The Home Screen's quick actions, from the shortcuts, given to the
    /// system when they change. An app without shortcuts never touches them.
    private func applyQuickActions() {
        #if os(iOS)
        guard Self.isApp, case .on = presence.shortcuts else { return }
        let actions = ShortcutRules.entries(source: presence.shortcuts, destinations: destinations).map(ShortcutRules.item)
        guard actions != quickActions else { return }
        quickActions = actions
        UIApplication.shared.shortcutItems = actions.map { action in
            UIApplicationShortcutItem(
                type: action.type, localizedTitle: action.title, localizedSubtitle: nil,
                icon: action.symbol.map { UIApplicationShortcutIcon(systemImageName: $0) },
                userInfo: ["href": action.href as NSString]
            )
        }
        #endif
    }
}

/// A reference that does not keep what it refers to.
struct Weak<Object: AnyObject> {
    weak var object: Object?

    init(_ object: Object) {
        self.object = object
    }
}
