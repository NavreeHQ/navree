#if os(macOS)
import AppKit
import NavreeDesign
import SwiftUI

/// The shell an item in the menu bar holds, told whether it stands in the
/// window under the item or in a full window.
struct HeldShell: View {
    let configuration: ShellConfiguration

    var body: some View {
        AppShell(configuration: configuration)
            .environment(\.navreeCompactWindow, PresenceModel.shared.shellUnderItem)
            // Any size the window is given: the hosting view otherwise shrinks
            // the window to a screen whose content has a size of its own (a
            // page the server could not find).
            .frame(minWidth: 0, maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
    }
}

/// The window an item in the menu bar opens with the whole app in it. It
/// takes the keyboard, so forms can be typed in, and holds the app's sheets
/// and questions as any window does. Escape closes it, as it closes the
/// system's own items' windows, once nothing in it takes the key first (a
/// field's words, a menu, a sheet's Cancel).
final class StatusPanel: NSPanel {
    /// What Escape does when nothing in the window took it.
    var escape: (@MainActor () -> Void)?

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }

    override func cancelOperation(_ sender: Any?) {
        if let escape { escape() } else { super.cancelOperation(sender) }
    }
}

/// An item in the menu bar holding the whole app, for an app in the menu bar
/// only: the system's `NSStatusItem` and, under it, a panel showing the app's
/// one shell and session, as the system's own items open their windows (no
/// arrow, the window's own material, closed by a click elsewhere unless
/// pinned). A right click opens the system's menu: the app's destinations,
/// the person's menu, Open in Window, Settings…, About and Quit. Open in
/// Window moves the same shell, its session and what it shows, into a full
/// window, with the app in the Dock while it is open; closing that window
/// puts the app back under its item.
@MainActor
final class StatusPanelController: NSObject, NSWindowDelegate {
    private(set) var item: NSStatusItem?
    private(set) var panel: StatusPanel?
    /// The full window, while the person has asked for one.
    private(set) var window: NSWindow?
    /// The app's shell, made once.
    private var app: NSViewController?
    /// The whole configuration the shell is made from.
    var configuration: ShellConfiguration?
    private var visibility: NSKeyValueObservation?
    private var monitors: [Any] = []
    private var watching: [any NSObjectProtocol] = []
    private var openedAtLaunch = false
    /// Whether the panel is open.
    private(set) var shown = false

    /// The panel's options, as the configuration says.
    private var options: StatusWindow {
        if case .app(let options)? = PresenceModel.shared.presence.statusItem?.content { options } else { StatusWindow() }
    }

    /// Puts the item in the menu bar or takes it out, as the place says, draws
    /// its button, starts the app's shell with the app, and opens the panel at
    /// launch when the app asks.
    func apply(_ model: PresenceModel) {
        let wanted = StatusRules.inserted(style: .window, item: model.presence.statusItem, place: model.place)
        guard wanted else {
            hide()
            return remove()
        }
        let item = self.item ?? make()
        StatusButton.behave(item, model)
        StatusButton.draw(item, model)
        if !openedAtLaunch {
            openedAtLaunch = true
            prepare()
            if options.opensAtLaunch { show() }
        }
    }

    /// The app's shell made at launch and laid out in the panel, which stays
    /// hidden: its core starts with the app, so what the server says before
    /// the person first opens the panel (the count beside the item) arrives,
    /// and the panel opens on the app already there. No window shows and the
    /// app stays out of the Dock.
    private func prepare() {
        guard let shell = shell() else { return }
        let panel = self.panel ?? makePanel()
        if panel.contentViewController == nil { panel.contentViewController = shell }
        panel.setFrame(CGRect(origin: panel.frame.origin, size: options.size), display: false)
        keepSmallest(panel)
        shell.view.layoutSubtreeIfNeeded()
    }

    private func make() -> NSStatusItem {
        let item = StatusButton.make()
        item.button?.target = self
        item.button?.action = #selector(clicked(_:))
        item.button?.sendAction(on: [.leftMouseUp, .rightMouseUp])
        visibility = StatusButton.watch(item)
        self.item = item
        return item
    }

    private func remove() {
        visibility = nil
        if let item { NSStatusBar.system.removeStatusItem(item) }
        item = nil
    }

    /// A left click opens or closes the panel; a right click, or a click with
    /// Control held, opens the menu.
    @objc private func clicked(_ sender: Any?) {
        let event = NSApp.currentEvent
        if event?.type == .rightMouseUp || event?.modifierFlags.contains(.control) == true {
            popUpMenu()
        } else {
            shown ? hide() : show()
        }
    }

    /// The right click's menu, as the system shows an item's menu.
    private func popUpMenu() {
        guard let item else { return }
        hide()
        item.menu = menu()
        item.button?.performClick(nil)
        item.menu = nil
    }

    /// The right click's menu, drawn from what the app's shell holds now.
    func menu() -> NSMenu {
        let model = PresenceModel.shared
        let menu = NSMenu()
        StatusMenus.fill(menu, StatusRules.contextMenu(
            destinations: model.destinations, appName: PresenceModel.appName, user: model.user
        ))
        return menu
    }

    // MARK: - The panel

    /// Opens the panel under the item; with the app in its own window, that
    /// window comes forward instead.
    func show() {
        if let window, window.isVisible {
            NSApp.activate()
            return window.makeKeyAndOrderFront(nil)
        }
        guard let button = item?.button, let place = button.window, let shell = shell() else { return }
        let panel = self.panel ?? makePanel()
        if panel.contentViewController == nil {
            panel.contentViewController = shell
            keepSmallest(panel)
        }
        let screen = (place.screen ?? NSScreen.main)?.visibleFrame ?? .zero
        // A panel the person may resize opens at the size they left it.
        let size = options.resizable && panel.frame.width > 1 ? panel.frame.size : options.size
        panel.setFrame(StatusRules.windowFrame(item: place.frame, size: size, screen: screen), display: true)
        NSApp.activate()
        panel.makeKeyAndOrderFront(nil)
        shown = true
        pressed(true)
        PresenceModel.shared.statusWindowChanged(shown: true)
        watchOutside()
    }

    /// The item shown pressed while the panel is open, as the system's own
    /// items show theirs. A click's own tracking lets the button go when it
    /// ends, after its action has run, so the pressed look is set again once
    /// that is over.
    private func pressed(_ on: Bool) {
        item?.button?.highlight(on)
        guard on else { return }
        DispatchQueue.main.async { [weak self] in
            MainActor.assumeIsolated {
                guard let self, self.shown else { return }
                self.item?.button?.highlight(true)
            }
        }
    }

    /// Closes the panel; the app's shell and its session stay as they were,
    /// so it opens again where the person left it.
    func hide() {
        guard shown else { return }
        unwatch()
        panel?.orderOut(nil)
        shown = false
        pressed(false)
        PresenceModel.shared.statusWindowChanged(shown: false)
    }

    /// Closes the panel for what happened outside it (a click elsewhere, the
    /// app going to the background), unless one of the system's own sheets
    /// stands over it (signing in): that sheet answers to the panel, and
    /// closing the panel under it would leave it nowhere.
    private func hideForOutside() {
        guard PresenceModel.shared.systemSheets == 0 else { return }
        hide()
    }

    /// The window the system's own sheets (signing in) stand over while the
    /// app is held by its item: its full window when the person opened one,
    /// else the panel, opened for it so the person sees what it asks.
    func windowForSystemSheets() -> NSWindow? {
        if let window, window.isVisible { return window }
        show()
        return shown ? panel : nil
    }

    /// The app's shell, made once: one session for the process, wherever it shows.
    private func shell() -> NSViewController? {
        if let app { return app }
        guard let configuration else { return nil }
        let hosting = NSHostingController(rootView: HeldShell(configuration: configuration))
        // The window keeps the size it is given; the shell's toolbar and title are the window's.
        hosting.sizingOptions = []
        hosting.sceneBridgingOptions = .all
        app = hosting
        return hosting
    }

    private func makePanel() -> StatusPanel {
        let options = self.options
        var style: NSWindow.StyleMask = [.titled, .fullSizeContentView]
        if options.resizable { style.insert(.resizable) }
        let panel = StatusPanel(contentRect: CGRect(origin: .zero, size: options.size), styleMask: style, backing: .buffered, defer: true)
        // No window buttons, as the system's own items' windows; the bar holds
        // the screen's title and the shell's toolbar, since the window holds a
        // whole app.
        panel.titlebarAppearsTransparent = true
        for button in [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton] {
            panel.standardWindowButton(button)?.isHidden = true
        }
        panel.title = PresenceModel.appName
        panel.toolbarStyle = .unifiedCompact
        panel.isMovable = false
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        panel.level = options.pinned ? .floating : .statusBar
        // As the system's own items' windows: on the space in use, beside an
        // app in full screen, never in the Window menu or among the windows
        // ⌘` steps through.
        panel.collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary, .ignoresCycle]
        panel.isExcludedFromWindowsMenu = true
        panel.minSize = options.minimumSize
        panel.animationBehavior = .utilityWindow
        panel.escape = { [weak self] in self?.hide() }
        self.panel = panel
        return panel
    }

    /// The smallest the person may make the panel, as its options say. The
    /// hosted shell sets the window's smallest size anew as it lays out, so
    /// the panel's delegate holds it too, as the person drags its edges.
    private func keepSmallest(_ panel: StatusPanel) {
        panel.minSize = options.minimumSize
        panel.delegate = self
    }

    /// The person's drag stops at the panel's smallest size, and at the full
    /// window's smallest content.
    func windowWillResize(_ sender: NSWindow, to frameSize: NSSize) -> NSSize {
        let smallest: CGSize
        if sender === panel {
            smallest = options.minimumSize
        } else if sender === window {
            let content = configuration?.windows.minimumSize ?? CGSize(width: 480, height: 400)
            smallest = sender.frameRect(forContentRect: CGRect(origin: .zero, size: content)).size
        } else {
            return frameSize
        }
        return NSSize(width: max(frameSize.width, smallest.width), height: max(frameSize.height, smallest.height))
    }

    /// Unless pinned, a click anywhere but the panel, its sheets, its menus and
    /// the item closes it, as does the app going to the background.
    private func watchOutside() {
        unwatch()
        guard !options.pinned else { return }
        let elsewhere = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] _ in
            MainActor.assumeIsolated { self?.hideForOutside() }
        }
        let inside = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] event in
            let window = event.window
            MainActor.assumeIsolated { self?.clicked(in: window) }
            return event
        }
        monitors = [elsewhere, inside].compactMap(\.self)
        watching = [
            NotificationCenter.default.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.hideForOutside() }
            },
        ]
    }

    private func clicked(in window: NSWindow?) {
        guard let panel, let window else { return }
        if window === panel || window.sheetParent === panel || window === item?.button?.window || window.level >= .popUpMenu { return }
        hideForOutside()
    }

    private func unwatch() {
        for monitor in monitors { NSEvent.removeMonitor(monitor) }
        monitors = []
        for token in watching { NotificationCenter.default.removeObserver(token) }
        watching = []
    }

    // MARK: - Open in Window

    /// The same shell in a full window, the panel closed.
    func openInWindow() {
        guard let shell = shell() else { return }
        hide()
        let window = self.window ?? makeWindow()
        if window.contentViewController !== shell {
            panel?.contentViewController = nil
            window.contentViewController = shell
            // The window keeps its own size and place, not the panel's.
            if !window.setFrameUsingName(Self.windowName) {
                window.setContentSize(configuration?.windows.idealSize ?? CGSize(width: 960, height: 680))
                window.center()
            }
            window.setFrameAutosaveName(Self.windowName)
        }
        PresenceModel.shared.shellUnderItem = false
        NSApp.activate()
        window.makeKeyAndOrderFront(nil)
    }

    private func makeWindow() -> NSWindow {
        let size = configuration?.windows.idealSize ?? CGSize(width: 960, height: 680)
        let window = NSWindow(
            contentRect: CGRect(origin: .zero, size: size),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered, defer: true
        )
        window.title = PresenceModel.appName
        window.isReleasedWhenClosed = false
        window.contentMinSize = configuration?.windows.minimumSize ?? CGSize(width: 480, height: 400)
        window.delegate = self
        self.window = window
        return window
    }

    static let windowName = "navree.status.window"

    /// The full window closed: the shell goes back under the item.
    func windowWillClose(_ notification: Notification) {
        guard let window, notification.object as? NSWindow === window else { return }
        window.contentViewController = nil
        panel?.contentViewController = app
        PresenceModel.shared.shellUnderItem = true
    }
}
#endif
