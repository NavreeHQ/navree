import Foundation
import SwiftUI

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

/// App > About: what the app adds to the system's own About panel. With
/// nothing added, the system's own item stands.
struct AboutCommands: Commands {
    let configuration: ShellConfiguration?

    var body: some Commands {
        EmptyCommands()
    }
}

/// The Help menu's item, as the app says. With nothing said, the system's own
/// item stands.
struct HelpCommands: Commands {
    let configuration: ShellConfiguration?

    var body: some Commands {
        EmptyCommands()
    }
}
