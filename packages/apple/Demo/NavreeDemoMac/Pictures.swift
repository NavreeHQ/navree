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
/// `views:<name>` (the window's views, their appearance and materials);
/// `toolbar:<name>` (the toolbar's items, their words and the column each
/// stands over);
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
                log.append("front: key window \"\(NSApp.keyWindow?.title ?? "none")\"")
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
            case "tree":
                write(tree(), to: out.appendingPathComponent("\(argument).txt"))
                log.append("tree \(argument).txt")
            case "views":
                write(views(), to: out.appendingPathComponent("\(argument).txt"))
                log.append("views \(argument).txt")
            case "toolbar":
                write(toolbar(), to: out.appendingPathComponent("\(argument).txt"))
                log.append("toolbar \(argument).txt")
            case "appearance":
                // The app's appearance: `dark`, `light`, or `system`.
                NSApp.appearance = argument == "dark" ? NSAppearance(named: .darkAqua)
                    : argument == "light" ? NSAppearance(named: .aqua) : nil
                log.append("appearance \(argument)")
            case "quit":
                write(log.joined(separator: "\n") + "\n", to: out.appendingPathComponent("steps.txt"))
                NSApp.terminate(nil)
            default:
                log.append("unknown step \(step)")
            }
        }
        write(log.joined(separator: "\n") + "\n", to: out.appendingPathComponent("steps.txt"))
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

    /// What the window in front says to VoiceOver, element by element: what
    /// a picture cannot show of glass the window server draws (a sidebar
    /// that floats) is still there to read.
    static func tree() -> String {
        guard let window = windows.first else { return "no window\n" }
        var lines: [String] = []
        func walk(_ element: Any, depth: Int) {
            guard depth < 40 else { return }
            let object = element as AnyObject
            let role = (object.accessibilityRole?() ?? nil)?.rawValue ?? ""
            let label = object.accessibilityLabel?() ?? nil
            let title = object.accessibilityTitle?() ?? nil
            let value = (object.accessibilityValue?() ?? nil).map { "\($0)" }
            let words = [label, title, value].compactMap { $0 }.filter { !$0.isEmpty }
            if !words.isEmpty || depth < 3 {
                lines.append(String(repeating: "  ", count: depth) + role + " " + words.joined(separator: " | "))
            }
            for child in (object.accessibilityChildren?() ?? nil) ?? [] {
                walk(child, depth: depth + 1)
            }
        }
        walk(window, depth: 0)
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
        func walk(_ element: Any, depth: Int) {
            guard depth < 8 else { return }
            let object = element as AnyObject
            for words in [object.accessibilityLabel?() ?? nil, object.accessibilityTitle?() ?? nil] {
                if let words, !words.isEmpty, !found.contains(words) { found.append(words) }
            }
            for child in (object.accessibilityChildren?() ?? nil) ?? [] { walk(child, depth: depth + 1) }
        }
        walk(view, depth: 0)
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
