import AppKit
import NavreeShell

/// The Mac demo's picture steps about where the app stands outside its
/// windows, written to the log as the system answers:
///
/// - `presence`: the activation policy, the Info.plist's `LSUIElement`, the
///   type the system lists the app as (`lsappinfo`), the place, the item in
///   the menu bar the app made, every status bar window of the app with its
///   frame, what AppKit keeps in the defaults of any item, and the main windows;
/// - `place:<dock|menuBar|both>`: chooses the place as Settings does;
/// - `badge:<n|none>`: sets the count as a server's `set_badge` does, then the
///   Dock tile's badge;
/// - `dockmenu`: the Dock menu the app's delegate gives the Dock;
/// - `dockopen:<item>`, `statuschoose:<item>`: that item of the Dock menu or
///   of the item's menu chosen, then the window in front's title;
/// - `reopen`: the app opened again with no window showing, as the Finder does;
/// - `close`: the window in front closed, as its close button does, then `presence`;
/// - `panel:<name>`: the window under an item holding the whole app, opened
///   as a click opens it and pictured as the window server composes it, as
///   `<name>-light.png` and `<name>-dark.png`, with a sheet up in it alone
///   (`<name>-sheet-…`) and with the panel (`<name>-whole-…`), and the
///   sheet's frame against the panel's; `hidepanel` closes it;
/// - `statusclick`, `statusclick:right`: a click on the item's button as the
///   mouse sends it, then whether the panel is open and the button pressed;
///   for a right click, the menu that opened (closed again a moment later);
/// - `keypress:<escape|return>`: that key sent to the key window, then the
///   key window and the panel's state; `panelstate`: that state alone;
/// - `panelsize:<width>x<height>`: the open panel made that size, its top kept;
/// - `deactivate`: the app sent to the background (the Finder brought
///   forward), then the panel's state;
/// - `openinwindow`, `closewindow`: that app in a full window and back, each
///   then `presence`;
/// - `typetext:<words>`: words typed into the first field of the key window;
/// - `menuimages:<App|Menu>`: each item of a menu with the description of its image;
/// - `status:<name>`: the item's button in the menu bar as `<name>.png` (on the
///   menu bar's colour for the app's appearance), its words and its name for VoiceOver;
/// - `statusmenu`: the item's menu, drawn as it is when it opens;
/// - `about:<name>`: App > About, the panel as `<name>-light.png` and
///   `<name>-dark.png`, and its words;
/// - `login:<on|off|status>`: opening at login turned on or off as the
///   General pane does, and what the system says of it.
///
/// None of them walks the accessibility tree.
@MainActor
enum PresenceSteps {
    /// The lines the step writes to the log, or nil for a step it does not know.
    static func run(_ step: String, _ argument: String, out: URL) async -> [String]? {
        let model = PresenceModel.shared
        switch step {
        case "presence":
            return presence()
        case "place":
            guard let place = MacPlace(rawValue: argument) else { return ["place \(argument): not a place"] }
            model.choose(place)
            try? await Task.sleep(for: .seconds(1))
            return ["place \(argument): chosen"] + presence()
        case "badge":
            model.call(badge: Int(argument))
            try? await Task.sleep(for: .milliseconds(300))
            return ["badge \(argument): count \(model.badge.count.map(String.init) ?? "none"), Dock tile \"\(NSApp.dockTile.badgeLabel ?? "")\""]
        case "dockmenu":
            guard let menu = NSApp.delegate?.applicationDockMenu?(NSApp), !menu.items.isEmpty else {
                return ["dockmenu: no items of the app's own (the system's items only)"]
            }
            return ["dockmenu:"] + menu.items.map { item in
                item.isSeparatorItem ? "  ---" : "  \(item.title)\(item.isEnabled ? "" : " (disabled)")"
            }
        case "dockopen", "statuschoose":
            // An item of the Dock menu or of the item's menu chosen, as a click chooses it.
            let menu = step == "dockopen" ? NSApp.delegate?.applicationDockMenu?(NSApp) : model.statusItemParts?.menu
            guard let menu, let index = menu.items.firstIndex(where: { $0.title == argument }) else {
                return ["\(step) \(argument): no such item"]
            }
            let before = WindowPictures.windows.first?.title ?? "no window"
            menu.performActionForItem(at: index)
            try? await Task.sleep(for: .seconds(4))
            return ["\(step) \(argument): window \"\(before)\" became \"\(WindowPictures.windows.first?.title ?? "no window")\", \(WindowPictures.windows.count) windows showing, main windows \(model.mainWindows)"]
        case "reopen":
            let before = WindowPictures.windows.count
            let handled = NSApp.delegate?.applicationShouldHandleReopen?(NSApp, hasVisibleWindows: before > 0) ?? true
            try? await Task.sleep(for: .seconds(2))
            return ["reopen with \(before) windows showing: left to the system \(handled), now \(WindowPictures.windows.count) windows, main windows \(model.mainWindows)"]
        case "menuimages":
            // Each item of a menu of the menu bar, with the symbol its image is.
            guard let menu = NSApp.mainMenu?.items.first(where: { $0.title == argument || (argument == "App" && $0 == NSApp.mainMenu?.items.first) })?.submenu
            else { return ["menuimages \(argument): no such menu"] }
            menu.update()
            return ["menuimages \(argument):"] + menu.items.filter { !$0.isSeparatorItem }.map { item in
                let image = item.image.map { "\($0.name() ?? "unnamed") \"\($0.accessibilityDescription ?? "")\"" } ?? "no image"
                return "  \(item.title): \(image)"
            }
        case "panel":
            // The window under the item, opened as a click opens it, pictured as
            // the window server composes it, in light and in dark.
            model.showStatusWindow()
            try? await Task.sleep(for: .seconds(4))
            guard let panel = model.statusWindows.panel, panel.isVisible else { return ["panel \(argument): no window under the item"] }
            var lines = ["panel \(argument): \(Int(panel.frame.width))x\(Int(panel.frame.height)) at \(NSStringFromRect(panel.frame)), under the item at \(model.statusItemParts?.button?.window.map { NSStringFromRect($0.frame) } ?? "none"), key \(panel.isKeyWindow), level \(panel.level.rawValue), main windows \(model.mainWindows)"]
            if let sheet = panel.attachedSheet {
                lines.append("  sheet \(Int(sheet.frame.width))x\(Int(sheet.frame.height)) at \(NSStringFromRect(sheet.frame)), \(Self.overhang(sheet.frame, panel.frame))")
            }
            for look in ["light", "dark"] {
                NSApp.appearance = NSAppearance(named: look == "dark" ? .darkAqua : .aqua)
                try? await Task.sleep(for: .seconds(1.5))
                lines.append("  " + composed(panel, name: "\(argument)-\(look)", out: out))
                // A sheet up in it is a window of its own: pictured alone, and
                // with the panel as the window server composes the two.
                if let sheet = panel.attachedSheet {
                    lines.append("  sheet: " + composed(sheet, name: "\(argument)-sheet-\(look)", out: out))
                    lines.append("  together: " + composed([sheet, panel], name: "\(argument)-whole-\(look)", out: out))
                }
            }
            NSApp.appearance = nil
            return lines
        case "statusclick":
            return await statusClick(right: argument == "right")
        case "keypress":
            // A key as the keyboard sends it to the key window, through the app's event queue.
            let before = NSApp.keyWindow
            guard let window = before, let key = Self.keys[argument] else { return ["keypress \(argument): no key window or no such key"] }
            for type in [NSEvent.EventType.keyDown, .keyUp] {
                if let event = NSEvent.keyEvent(
                    with: type, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                    windowNumber: window.windowNumber, context: nil, characters: key.characters,
                    charactersIgnoringModifiers: key.characters, isARepeat: false, keyCode: key.code
                ) {
                    NSApp.postEvent(event, atStart: false)
                }
            }
            try? await Task.sleep(for: .seconds(2))
            return ["keypress \(argument) in \(Self.describe(before)): key window now \(Self.describe(NSApp.keyWindow)), " + panelState()]
        case "deactivate":
            // The app sent to the background as a click in another app's window
            // sends it: the Finder brought forward, the app yielding to it.
            let finder = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.finder").first
            NSApp.yieldActivation(toApplicationWithBundleIdentifier: "com.apple.finder")
            let asked = finder?.activate(from: .current, options: []) ?? false
            try? await Task.sleep(for: .seconds(2))
            return ["deactivate: the Finder asked to come forward \(asked), app now \(NSApp.isActive ? "active" : "not active"), " + panelState()]
        case "panelstate":
            return ["panelstate: " + panelState()]
        case "panelsize":
            // The panel made that size, as a person drags its edge, its top kept.
            let numbers = argument.split(separator: "x").compactMap { Double($0) }
            guard numbers.count == 2, let panel = model.statusWindows.panel, panel.isVisible else { return ["panelsize \(argument): no panel open"] }
            var frame = panel.frame
            frame.origin.y += frame.height - numbers[1]
            frame.size = CGSize(width: numbers[0], height: numbers[1])
            panel.setFrame(frame, display: true)
            try? await Task.sleep(for: .seconds(1))
            return ["panelsize \(argument): \(Int(panel.frame.width))x\(Int(panel.frame.height)), smallest \(Int(panel.contentMinSize.width))x\(Int(panel.contentMinSize.height)), resizable \(panel.styleMask.contains(.resizable))"]
        case "composedwindow":
            // The app's full window, as the window server composes it.
            guard let window = model.statusWindows.window, window.isVisible else { return ["composedwindow \(argument): no full window"] }
            return ["composedwindow: " + composed(window, name: argument, out: out)]
        case "hidepanel":
            model.hideStatusWindow()
            try? await Task.sleep(for: .seconds(1))
            return ["hidepanel: shown \(model.statusWindowShown), panel visible \(model.statusWindows.panel?.isVisible ?? false)"]
        case "openinwindow":
            model.openInWindow()
            try? await Task.sleep(for: .seconds(3))
            let window = model.statusWindows.window
            return ["openinwindow: window \"\(window?.title ?? "none")\" \(window.map { "\(Int($0.frame.width))x\(Int($0.frame.height))" } ?? ""), panel visible \(model.statusWindows.panel?.isVisible ?? false)"] + presence()
        case "closewindow":
            model.statusWindows.window?.performClose(nil)
            try? await Task.sleep(for: .seconds(2))
            return ["closewindow: window visible \(model.statusWindows.window?.isVisible ?? false)"] + presence()
        case "typetext":
            // Words typed into the first field of the key window, as the keyboard types them.
            guard let window = NSApp.keyWindow, let field = Self.field(in: window.contentView) else {
                return ["typetext \(argument): no field in the key window \"\(NSApp.keyWindow?.title ?? "none")\""]
            }
            window.makeFirstResponder(field)
            let editor = window.firstResponder as? NSTextView
            editor?.selectAll(nil)
            editor?.insertText(argument, replacementRange: editor?.selectedRange() ?? NSRange(location: 0, length: 0))
            try? await Task.sleep(for: .seconds(1))
            return ["typetext \(argument): into \"\(field.placeholderString ?? field.accessibilityLabel() ?? "a field")\" of \"\(window.title)\", now \"\(field.stringValue)\""]
        case "close":
            let before = WindowPictures.windows.count
            WindowPictures.windows.first?.performClose(nil)
            try? await Task.sleep(for: .seconds(1.5))
            return ["close: \(before) windows showing, now \(WindowPictures.windows.count), main windows \(model.mainWindows)"] + presence()
        case "status":
            return status(argument.isEmpty ? "status" : argument, out: out)
        case "statusmenu":
            // The item's own menu, drawn as it is when it opens.
            guard let menu = model.statusItemParts?.menu else { return ["statusmenu: no item in the menu bar"] }
            return ["statusmenu:"] + menu.items.map { item in
                if item.isSeparatorItem { return "  ---" }
                if item.isSectionHeader { return "  [\(item.title)]" }
                var line = "  \(item.title)"
                if !item.keyEquivalent.isEmpty { line += " ⌘\(item.keyEquivalent.uppercased())" }
                if let image = item.image { line += " [\(image.accessibilityDescription ?? "image")]" }
                if let badge = item.badge { line += " (\(badge.stringValue ?? ""))" }
                if let href = item.representedObject as? String { line += " → \(href)" }
                return line
            }
        case "about":
            return await about(argument.isEmpty ? "about" : argument, out: out)
        case "login":
            var lines: [String] = []
            if argument == "on" || argument == "off" {
                let problem = LoginItem.set(argument == "on")
                lines.append("login \(argument): \(problem.map { "refused: \($0)" } ?? "done")")
            }
            lines.append("login status \(LoginItem.status.rawValue)")
            return lines
        default:
            return nil
        }
    }

    /// The item's button as the app draws it, without the menu bar's own
    /// material, which the window server blends.
    static func status(_ name: String, out: URL) -> [String] {
        guard let button = PresenceModel.shared.statusItemParts?.button else { return ["status \(name): no item in the menu bar"] }
        let view: NSView = button
        let appearance = NSApp.appearance ?? view.effectiveAppearance
        view.appearance = appearance
        defer { view.appearance = nil }
        view.layoutSubtreeIfNeeded()
        var line = "status \(name): \(Int(view.bounds.width))x\(Int(view.bounds.height)) points, appearance \(appearance.name.rawValue)"
        line += ", title \"\(button.title)\", image \(button.image.map { $0.isTemplate ? "template" : "colour" } ?? "none")"
        line += ", accessibility label \"\(button.accessibilityLabel() ?? "")\""
        line += ", image description \"\(button.image?.accessibilityDescription ?? "")\""
        // Four pixels a point, to read it.
        guard let bitmap = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(view.bounds.width) * 4, pixelsHigh: Int(view.bounds.height) * 4,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
            bytesPerRow: 0, bitsPerPixel: 0
        ) else { return [line] }
        bitmap.size = view.bounds.size
        view.cacheDisplay(in: view.bounds, to: bitmap)
        // On a plain ground of the appearance, so a light template shows: the
        // menu bar's own material is the window server's.
        let dark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        let ground = NSColor(white: dark ? 0.16 : 0.96, alpha: 1)
        let picture = NSImage(size: NSSize(width: bitmap.pixelsWide, height: bitmap.pixelsHigh), flipped: false) { rect in
            ground.setFill()
            rect.fill()
            bitmap.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: false, hints: nil)
            return true
        }
        guard let tiff = picture.tiffRepresentation, let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:])
        else { return [line] }
        try? png.write(to: out.appendingPathComponent("\(name).png"))
        return [line + ", pictured \(name).png"]
    }

    /// Keys a step can press, with their characters and key codes.
    static let keys: [String: (characters: String, code: UInt16)] = [
        "escape": ("\u{1b}", 53), "return": ("\r", 36),
    ]

    /// How far a sheet stands outside the window it is on, or that it fits.
    static func overhang(_ sheet: CGRect, _ window: CGRect) -> String {
        let leading = max(window.minX - sheet.minX, 0), trailing = max(sheet.maxX - window.maxX, 0)
        let below = max(window.minY - sheet.minY, 0)
        if leading == 0, trailing == 0, below == 0 { return "inside the panel" }
        return "outside the panel by \(Int(leading)) leading, \(Int(trailing)) trailing, \(Int(below)) below"
    }

    static func describe(_ window: NSWindow?) -> String {
        guard let window else { return "none" }
        let kind = window is NSPanel && window.sheetParent == nil ? "panel" : window.sheetParent != nil ? "sheet" : "window"
        return "\(kind) \"\(window.title)\""
    }

    /// Whether the window under the item is open, its sheet, and how the item's button looks.
    static func panelState() -> String {
        let model = PresenceModel.shared
        let panel = model.statusWindows.panel
        let button = model.statusItemParts?.button
        return "panel shown \(model.statusWindowShown), visible \(panel?.isVisible ?? false), sheet \(panel?.attachedSheet.map { "up \"\($0.title)\"" } ?? "none"), item pressed \(button?.isHighlighted ?? false)"
    }

    /// A click on the item's button, as the mouse sends it, through the app's
    /// event queue to the item's own window: a left click, or a right click,
    /// whose menu is written down as it opens and then closed.
    static func statusClick(right: Bool) async -> [String] {
        guard let button = PresenceModel.shared.statusItemParts?.button, let window = button.window else {
            return ["statusclick: no item in the menu bar"]
        }
        let middle = button.convert(NSPoint(x: button.bounds.midX, y: button.bounds.midY), to: nil)
        let types: [NSEvent.EventType] = right ? [.rightMouseDown, .rightMouseUp] : [.leftMouseDown, .leftMouseUp]
        let events = types.compactMap { type in
            NSEvent.mouseEvent(
                with: type, location: middle, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: type == types[0] ? 1 : 0
            )
        }
        let opened = OpenedMenu()
        let watching = right ? NotificationCenter.default.addObserver(forName: NSMenu.didBeginTrackingNotification, object: nil, queue: nil) { note in
            nonisolated(unsafe) let menu = note.object as? NSMenu
            MainActor.assumeIsolated {
                guard let menu else { return }
                opened.items = menu.items.map { item in
                    item.isSeparatorItem ? "---" : item.isSectionHeader ? "[\(item.title)]" : item.title
                }
                // Closed as the person would close it, a moment after it opened,
                // from the menu's own run loop.
                menu.perform(#selector(NSMenu.cancelTracking), with: nil, afterDelay: 1.5, inModes: [.common])
            }
        } : nil
        defer { if let watching { NotificationCenter.default.removeObserver(watching) } }
        let before = panelState()
        events.forEach { NSApp.postEvent($0, atStart: false) }
        try? await Task.sleep(for: .seconds(right ? 3 : 2))
        var lines = ["statusclick\(right ? ":right" : "") (before: \(before)): " + panelState()]
        if right { lines.append("  menu that opened: " + (opened.items.isEmpty ? "none" : opened.items.joined(separator: " | "))) }
        return lines
    }

    final class OpenedMenu {
        var items: [String] = []
    }

    /// Windows as the window server composes them together, each in its place,
    /// the first in front.
    static func composed(_ windows: [NSWindow], name: String, out: URL) -> String {
        typealias Capture = @convention(c) (CGRect, CFArray, UInt32) -> Unmanaged<CGImage>?
        guard let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImageFromArray") else {
            return "\(name): the window server cannot picture windows here"
        }
        let capture = unsafeBitCast(symbol, to: Capture.self)
        // The windows' numbers as the array's own values, not as objects.
        var values = windows.map { UnsafeRawPointer(bitPattern: UInt($0.windowNumber)) }
        guard let numbers = CFArrayCreate(kCFAllocatorDefault, &values, values.count, nil) else { return "\(name): no picture" }
        // Their frames' shadows left out, at the screen's scale.
        guard let image = capture(.null, numbers, (1 << 0) | (1 << 3))?.takeRetainedValue(),
            let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
        else { return "\(name): no picture" }
        try? png.write(to: out.appendingPathComponent("\(name).png"))
        return "pictured \(name).png \(image.width)x\(image.height) pixels"
    }

    /// A window as the window server composes it, its material included: an
    /// app may picture its own windows with no screen recording.
    static func composed(_ window: NSWindow, name: String, out: URL) -> String {
        typealias Capture = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?
        guard let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImage") else {
            return "\(name): the window server cannot picture a window here"
        }
        let capture = unsafeBitCast(symbol, to: Capture.self)
        // Only this window, its frame's shadow left out, at the screen's scale.
        guard let image = capture(.null, 1 << 3, UInt32(window.windowNumber), (1 << 0) | (1 << 3))?.takeRetainedValue(),
            let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
        else { return "\(name): no picture" }
        try? png.write(to: out.appendingPathComponent("\(name).png"))
        return "pictured \(name).png \(image.width)x\(image.height) pixels"
    }

    /// The first field a person can type in.
    static func field(in view: NSView?) -> NSTextField? {
        guard let view else { return nil }
        if let field = view as? NSTextField, field.isEditable, !field.isHidden { return field }
        for child in view.subviews {
            if let found = field(in: child) { return found }
        }
        return nil
    }

    /// App > About chosen from the menu bar, the panel pictured and closed.
    static func about(_ name: String, out: URL) async -> [String] {
        let appMenu = NSApp.mainMenu?.items.first?.submenu
        guard let menu = appMenu, let index = menu.items.firstIndex(where: { $0.title.hasPrefix("About") }) else {
            return ["about \(name): no About item"]
        }
        let title = menu.items[index].title
        let before = Set(NSApp.windows.map(ObjectIdentifier.init))
        menu.performActionForItem(at: index)
        try? await Task.sleep(for: .seconds(1.5))
        guard let panel = NSApp.windows.first(where: { $0.isVisible && !before.contains(ObjectIdentifier($0)) })
            ?? NSApp.windows.first(where: { $0.isVisible && $0 is NSPanel })
        else { return ["about \(name): \"\(title)\" chosen, no panel came"] }
        var lines = ["about \(name): \"\(title)\" chosen, panel \(Int(panel.frame.width))x\(Int(panel.frame.height))"]
        lines += texts(in: panel.contentView).map { "  \($0)" }
        // In each appearance, the panel's own.
        for (look, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            panel.appearance = NSAppearance(named: appearance)
            guard let frame = panel.contentView?.superview ?? panel.contentView else { break }
            frame.layoutSubtreeIfNeeded()
            frame.display()
            guard let bitmap = frame.bitmapImageRepForCachingDisplay(in: frame.bounds) else { continue }
            frame.cacheDisplay(in: frame.bounds, to: bitmap)
            if let png = bitmap.representation(using: .png, properties: [:]) {
                try? png.write(to: out.appendingPathComponent("\(name)-\(look).png"))
                lines.append("  pictured \(name)-\(look).png")
            }
        }
        panel.appearance = nil
        panel.close()
        return lines
    }

    /// The words of a window's text fields and text views, in the views' order.
    static func texts(in view: NSView?) -> [String] {
        guard let view else { return [] }
        var found: [String] = []
        if let field = view as? NSTextField, !field.stringValue.isEmpty { found.append(field.stringValue) }
        if let text = view as? NSTextView, !text.string.isEmpty {
            var links: [String] = []
            text.textStorage?.enumerateAttribute(.link, in: NSRange(location: 0, length: text.textStorage?.length ?? 0)) { value, _, _ in
                if let value { links.append("\(value)") }
            }
            found.append(text.string + (links.isEmpty ? "" : " (links: \(links.joined(separator: ", ")))"))
        }
        for child in view.subviews { found += texts(in: child) }
        return found
    }

    /// Where the app stands, as the app and the system say.
    static func presence() -> [String] {
        let model = PresenceModel.shared
        let policy = switch NSApp.activationPolicy() {
        case .regular: "regular"
        case .accessory: "accessory"
        case .prohibited: "prohibited"
        @unknown default: "unknown"
        }
        let uiElement = Bundle.main.object(forInfoDictionaryKey: "LSUIElement").map { "\($0)" } ?? "absent"
        let item = model.presence.statusItem
        let inserted = StatusRules.inserted(style: StatusRules.style(item) ?? .menu, item: item, place: model.place)
        // Every status bar window of the app, and the items the app made:
        // AppKit lists no app's items, so the one the library makes is read
        // from the library; what AppKit keeps of any item made is in the defaults.
        let statusWindows = NSApp.windows.filter { String(describing: type(of: $0)).contains("StatusBar") }
            .map { "\(type(of: $0)) \(NSStringFromRect($0.frame))\($0.isVisible ? "" : " hidden")" }
        let kept = UserDefaults.standard.dictionaryRepresentation().keys.filter { $0.hasPrefix("NSStatusItem") }.sorted()
        let made = model.statusItemParts.map { "\(Int($0.button?.bounds.width ?? 0))x\(Int($0.button?.bounds.height ?? 0)) points" } ?? "none"
        return [
            "presence: activation policy \(policy), LSUIElement \(uiElement), lsappinfo type \(listedType())",
            "  place \(model.place.rawValue) (build \(model.presence.place.rawValue), chosen \(model.chosen?.rawValue ?? "none"), offered \(PresenceRules.choices(model.presence).map(\.rawValue)))",
            "  status item \(item == nil ? "none configured" : "configured, inserted \(inserted)"); item made: \(made)",
            "  status bar windows \(statusWindows.isEmpty ? "none" : statusWindows.joined(separator: ", ")); kept in the defaults: \(kept.isEmpty ? "nothing" : kept.joined(separator: ", "))",
            "  main windows \(model.mainWindows), windows showing \(WindowPictures.windows.count), app \(NSApp.isActive ? "active" : "not active")",
        ]
    }

    /// The type the system's launch services list the app as: `Foreground`
    /// for an app in the Dock, `UIElement` for one that is not.
    static func listedType() -> String {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/lsappinfo")
        process.arguments = ["info", "-only", "applicationtype", String(ProcessInfo.processInfo.processIdentifier)]
        let pipe = Pipe()
        process.standardOutput = pipe
        guard (try? process.run()) != nil else { return "unknown" }
        process.waitUntilExit()
        let text = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        guard let range = text.range(of: #"type="[^"]*""#, options: .regularExpression) else { return "unknown" }
        return String(text[range].dropFirst(6).dropLast())
    }
}
