import NavreeProtocol

/// Where a screen's action goes. The server says what an action is; this is
/// where Apple's guidelines put it.
public enum Slot: Sendable, Equatable {
    /// The top bar's trailing end, drawn prominent: the screen's one main act
    /// (Save, Send). `ToolbarItemPlacement.primaryAction` on iPhone and iPad.
    case primary
    /// The top bar's trailing end, as a symbol: making something new (`plus`).
    case create
    /// A sheet's top trailing corner: Done, Save, Send (`confirmationAction`).
    case confirm
    /// A sheet's top leading corner: Cancel, Close (`cancellationAction`).
    case cancel
    /// The system's overflow menu at the trailing end (`secondaryAction`).
    case overflow
    /// A section of its own at the end of the content, in red.
    case destructiveSection
    /// The top bar's leading end, for going somewhere (`navigation`).
    case leading
    /// On a bar as its symbol alone, its words for VoiceOver and the overflow.
    case bar
    /// A Mac window's toolbar, as its symbol (its words where it has none),
    /// the system's overflow taking it when the window is too narrow.
    case window
    /// Only in the menu bar's menu for what the window shows, last and in
    /// red: what destroys, on a Mac.
    case menu
}

/// The bars a screen's actions go to.
public enum ActionBar: Sendable, Equatable {
    /// A navigation bar (iPhone, iPad): the main act and making something new
    /// at its trailing end, the rest in its overflow menu, what destroys in a
    /// section at the end of the content.
    case navigation
    /// A Mac window's toolbar, beside a menu bar that holds every action
    /// too: every action is an item of the toolbar (the system moves those
    /// that do not fit into its overflow menu), and what destroys stays out
    /// of the toolbar, the last command of the window's menu, in red.
    case window

    /// The bars of the system this is built for.
    public static var current: ActionBar {
        #if os(macOS)
        .window
        #else
        .navigation
        #endif
    }
}

/// What placement needs to know of an action.
public struct ActionTraits: Sendable, Equatable {
    public var role: Navree.Role
    public var prominence: Navree.Prominence
    public var standard: Navree.Standard?
    public var isMenu: Bool

    public init(role: Navree.Role, prominence: Navree.Prominence = .medium, standard: Navree.Standard? = nil, isMenu: Bool = false) {
        self.role = role
        self.prominence = prominence
        self.standard = standard
        self.isMenu = isMenu
    }

    public init?(_ node: Navree.Node) {
        switch node {
        case .button(let button):
            self.init(role: button.role, prominence: button.prominence, standard: button.standard)
        case .menu:
            self.init(role: .secondary, isMenu: true)
        case .link:
            self.init(role: .navigation)
        default:
            return nil
        }
    }
}

public enum PlacementRules {
    /// Where each of a screen's actions goes, in the order given.
    ///
    /// In a sheet: cancel at the top leading corner, the one confirming action
    /// at the top trailing corner. On a screen: creation as a symbol at the
    /// trailing end, the one main action beside it, everything else in the
    /// system's overflow menu. Anything destructive gets a section of its own
    /// at the end of the content, and is never the prominent action. Only one
    /// action is ever prominent; a second "primary" goes to the overflow menu.
    ///
    /// On a Mac window (`bar: .window`), what would go to the overflow menu
    /// stands in the window's toolbar, and what destroys only in the menu bar.
    public static func place(_ actions: [ActionTraits], depth: ScreenDepth, bar: ActionBar = .navigation) -> [Slot] {
        var tookMain = false
        var tookCreate = false
        let inSheet = depth == .sheet || depth == .cover
        let window = bar == .window && !inSheet
        return actions.map { (action: ActionTraits) -> Slot in
            if action.role == .destructive || action.standard == .delete {
                return window ? .menu : .destructiveSection
            }
            if action.role == .cancel || action.standard == .cancel {
                return inSheet ? .cancel : window ? .window : .overflow
            }
            if action.role == .navigation {
                return inSheet ? .overflow : .leading
            }
            let isMain = action.role == .primary || action.role == .confirm || action.standard == .done
            if isMain {
                guard !tookMain else { return window ? .window : .overflow }
                tookMain = true
                return inSheet ? .confirm : .primary
            }
            if action.role == .create || action.standard == .add {
                guard !tookCreate, !inSheet else { return window ? .window : .overflow }
                tookCreate = true
                return .create
            }
            return window ? .window : .overflow
        }
    }
}
