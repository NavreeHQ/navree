import AppKit
import SwiftUI

/// A record of how the Mac demo looks and what its menu bar holds, taken by
/// the app itself, so no screen recording is needed:
///
///     Charkha -NavreePictures <folder> -NavreeSteps "wait:4;menus;picture:orders;key:cmd-2;wait:2;picture:inventory;quit"
///
/// Steps, in order: `wait:<seconds>`; `size:<width>x<height>` (the window in
/// front); `menus` (every menu, its items and their shortcuts, to
/// `menus.txt`); `menu:<Menu>/<Item>` (chooses that command);
/// `key:<cmd|opt|shift>-...-<key>` (presses that shortcut on the menu bar,
/// as the keyboard would); `picture:<name>` (each window as `<name>-<n>.png`);
/// `composed:<name>` (the window in front as the window server composes it,
/// as `<name>.png`, with the glass and the floating sidebar that `picture`
/// leaves out);
/// `views:<name>` (the window's views, their appearance and materials);
/// `toolbar:<name>` (the toolbar's items, their words and the column each
/// stands over);
/// `elements:<name>` (every element the window says to VoiceOver, its role,
/// its words and its frame in the window, in points from its top leading
/// corner: where each word and control stands);
/// `click:<x>,<y>` or `click:<words>` (a click of the mouse at that point of
/// the window in front, or in the middle of the first element whose words
/// are those, else hold them, as a person's click comes: through the app's
/// event queue);
/// `choose:<row>` (a row of the window's table chosen through the table's
/// own selection, as a click chooses it);
/// `scroll:<points>` (the content under the window's middle scrolled down by
/// that many points, up when negative);
/// `appearance:<dark|light|system>`; `quit`.
struct DemoPictures: ViewModifier {
    func body(content: Content) -> some View {
        content.task { await WindowPictures.run() }
    }
}

@MainActor
enum WindowPictures {
    private static var started = false

    static func run() async {
        let defaults = UserDefaults.standard
        guard !started, let folder = defaults.string(forKey: "NavreePictures") else { return }
        started = true
        let out = URL(fileURLWithPath: (folder as NSString).expandingTildeInPath, isDirectory: true)
        try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        var log: [String] = []
        // SwiftUI gives its windows' elements only to an assistive app, which
        // first sets the app's enhanced user interface: the app does so itself.
        let enhance = NSSelectorFromString("accessibilitySetEnhancedUserInterfaceAttribute:")
        if NSApp.responds(to: enhance) { _ = NSApp.perform(enhance, with: NSNumber(value: true)) }
        // The menu bar acts on the key window of the active app, as for a person.
        NSApp.activate(ignoringOtherApps: true)
        windows.first?.makeKeyAndOrderFront(nil)
        let steps = (defaults.string(forKey: "NavreeSteps") ?? "wait:4;menus;picture:window;quit")
            .split(separator: ";").map { String($0).trimmingCharacters(in: .whitespaces) }
        for step in steps {
            let parts = step.split(separator: ":", maxSplits: 1).map(String.init)
            let argument = parts.count > 1 ? parts[1] : ""
            switch parts.first ?? "" {
            case "wait":
                try? await Task.sleep(for: .seconds(Double(argument) ?? 1))
            case "front":
                NSApp.activate(ignoringOtherApps: true)
                windows.first?.makeKeyAndOrderFront(nil)
                log.append("front: key window \"\(NSApp.keyWindow?.title ?? "none")\", app \(NSApp.isActive ? "active" : "not active")")
            case "size":
                resize(argument)
                log.append("size \(argument)")
            case "move":
                let numbers = argument.split(separator: ",").compactMap { Double($0) }
                if numbers.count == 2, let window = windows.first {
                    window.setFrameOrigin(NSPoint(x: numbers[0], y: numbers[1]))
                }
                log.append("move \(argument) pointer \(NSEvent.mouseLocation)")
            case "menus":
                let name = argument.isEmpty ? "menus.txt" : "menus-\(argument).txt"
                write(menus(), to: out.appendingPathComponent(name))
                log.append("menus written to \(name)")
            case "menu":
                log.append("menu \(argument): \(choose(argument) ? "chosen" : "not found or disabled")")
            case "key":
                log.append("key \(argument): \(press(argument) ? "taken by the menu bar" : "not taken")")
            case "type":
                // The shortcut as the keyboard sends it, through the app's event queue.
                let before = windows.first?.title ?? ""
                post(argument)
                try? await Task.sleep(for: .seconds(2))
                log.append("type \(argument): window \"\(before)\" became \"\(windows.first?.title ?? "")\"")
            case "picture":
                log.append(contentsOf: pictures(argument, out: out))
            case "composed":
                log.append(composed(argument, out: out))
            case "tree":
                write(tree(), to: out.appendingPathComponent("\(argument).txt"))
                log.append("tree \(argument).txt")
            case "views":
                write(views(), to: out.appendingPathComponent("\(argument).txt"))
                log.append("views \(argument).txt")
            case "toolbar":
                write(toolbar(), to: out.appendingPathComponent("\(argument).txt"))
                log.append("toolbar \(argument).txt")
            case "elements":
                write(await fromRunLoop(elements), to: out.appendingPathComponent("\(argument).txt"))
                log.append("elements \(argument).txt")
            case "click":
                let before = windows.first?.title ?? ""
                let place = await fromRunLoop { click(argument) }
                try? await Task.sleep(for: .seconds(2))
                log.append("click \(argument) \(place): window \"\(before)\" became \"\(windows.first?.title ?? "")\"")
            case "choose":
                let before = windows.first?.title ?? ""
                let chosen = choose(Int(argument) ?? 0)
                try? await Task.sleep(for: .seconds(2))
                log.append("choose \(argument): \(chosen); front window \"\(before)\" became \"\(windows.first?.title ?? "")\", \(windows.count) windows")
            case "scroll":
                log.append("scroll \(argument): \(scroll(Double(argument) ?? 0))")
            case "appearance":
                // The app's appearance: `dark`, `light`, or `system`.
                NSApp.appearance = argument == "dark" ? NSAppearance(named: .darkAqua)
                    : argument == "light" ? NSAppearance(named: .aqua) : nil
                log.append("appearance \(argument)")
            case "quit":
                write(log.joined(separator: "\n") + "\n", to: out.appendingPathComponent("steps.txt"))
                // A sheet still up would hold the app open.
                for window in NSApp.windows {
                    if let sheet = window.attachedSheet { window.endSheet(sheet) }
                }
                NSApp.terminate(nil)
            default:
                log.append("unknown step \(step)")
            }
        }
        write(log.joined(separator: "\n") + "\n", to: out.appendingPathComponent("steps.txt"))
    }

    /// Runs `work` from the main run loop, as an assistive app's requests
    /// come, rather than inside this task: asking a table for its elements
    /// lays out all its rows, and their views' own code then runs where it
    /// would for VoiceOver.
    static func fromRunLoop<T: Sendable>(_ work: @escaping @MainActor () -> T) async -> T {
        await withCheckedContinuation { continuation in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { continuation.resume(returning: work()) }
            }
        }
    }

    /// The windows on screen, the one in front first.
    static var windows: [NSWindow] {
        NSApp.orderedWindows.filter { $0.isVisible && $0.frame.width > 100 }
    }

    static func resize(_ size: String) {
        let numbers = size.split(separator: "x").compactMap { Double($0) }
        guard numbers.count == 2, let window = windows.first else { return }
        var frame = window.frame
        frame.origin.y += frame.height - numbers[1]
        frame.size = CGSize(width: numbers[0], height: numbers[1])
        window.setFrame(frame, display: true, animate: false)
    }

    /// Each window as the window server would show it: its title bar, its
    /// toolbar and its content, drawn by the window's own views.
    static func pictures(_ name: String, out: URL) -> [String] {
        windows.enumerated().compactMap { index, window in
            guard let content = window.contentView, let frame = content.superview ?? Optional(content) else { return nil }
            frame.layoutSubtreeIfNeeded()
            guard let bitmap = frame.bitmapImageRepForCachingDisplay(in: frame.bounds) else { return nil }
            frame.cacheDisplay(in: frame.bounds, to: bitmap)
            guard let png = bitmap.representation(using: .png, properties: [:]) else { return nil }
            let file = out.appendingPathComponent("\(name)-\(index + 1).png")
            try? png.write(to: file)
            return "picture \(file.lastPathComponent) \(Int(frame.bounds.width))x\(Int(frame.bounds.height)) \"\(window.title)\""
        }
    }

    /// The window in front as the window server composes it, its glass and
    /// a sidebar floating on it included: an app may always picture its own
    /// windows, with no screen recording.
    static func composed(_ name: String, out: URL) -> String {
        typealias Capture = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?
        guard let window = windows.first else { return "composed \(name): no window" }
        guard let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImage") else {
            return "composed \(name): the window server cannot picture a window here"
        }
        let capture = unsafeBitCast(symbol, to: Capture.self)
        // Only this window, its frame's shadow left out, at the screen's scale.
        guard let image = capture(.null, 1 << 3, UInt32(window.windowNumber), (1 << 0) | (1 << 3))?.takeRetainedValue(),
            let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
        else { return "composed \(name): no picture" }
        let file = out.appendingPathComponent("\(name).png")
        try? png.write(to: file)
        return "composed \(file.lastPathComponent) \(image.width)x\(image.height) pixels \"\(window.title)\""
    }

    /// What the window in front says to VoiceOver, element by element: what
    /// a picture cannot show of glass the window server draws (a sidebar
    /// that floats) is still there to read.
    static func tree() -> String {
        guard let window = windows.first else { return "no window\n" }
        var lines: [String] = []
        walk(window, limit: 40) { element, depth in
            let words = element.words
            if !words.isEmpty || depth < 3 {
                lines.append(String(repeating: "  ", count: depth) + element.role + " " + words.joined(separator: " | "))
            }
            return true
        }
        return lines.joined(separator: "\n") + "\n"
    }

    /// The window in front's toolbar, item by item from the leading end: what
    /// each is, its words, where it stands in the window, and over which of
    /// the split view's columns, so a picture's toolbar can be read against
    /// the columns under it.
    static func toolbar() -> String {
        guard let window = windows.first else { return "no window\n" }
        var lines: [String] = []
        let columns = splitColumns(in: window)
        lines.append("columns " + columns.map { "\(Int($0.minX))-\(Int($0.maxX))" }.joined(separator: " | "))
        let items = window.toolbar?.items ?? []
        for item in items {
            var line = item.itemIdentifier.rawValue
            let words = [item.label, item.paletteLabel, item.toolTip ?? ""].filter { !$0.isEmpty }
            if !words.isEmpty { line += " \"" + words.joined(separator: "\" \"") + "\"" }
            if let view = item.view, view.window != nil {
                let frame = view.convert(view.bounds, to: nil)
                line += " at \(Int(frame.minX))-\(Int(frame.maxX))"
                if let column = columns.firstIndex(where: { $0.minX <= frame.midX && frame.midX < $0.maxX }) {
                    line += " over column \(column + 1)"
                }
                let texts = labels(in: view)
                if !texts.isEmpty { line += " says \"" + texts.joined(separator: "\" \"") + "\"" }
            } else {
                line += " (not shown)"
            }
            if !item.isEnabled { line += " (disabled)" }
            lines.append(line)
        }
        lines.append("title \"\(window.title)\" subtitle \"\(window.subtitle)\"")
        return lines.joined(separator: "\n") + "\n"
    }

    /// The columns of the window's split view, from the leading edge.
    static func splitColumns(in window: NSWindow) -> [CGRect] {
        guard let root = window.contentView else { return [] }
        var found: NSSplitView?
        func walk(_ view: NSView) {
            if found == nil, let split = view as? NSSplitView, split.isVertical { found = split; return }
            view.subviews.forEach(walk)
        }
        walk(root)
        return (found?.arrangedSubviews ?? []).filter { !$0.isHidden && $0.frame.width > 1 }
            .map { $0.convert($0.bounds, to: nil) }
    }

    /// The words a view and the views in it say to VoiceOver.
    static func labels(in view: NSView) -> [String] {
        var found: [String] = []
        walk(view, limit: 8) { element, _ in
            for words in element.words.prefix(2) where !found.contains(words) { found.append(words) }
            return true
        }
        return found
    }

    /// The window in front's views, each with its frame in the window, the
    /// appearance it draws in and, for a material, which one: what a picture
    /// taken by the app cannot show of what the window server blends.
    static func views() -> String {
        guard let window = windows.first, let root = window.contentView?.superview ?? window.contentView else {
            return "no window\n"
        }
        var lines = ["window appearance \(window.effectiveAppearance.name.rawValue)"]
        func walk(_ view: NSView, depth: Int) {
            guard depth < 60 else { return }
            let frame = view.convert(view.bounds, to: nil)
            var line = String(repeating: " ", count: depth) + String(describing: type(of: view))
                + " \(Int(frame.minX)),\(Int(frame.minY)) \(Int(frame.width))x\(Int(frame.height))"
            if let own = view.appearance { line += " own=\(own.name.rawValue)" }
            line += " draws=\(view.effectiveAppearance.name.rawValue)"
            if let effect = view as? NSVisualEffectView {
                line += " material=\(effect.material.rawValue) blending=\(effect.blendingMode.rawValue) state=\(effect.state.rawValue)"
            }
            if view.isHidden { line += " hidden" }
            lines.append(line)
            for child in view.subviews { walk(child, depth: depth + 1) }
        }
        walk(root, depth: 0)
        return lines.joined(separator: "\n") + "\n"
    }

    /// Every element the window in front says to VoiceOver, indented by
    /// depth: its role, its frame in the window (x and y from the window's
    /// top leading corner, then width and height, in points) and its words.
    /// What a picture shows as pixels, this gives as the frames SwiftUI laid
    /// out: each word's and each control's leading edge, a button's height.
    static func elements() -> String {
        guard let window = windows.first else { return "no window\n" }
        var lines = [
            "window \(Int(window.frame.width))x\(Int(window.frame.height)) \"\(window.title)\";"
                + " frames as x y width height, in points from the window's top leading corner",
        ]
        walk(window) { element, depth in
            var line = String(repeating: "  ", count: depth) + element.role
            if let frame = element.frame(in: window) {
                line += " " + [frame.minX, frame.minY, frame.width, frame.height].map(points).joined(separator: " ")
            }
            if !element.words.isEmpty { line += " \"" + element.words.joined(separator: "\" \"") + "\"" }
            lines.append(line)
            return true
        }
        return lines.joined(separator: "\n") + "\n"
    }

    /// Walks the elements of a window or a view, each before its children, until
    /// `visit` says to stop. A menu's items are not walked: they stand in
    /// the menu, not in the window.
    static func walk(_ root: AnyObject, limit: Int = 60, _ visit: (Element, Int) -> Bool) {
        func walk(_ element: Element, depth: Int) -> Bool {
            guard depth < limit else { return true }
            if !visit(element, depth) { return false }
            guard !["AXMenuButton", "AXPopUpButton", "AXMenu", "AXMenuBar", "AXMenuItem"].contains(element.role) else { return true }
            for child in element.children where !walk(child, depth: depth + 1) { return false }
            return true
        }
        _ = walk(Element(object: root), depth: 0)
    }

    /// An element as VoiceOver reads it: through the methods of its role
    /// where it has them, else through its attributes by name. Words are
    /// read as whatever object holds them and kept only when they are a
    /// string, an attributed string or a number: bridging anything else to a
    /// string throws, and an exception thrown through Swift's frames leaves
    /// the main actor's checks reading freed memory later on.
    struct Element {
        let object: AnyObject

        /// An attribute by name, only when the element says it has it: an
        /// element asked for one it has not may throw, and nothing thrown
        /// may pass through this code.
        private func attribute(_ name: String) -> Any? {
            guard let object = object as? NSObject else { return nil }
            let names = NSSelectorFromString("accessibilityAttributeNames")
            let read = NSSelectorFromString("accessibilityAttributeValue:")
            guard object.responds(to: names), object.responds(to: read),
                let known = object.perform(names)?.takeUnretainedValue() as? [Any],
                known.contains(where: { ($0 as? NSObject)?.isEqual(name as NSString) == true })
            else { return nil }
            return object.perform(read, with: name)?.takeUnretainedValue()
        }

        /// What a method of the element's role returns, else its attribute
        /// by that name, as words, whatever kind of object holds them.
        private func words(_ method: String, _ name: String) -> String? {
            guard let object = object as? NSObject else { return nil }
            var found: Any?
            let selector = NSSelectorFromString(method)
            if object.responds(to: selector) { found = object.perform(selector)?.takeUnretainedValue() }
            if found == nil { found = attribute(name) }
            guard let found = found as? NSObject else { return nil }
            if let words = found as? NSAttributedString { return words.string }
            if found.isKind(of: NSString.self) { return String(describing: found) }
            if let number = found as? NSNumber { return number.stringValue }
            return nil
        }

        var role: String { words("accessibilityRole", "AXRole") ?? "" }

        var words: [String] {
            [words("accessibilityLabel", "AXDescription"), words("accessibilityTitle", "AXTitle"), words("accessibilityValue", "AXValue")]
                .compactMap { $0 }.filter { !$0.isEmpty }
        }

        var children: [Element] {
            guard let object = object as? NSObject else { return [] }
            var found: Any?
            let selector = NSSelectorFromString("accessibilityChildren")
            if object.responds(to: selector) { found = object.perform(selector)?.takeUnretainedValue() }
            if found == nil { found = attribute("AXChildren") }
            return (found as? [Any] ?? []).map { Element(object: $0 as AnyObject) }
        }

        /// The element's frame in the window, from its top leading corner.
        func frame(in window: NSWindow) -> CGRect? {
            var screen = object.accessibilityFrame?() ?? .zero
            if screen.width == 0, screen.height == 0,
                let position = (attribute("AXPosition") as? NSValue)?.pointValue,
                let size = (attribute("AXSize") as? NSValue)?.sizeValue
            {
                // Inside the app, as the frame is: from the screen's bottom leading corner.
                screen = CGRect(origin: position, size: size)
            }
            guard screen.width > 0 || screen.height > 0 else { return nil }
            let base = window.convertFromScreen(screen)
            return CGRect(x: base.minX, y: window.frame.height - base.maxY, width: base.width, height: base.height)
        }
    }

    /// A length in points, to the half point the screen draws.
    static func points(_ value: CGFloat) -> String {
        let rounded = (value * 2).rounded() / 2
        return rounded == rounded.rounded() ? String(Int(rounded)) : String(format: "%.1f", rounded)
    }

    /// A click at a point of the window in front (`x,y` from its top leading
    /// corner), or in the middle of the first element whose words hold the
    /// argument: a press and a release of the mouse's button, posted to the
    /// app's event queue as the window server delivers a person's.
    static func click(_ argument: String) -> String {
        guard let window = windows.first else { return "no window" }
        let numbers = argument.split(separator: ",").compactMap { Double($0.trimmingCharacters(in: .whitespaces)) }
        var point: CGPoint?
        if numbers.count == 2 {
            point = CGPoint(x: numbers[0], y: numbers[1])
        } else if let frame = find(argument, in: window) {
            point = CGPoint(x: frame.midX, y: frame.midY)
        }
        guard let point else { return "found nothing saying \"\(argument)\"" }
        let base = NSPoint(x: point.x, y: window.frame.height - point.y)
        let events = [(NSEvent.EventType.leftMouseDown, Float(1)), (.leftMouseUp, 0)].compactMap { type, pressure in
            NSEvent.mouseEvent(
                with: type, location: base, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: pressure
            )
        }
        guard events.count == 2 else { return "no event" }
        if NSApp.isActive {
            events.forEach { NSApp.postEvent($0, atStart: false) }
            return "at \(points(point.x)),\(points(point.y))"
        }
        // An app the system keeps from the front takes a click on its window
        // only as the click that brings it forward, which it cannot be: the
        // view under the point is handed the press itself, and the release
        // waits in the queue for the press's own tracking.
        guard let frame = window.contentView?.superview ?? window.contentView,
            let view = frame.hitTest(frame.convert(base, from: nil))
        else { return "nothing under \(points(point.x)),\(points(point.y))" }
        NSApp.postEvent(events[1], atStart: false)
        view.mouseDown(with: events[0])
        return "at \(points(point.x)),\(points(point.y)) (the app not in front: handed to \(type(of: view)))"
    }

    /// Chooses a row of the window's table (counted from 0), as a click on
    /// it does in a window in front: through the table's own selection, which
    /// tells the table's delegate, and SwiftUI's selection with it.
    static func choose(_ row: Int) -> String {
        guard let window = windows.first, let root = window.contentView else { return "no window" }
        var found: NSTableView?
        func walk(_ view: NSView) {
            if found == nil, let table = view as? NSTableView, table.numberOfColumns > 1 { found = table; return }
            view.subviews.forEach(walk)
        }
        walk(root)
        guard let table = found, row < table.numberOfRows else { return "no table with that row" }
        guard table.delegate?.tableView?(table, shouldSelectRow: row) ?? true else { return "row \(row) may not be chosen" }
        table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        return "row \(row) chosen of \(table.numberOfRows), selected \(Array(table.selectedRowIndexes))"
    }

    /// The frame of the first element whose words are these, else of the
    /// first whose words hold them, in the window.
    static func find(_ words: String, in window: NSWindow) -> CGRect? {
        var exact: CGRect?
        var holding: CGRect?
        walk(window) { element, depth in
            guard depth > 0, let frame = element.frame(in: window) else { return true }
            if element.words.contains(words) {
                exact = frame
                return false
            }
            if holding == nil, element.words.contains(where: { $0.contains(words) }) { holding = frame }
            return true
        }
        return exact ?? holding
    }

    /// Scrolls the scroll view under the window's middle by some points.
    static func scroll(_ by: Double) -> String {
        guard let window = windows.first, let root = window.contentView else { return "no window" }
        let middle = NSPoint(x: root.bounds.midX, y: root.bounds.midY)
        var view = root.hitTest(middle)
        while let current = view, !(current is NSScrollView) { view = current.superview }
        guard let scroll = view as? NSScrollView, let document = scroll.documentView else { return "no scroll view" }
        let clip = scroll.contentView
        var origin = clip.bounds.origin
        let room = max(document.frame.height - clip.bounds.height, 0)
        origin.y = document.isFlipped ? min(max(origin.y + by, 0), room) : min(max(origin.y - by, 0), room)
        clip.scroll(to: origin)
        scroll.reflectScrolledClipView(clip)
        return "to \(Int(origin.y)) of \(Int(room))"
    }

    /// Every menu of the menu bar, each item with its shortcut, indented by depth.
    static func menus() -> String {
        guard let bar = NSApp.mainMenu else { return "no menu bar\n" }
        var lines: [String] = []
        func walk(_ menu: NSMenu, depth: Int) {
            // As the menu bar does when a person opens the menu.
            menu.delegate?.menuNeedsUpdate?(menu)
            menu.delegate?.menuWillOpen?(menu)
            defer { menu.delegate?.menuDidClose?(menu) }
            menu.update()
            for item in menu.items {
                let pad = String(repeating: "    ", count: depth)
                if item.isSeparatorItem {
                    lines.append(pad + "----")
                    continue
                }
                var line = pad + item.title
                let key = shortcut(item)
                if !key.isEmpty { line += "    " + key }
                if !item.isEnabled { line += "    (disabled)" }
                if item.state == .on { line += "    (checked)" }
                if item.image != nil { line += "    [icon]" }
                lines.append(line)
                if let submenu = item.submenu { walk(submenu, depth: depth + 1) }
            }
        }
        walk(bar, depth: 0)
        return lines.joined(separator: "\n") + "\n"
    }

    static func shortcut(_ item: NSMenuItem) -> String {
        guard !item.keyEquivalent.isEmpty else { return "" }
        let mask = item.keyEquivalentModifierMask
        var text = ""
        if mask.contains(.control) { text += "⌃" }
        if mask.contains(.option) { text += "⌥" }
        if mask.contains(.shift) { text += "⇧" }
        if mask.contains(.command) { text += "⌘" }
        let key: String
        switch item.keyEquivalent {
        case "\u{8}", "\u{7f}": key = "⌫"
        case "\r": key = "↩"
        case "\u{1b}": key = "⎋"
        default: key = item.keyEquivalent.uppercased()
        }
        return text + key
    }

    /// Chooses the command at `Menu/Item` (or `Menu/Submenu/Item`).
    static func choose(_ path: String) -> Bool {
        var menu = NSApp.mainMenu
        let names = path.split(separator: "/").map(String.init)
        for (depth, name) in names.enumerated() {
            guard let current = menu else { return false }
            current.update()
            guard let index = current.items.firstIndex(where: { $0.title == name }) else { return false }
            let item = current.items[index]
            if depth == names.count - 1 {
                guard item.isEnabled else { return false }
                current.performActionForItem(at: index)
                return true
            }
            menu = item.submenu
        }
        return false
    }

    /// Posts a shortcut's key down and up to the app's own event queue.
    static func post(_ chord: String) {
        guard let (flags, key) = parse(chord), let window = windows.first else { return }
        for type in [NSEvent.EventType.keyDown, .keyUp] {
            if let event = NSEvent.keyEvent(
                with: type, location: .zero, modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, characters: key, charactersIgnoringModifiers: key,
                isARepeat: false, keyCode: keyCode(key)
            ) {
                NSApp.postEvent(event, atStart: false)
            }
        }
    }

    /// The hardware key of a character, as an ANSI keyboard has it.
    static func keyCode(_ key: String) -> UInt16 {
        let codes: [String: UInt16] = [
            "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11, "q": 12, "w": 13,
            "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23, "9": 25, "7": 26,
            "8": 28, "0": 29, "]": 30, "o": 31, "u": 32, "[": 33, "i": 34, "p": 35, "l": 37, "j": 38, "k": 40, "n": 45,
            "m": 46, ",": 43, "\u{8}": 51,
        ]
        return codes[key] ?? 0
    }

    static func parse(_ chord: String) -> (NSEvent.ModifierFlags, String)? {
        var flags: NSEvent.ModifierFlags = []
        var key = ""
        for part in chord.split(separator: "-").map(String.init) {
            switch part {
            case "cmd": flags.insert(.command)
            case "opt": flags.insert(.option)
            case "shift": flags.insert(.shift)
            case "ctrl": flags.insert(.control)
            case "delete": key = "\u{8}"
            case "comma": key = ","
            default: key = part
            }
        }
        return key.isEmpty ? nil : (flags, key)
    }

    /// Presses a shortcut on the menu bar: `cmd-n`, `cmd-opt-n`, `cmd-delete`, `cmd-[`.
    static func press(_ chord: String) -> Bool {
        let parts = chord.split(separator: "-").map(String.init)
        var flags: NSEvent.ModifierFlags = []
        var key = ""
        for part in parts {
            switch part {
            case "cmd": flags.insert(.command)
            case "opt": flags.insert(.option)
            case "shift": flags.insert(.shift)
            case "ctrl": flags.insert(.control)
            case "delete": key = "\u{8}"
            case "comma": key = ","
            default: key = part
            }
        }
        guard let window = windows.first, let menu = NSApp.mainMenu,
            let event = NSEvent.keyEvent(
                with: .keyDown, location: .zero, modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, characters: key, charactersIgnoringModifiers: key,
                isARepeat: false, keyCode: 0
            )
        else { return false }
        // As the keyboard's event goes: the key window first, then the menu bar.
        if window.performKeyEquivalent(with: event) { return true }
        return menu.performKeyEquivalent(with: event)
    }

    static func write(_ text: String, to file: URL) {
        try? Data(text.utf8).write(to: file)
    }
}
