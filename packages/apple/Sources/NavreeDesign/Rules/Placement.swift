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
}

/// What placement needs to know of an action.
public struct ActionTraits: Sendable, Equatable {
    public var role: Role
    public var prominence: ButtonProminence
    public var standard: StandardAction?
    public var isMenu: Bool

    public init(role: Role, prominence: ButtonProminence = .medium, standard: StandardAction? = nil, isMenu: Bool = false) {
        self.role = role
        self.prominence = prominence
        self.standard = standard
        self.isMenu = isMenu
    }

    public init?(_ node: Node) {
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
    public static func place(_ actions: [ActionTraits], depth: ScreenDepth) -> [Slot] {
        var tookMain = false
        var tookCreate = false
        let inSheet = depth == .sheet || depth == .cover
        return actions.map { (action: ActionTraits) -> Slot in
            if action.role == .destructive || action.standard == .delete {
                return .destructiveSection
            }
            if action.role == .cancel || action.standard == .cancel {
                return inSheet ? .cancel : .overflow
            }
            if action.role == .navigation {
                return inSheet ? .overflow : .leading
            }
            let isMain = action.role == .primary || action.role == .confirm || action.standard == .done
            if isMain {
                guard !tookMain else { return .overflow }
                tookMain = true
                return inSheet ? .confirm : .primary
            }
            if action.role == .create || action.standard == .add {
                guard !tookCreate, !inSheet else { return .overflow }
                tookCreate = true
                return .create
            }
            return .overflow
        }
    }
}
