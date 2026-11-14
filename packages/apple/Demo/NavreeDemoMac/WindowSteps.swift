import AppKit

/// The Mac demo's picture steps about its windows and its icon:
///
/// `frame` (the front window's frame, the smallest content the system lets
/// the person make it, its toolbar's style and labels, whether windows join
/// as tabs, and every window of the app by its kind);
/// `labels:<iconOnly|titleAndIcon|titleOnly>` (the toolbar's labels changed
/// as the person changes them from the toolbar's menu); `about:<name>` (App > About, its panel pictured as
/// `<name>.png` and its words written); `icon:<name>` (the app's icon as the
/// system draws it for Finder, from the bundle, in the light and the dark,
/// as `<name>-light.png` and `<name>-dark.png`).
@MainActor
enum WindowSteps {
    /// The lines the step writes to the log, or nil for a step it does not know.
    static func run(_ step: String, _ argument: String, out: URL) async -> [String]? {
        switch step {
        case "frame":
            return frame()
        case "labels":
            return await labels(argument)
        case "about":
            return await about(argument, out: out)
        case "icon":
            return icon(argument, out: out)
        default:
            return nil
        }
    }

    static func frame() -> [String] {
        var lines: [String] = []
        if let window = WindowPictures.windows.first {
            let frame = window.frame
            lines.append("frame \"\(window.title)\" \(Int(frame.width))x\(Int(frame.height)) at \(Int(frame.minX)),\(Int(frame.minY))")
            lines.append("  content smallest \(Int(window.contentMinSize.width))x\(Int(window.contentMinSize.height))")
            lines.append("  toolbar style \(name(window.toolbarStyle)), title \(window.titleVisibility == .visible ? "shown" : "hidden")")
            if let toolbar = window.toolbar {
                lines.append("  toolbar labels \(name(toolbar.displayMode)), the person may change them: \(toolbar.allowsDisplayModeCustomization)")
            }
            lines.append("  tabbing mode \(name(window.tabbingMode)), tabs \(window.tabbedWindows?.count ?? 0)")
        } else {
            lines.append("frame: no window")
        }
        lines.append("  windows join as tabs: \(NSWindow.allowsAutomaticWindowTabbing)")
        let kinds = NSApp.windows.map { "\(type(of: $0))\($0.isVisible ? "" : " (hidden)")" }
        lines.append("  the app's windows: " + kinds.joined(separator: ", "))
        // An item in the menu bar lives in a window of its own there.
        for window in NSApp.windows where "\(type(of: window))".contains("StatusBar") {
            let buttons = descendants(of: window.contentView).filter { $0 is NSStatusBarButton }
            lines.append(
                "  menu bar item: \(Int(window.frame.width))x\(Int(window.frame.height)), on screen \(window.occlusionState.contains(.visible)), "
                    + "\(buttons.count) buttons \(buttons.map { "\"\(($0 as? NSButton)?.title ?? "")\" \(Int($0.frame.width)) wide, hidden \($0.isHidden)" })"
            )
        }
        return lines
    }

    static func labels(_ argument: String) async -> [String] {
        let modes: [String: NSToolbar.DisplayMode] = [
            "iconOnly": .iconOnly, "titleAndIcon": .iconAndLabel, "titleOnly": .labelOnly,
        ]
        guard let mode = modes[argument], let toolbar = WindowPictures.windows.first?.toolbar else {
            return ["labels \(argument): no such labels or no toolbar"]
        }
        toolbar.displayMode = mode
        try? await Task.sleep(for: .seconds(1))
        return ["labels \(argument): toolbar shows \(name(toolbar.displayMode))"]
    }

    static func about(_ name: String, out: URL) async -> [String] {
        let app = Bundle.main.object(forInfoDictionaryKey: "CFBundleName") as? String ?? "Charkha"
        let chosen = WindowPictures.choose("\(app)/About \(app)")
        try? await Task.sleep(for: .seconds(1.5))
        // The About panel is a panel, which the app's ordered windows leave out.
        guard let panel = NSApp.windows.first(where: { $0 is NSPanel && $0.isVisible }) else {
            return ["about: \(chosen ? "chosen" : "not found"), no panel"]
        }
        var lines = ["about: \(chosen ? "chosen" : "not found"), panel \(Int(panel.frame.width))x\(Int(panel.frame.height))"]
        lines.append("  says \"" + words(in: panel.contentView).joined(separator: "\" \"") + "\"")
        if let view = panel.contentView?.superview ?? panel.contentView,
            let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds)
        {
            view.cacheDisplay(in: view.bounds, to: bitmap)
            if let png = bitmap.representation(using: .png, properties: [:]) {
                try? png.write(to: out.appendingPathComponent("\(name).png"))
                lines.append("  picture \(name).png")
            }
        }
        panel.close()
        return lines
    }

    static func icon(_ name: String, out: URL) -> [String] {
        let icon = NSWorkspace.shared.icon(forFile: Bundle.main.bundlePath)
        var lines = ["icon of \(Bundle.main.bundlePath.split(separator: "/").last ?? ""): \(icon.representations.count) sizes, CFBundleIconName \(Bundle.main.object(forInfoDictionaryKey: "CFBundleIconName") as? String ?? "none")"]
        for (look, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            guard let rep = NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: 512, pixelsHigh: 512, bitsPerSample: 8, samplesPerPixel: 4,
                hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
            ) else { continue }
            rep.size = NSSize(width: 512, height: 512)
            NSAppearance(named: appearance)?.performAsCurrentDrawingAppearance {
                NSGraphicsContext.saveGraphicsState()
                NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
                icon.draw(in: NSRect(x: 0, y: 0, width: 512, height: 512))
                NSGraphicsContext.restoreGraphicsState()
            }
            if let png = rep.representation(using: .png, properties: [:]) {
                try? png.write(to: out.appendingPathComponent("\(name)-\(look).png"))
                lines.append("  picture \(name)-\(look).png")
            }
        }
        return lines
    }

    /// A view and every view in it.
    static func descendants(of view: NSView?) -> [NSView] {
        guard let view else { return [] }
        return [view] + view.subviews.flatMap { descendants(of: $0) }
    }

    /// The words of the text fields in a view and the views in it.
    static func words(in view: NSView?) -> [String] {
        guard let view else { return [] }
        var found: [String] = []
        if let field = view as? NSTextField, !field.stringValue.isEmpty { found.append(field.stringValue) }
        if let text = view as? NSTextView, !text.string.isEmpty { found.append(text.string) }
        for child in view.subviews { found += words(in: child) }
        return found
    }

    static func name(_ style: NSWindow.ToolbarStyle) -> String {
        switch style {
        case .automatic: "automatic"
        case .expanded: "expanded"
        case .preference: "preference"
        case .unified: "unified"
        case .unifiedCompact: "unifiedCompact"
        @unknown default: "other \(style.rawValue)"
        }
    }

    static func name(_ mode: NSToolbar.DisplayMode) -> String {
        switch mode {
        case .default: "default"
        case .iconAndLabel: "iconAndLabel"
        case .iconOnly: "iconOnly"
        case .labelOnly: "labelOnly"
        @unknown default: "other \(mode.rawValue)"
        }
    }

    static func name(_ mode: NSWindow.TabbingMode) -> String {
        switch mode {
        case .automatic: "automatic"
        case .preferred: "preferred"
        case .disallowed: "disallowed"
        @unknown default: "other \(mode.rawValue)"
        }
    }
}
