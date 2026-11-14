import Foundation
import SwiftUI

#if os(macOS)
import AppKit
#endif

/// What the About panel adds to the app's name, icon and version, which stay
/// the system's from the Info.plist.
public struct AboutPanel: Sendable, Equatable {
    /// Lines under the version, with their links.
    public var credits: AttributedString?
    /// The copyright line, in place of the Info.plist's.
    public var copyright: String?

    /// Whether nothing is added: the system's own panel and its own item.
    public var isSystems: Bool { credits == nil && copyright == nil }
}

/// The Help menu's item.
public enum HelpItem: Sendable, Equatable {
    /// "<App> Help" opens a page of the app, as a link from outside does.
    case page(title: String, href: String)
    /// "<App> Help" opens a site in the person's browser.
    case site(title: String, url: URL)
    /// No item: the menu keeps only the system's search field.
    case none
}

/// About and Help, as plain rules.
public enum AboutRules {
    /// The panel's options: the credits read as Markdown with their links, the
    /// copyright only when the app gives one.
    public static func options(_ about: About) -> AboutPanel {
        let credits = about.credits.flatMap { text -> AttributedString? in
            guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
            let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
            return (try? AttributedString(markdown: text, options: options)) ?? AttributedString(text)
        }
        let copyright = about.copyright.flatMap { $0.trimmingCharacters(in: .whitespaces).isEmpty ? nil : $0 }
        return AboutPanel(credits: credits, copyright: copyright)
    }
}

public enum HelpRules {
    /// The Help menu's item; nil keeps the system's own (which opens the app's
    /// help book, or says there is none). A page must be of the app's origin
    /// and a site an `https` address, else the system's item stays.
    public static func item(_ help: HelpMenu, appName: String) -> HelpItem? {
        let title = String(localized: "\(appName) Help")
        switch help {
        case .system:
            return nil
        case .page(let href):
            return OpeningRules.address(href).map { .page(title: title, href: $0) }
        case .site(let url):
            return url.scheme?.lowercased() == "https" && url.host() != nil ? .site(title: title, url: url) : nil
        case .hidden:
            return HelpItem.none
        }
    }
}

/// App > About: what the app adds to the system's own About panel (Mac).
/// With nothing added, the system's own item stands.
struct AboutCommands: Commands {
    let configuration: ShellConfiguration?

    var body: some Commands {
        #if os(macOS)
        let panel = AboutRules.options(configuration?.presence.about ?? About())
        if !panel.isSystems {
            CommandGroup(replacing: .appInfo) {
                // With the symbol of the system's own item.
                Button(String(localized: "About \(PresenceModel.appName)"), systemImage: "info.circle") { AboutWindow.show(panel) }
            }
        }
        #endif
    }
}

/// The Help menu's item, as the app says (a Mac, and the iPad's menu bar).
/// With nothing said, the system's own item stands.
struct HelpCommands: Commands {
    let configuration: ShellConfiguration?

    var body: some Commands {
        if let item = HelpRules.item(configuration?.presence.help ?? .system, appName: PresenceModel.appName) {
            CommandGroup(replacing: .help) {
                HelpButton(item: item)
            }
        }
    }
}

/// "<App> Help" (⌘?): a page of the app in the window in front, or a site in
/// the person's browser.
struct HelpButton: View {
    let item: HelpItem
    @Environment(\.openURL) private var openURL
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        switch item {
        case .page(let title, let href):
            Button(title, systemImage: "questionmark.circle") { PresenceModel.shared.open(href, with: openWindow) }
                .keyboardShortcut("?", modifiers: .command)
        case .site(let title, let url):
            Button(title, systemImage: "questionmark.circle") { openURL(url) }
                .keyboardShortcut("?", modifiers: .command)
        case .none:
            EmptyView()
        }
    }
}

#if os(macOS)
/// The system's standard About panel, with what the app adds: its name, icon
/// and version stay the Info.plist's.
@MainActor
public enum AboutWindow {
    public static func show(_ panel: AboutPanel) {
        var options: [NSApplication.AboutPanelOptionKey: Any] = [:]
        if let credits = panel.credits { options[.credits] = attributed(credits) }
        if let copyright = panel.copyright { options[NSApplication.AboutPanelOptionKey(rawValue: "Copyright")] = copyright }
        NSApp.activate()
        NSApp.orderFrontStandardAboutPanel(options: options)
    }

    /// The credits in the panel's small system type and label colour, centred,
    /// their links kept.
    static func attributed(_ credits: AttributedString) -> NSAttributedString {
        let text = NSMutableAttributedString(credits)
        let whole = NSRange(location: 0, length: text.length)
        let centred = NSMutableParagraphStyle()
        centred.alignment = .center
        text.addAttributes([
            .font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize),
            .foregroundColor: NSColor.labelColor,
            .paragraphStyle: centred,
        ], range: whole)
        return text
    }
}
#endif
