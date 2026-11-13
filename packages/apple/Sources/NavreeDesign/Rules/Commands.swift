import Foundation
import NavreeProtocol

/// The key a menu command is chosen with, always with Command.
public struct Shortcut: Sendable, Hashable {
    public enum Key: Sendable, Hashable {
        case letter(Character)
        /// ⌘1 to ⌘9.
        case digit(Int)
        /// ⌘⌫: delete, as Finder and Photos have it.
        case delete
        /// ⌘[: back.
        case leftBracket
    }

    public var key: Key
    public var option: Bool

    public init(_ key: Key, option: Bool = false) {
        self.key = key
        self.option = option
    }

    /// ⌘N: a new thing of what the window shows.
    public static let new = Shortcut(.letter("n"))
    /// ⌥⌘N: a new window, where ⌘N makes a new thing (Mail's New Viewer Window).
    public static let newWindow = Shortcut(.letter("n"), option: true)
    /// ⌘F: the search field takes the focus.
    public static let find = Shortcut(.letter("f"))
    /// ⌘R: the screen is fetched again.
    public static let refresh = Shortcut(.letter("r"))
    /// ⌘⌫: what is shown is deleted, after the person says so.
    public static let delete = Shortcut(.delete)
    /// ⌘[: one screen back.
    public static let back = Shortcut(.leftBracket)
}

/// Where a command stands in the menu bar.
public enum MenuSlot: Sendable, Equatable {
    /// File, at the top: making something new.
    case new
    /// File, before Print: sending what is shown elsewhere.
    case share
    /// Edit > Find.
    case find
    /// The menu named for what the window shows, among the app's own menus.
    case screen
    /// The same menu, last and set apart: what destroys.
    case destructive
}

/// One command of the menu bar for one of a screen's actions.
public struct MenuEntry: Sendable, Equatable {
    /// The command's words, in title case, with an ellipsis where more is asked first.
    public var title: String
    public var slot: MenuSlot
    public var shortcut: Shortcut?
    /// The system symbol beside it: only for a standard action (macOS 27
    /// shows icons in menus for key actions only).
    public var symbol: String?
    public var disabled: Bool
    /// The action's place among the leaves given.
    public var index: Int
}

/// The menu bar of a Mac app, and of an iPad app with a keyboard: every
/// action the toolbar and the context menus offer is also a command there,
/// with the shortcut Apple's apps give the standard ones.
public enum CommandRules {
    /// The actions behind a screen's toolbar or a row's context menu, one by
    /// one: a menu of actions stands for the actions it holds, a toolbar for
    /// its items.
    public static func leaves(_ nodes: [Navree.Node]) -> [Navree.Node] {
        nodes.flatMap { node -> [Navree.Node] in
            switch node {
            case .menu(let menu): leaves(menu.items)
            case .toolbar(let bar): leaves(bar.items.map(part))
            case .button(let button) where button.action == nil: []
            case .button, .link: [node]
            default: []
            }
        }
    }

    /// The commands of a screen's actions (already leaves), each where the
    /// macOS menu bar puts it. Making something new is File > New with ⌘N,
    /// sharing is File > Share, searching is Edit > Find with ⌘F, deleting is
    /// the last command of the screen's menu with ⌘⌫; anything else is a
    /// command of the screen's menu with no shortcut of its own. A shortcut
    /// goes to the first action that may have it, never to two.
    public static func entries(_ actions: [Navree.Node]) -> [MenuEntry] {
        var taken: Set<Shortcut> = []
        func claim(_ shortcut: Shortcut) -> Shortcut? {
            taken.insert(shortcut).inserted ? shortcut : nil
        }
        return actions.enumerated().compactMap { index, node -> MenuEntry? in
            switch node {
            case .button(let button):
                let disabled = button.disabled || button.pending
                let asks = button.ask != nil && !button.undoable
                let symbol = button.standard.flatMap(Symbols.standard)
                if button.role == .create || button.standard == .add {
                    return MenuEntry(title: title(button.label), slot: .new, shortcut: claim(.new), symbol: symbol, disabled: disabled, index: index)
                }
                if button.standard == .share {
                    return MenuEntry(title: title(button.label, more: true), slot: .share, shortcut: nil, symbol: symbol, disabled: disabled, index: index)
                }
                if button.standard == .search {
                    return MenuEntry(title: title(button.label, more: true), slot: .find, shortcut: claim(.find), symbol: symbol, disabled: disabled, index: index)
                }
                if button.standard == .delete {
                    return MenuEntry(title: title(button.label, more: asks), slot: .destructive, shortcut: claim(.delete), symbol: symbol, disabled: disabled, index: index)
                }
                if button.role == .destructive {
                    return MenuEntry(title: title(button.label, more: asks), slot: .destructive, shortcut: nil, symbol: nil, disabled: disabled, index: index)
                }
                return MenuEntry(title: title(button.label, more: asks), slot: .screen, shortcut: nil, symbol: symbol, disabled: disabled, index: index)
            case .link(let link):
                return MenuEntry(title: title(link.label), slot: .screen, shortcut: nil, symbol: nil, disabled: false, index: index)
            default:
                return nil
            }
        }
    }

    /// A toolbar's item as the button or menu it stands for.
    static func part(_ item: Navree.ToolbarItem) -> Navree.Node {
        if !item.menu.isEmpty {
            return .menu(Navree.Menu(icon: item.icon, id: item.id, items: item.menu, label: item.title))
        }
        return .button(Navree.Button(
            action: item.action, ask: item.ask, disabled: item.disabled, icon: item.icon, id: item.id,
            label: item.title, prominence: item.prominence, reason: item.reason, role: item.role,
            standard: item.standard, undoable: item.undoable
        ))
    }

    /// Whether a screen has a search field for Find (⌘F) to go to: its own
    /// search, or the search field of its table's query.
    public static func searches(_ screen: Navree.Screen) -> Bool {
        if screen.search != nil { return true }
        var found = false
        screen.body.walk { node in
            guard !found, case .table(let table) = node, let query = table.query else { return }
            query.walkFields { field in
                if case .field(let field) = field, case .text(let text) = field.input, text.kind == .search { found = true }
            }
        }
        return found
    }

    /// The shortcut of the destination at `index` in the View menu: ⌘1 to ⌘9.
    public static func destination(_ index: Int) -> Shortcut? {
        (0..<9).contains(index) ? Shortcut(.digit(index + 1)) : nil
    }

    /// Words a menu shows in title case, as macOS menus write them: every word
    /// capitalised but the short ones inside the title. Words already written
    /// with capitals inside them (an order number, a brand) stay as they are.
    /// `more` ends them with an ellipsis: the command asks for more first.
    public static func title(_ words: String, more: Bool = false) -> String {
        let small: Set<String> = ["a", "an", "and", "as", "at", "but", "by", "for", "in", "of", "on", "or", "the", "to", "via"]
        let parts = words.split(separator: " ", omittingEmptySubsequences: true).map(String.init)
        let titled = parts.enumerated().map { index, word -> String in
            let lower = word.lowercased()
            if index > 0, index < parts.count - 1, small.contains(lower) { return lower }
            if word != lower { return word }
            return word.prefix(1).uppercased() + word.dropFirst()
        }
        let text = titled.joined(separator: " ")
        guard more, !text.hasSuffix("…"), !text.hasSuffix("...") else { return text }
        return text + "…"
    }

    /// The name of the menu that holds the window's own commands: the
    /// destination it shows, as Mail names its Mailbox menu. A name the
    /// system's own menus already have is not taken twice.
    public static func screenMenu(_ destination: String?) -> String {
        let name = destination.map { title($0) } ?? ""
        let system: Set<String> = ["", "File", "Edit", "View", "Window", "Help", "Format"]
        return system.contains(name) ? "Actions" : name
    }
}

/// Where the account's settings are: the user menu's link to them, which a
/// Mac shows as the Settings window from the App menu (⌘,), never as a page.
public enum SettingsRules {
    /// The user menu's entry that leads to the person's settings: a link whose
    /// icon is a gear or sliders, or whose address ends in `settings` or
    /// `preferences`. The protocol has no word for it, so it is read here.
    public static func destination(in menu: [Navree.Node]) -> (index: Int, href: String)? {
        for (index, node) in menu.enumerated() {
            let found: (href: String, icon: Navree.Icon?)?
            switch node {
            case .link(let link):
                if case .navigate(let go) = link.action { found = (go.href, nil) } else { found = nil }
            case .button(let button):
                if case .navigate(let go)? = button.action { found = (go.href, button.icon) } else { found = nil }
            default:
                found = nil
            }
            guard let found else { continue }
            if let icon = found.icon, isGear(icon) { return (index, found.href) }
            let path = found.href.split(separator: "?").first.map(String.init) ?? found.href
            let last = path.split(separator: "/").last.map { $0.lowercased() } ?? ""
            if last == "settings" || last == "preferences" { return (index, found.href) }
        }
        return nil
    }

    static func isGear(_ icon: Navree.Icon) -> Bool {
        let names: Set<String> = ["settings", "settings-2", "cog", "sliders", "sliders-horizontal", "sliders-vertical"]
        if let name = icon.name, names.contains(name) { return true }
        if let symbol = icon.symbol?.apple, symbol.hasPrefix("gearshape") { return true }
        return false
    }
}
