import NavreeProtocol

/// A screen's own actions, as the drawing places them: buttons, menus and
/// links, with a toolbar's items read as the buttons and menus they are.
public enum ActionRules {
    /// The actions in `nodes`, each toolbar's items in its place: an item is a
    /// button with its words, symbol, role and question; an item that opens a
    /// menu is that menu.
    public static func expand(_ nodes: [Navree.Node]) -> [Navree.Node] {
        nodes.flatMap { node -> [Navree.Node] in
            guard case .toolbar(let toolbar) = node else { return [node] }
            return toolbar.items.map(node(for:))
        }
    }

    /// A toolbar's item as the part it stands for.
    public static func node(for item: Navree.ToolbarItem) -> Navree.Node {
        if !item.menu.isEmpty {
            return .menu(Navree.Menu(icon: item.icon, id: item.id, items: item.menu, label: item.title))
        }
        return .button(Navree.Button(
            action: item.action, ask: item.ask, disabled: item.disabled, icon: item.icon, id: item.id,
            label: item.title, prominence: item.prominence, reason: item.reason, role: item.role,
            standard: item.standard, undoable: item.undoable
        ))
    }

    /// The toolbars that stand in a screen's body: a group of actions at its
    /// foot, which Apple's systems put on the bottom bar.
    public static func footToolbars(_ body: Navree.Node) -> [Navree.Toolbar] {
        var found: [Navree.Toolbar] = []
        body.walk { node in
            if case .toolbar(let toolbar) = node { found.append(toolbar) }
        }
        return found
    }

    /// Why a control can't be used now, when it can't and the server says why.
    public static func reason(_ button: Navree.Button) -> String? {
        guard button.disabled, let reason = button.reason, !reason.isEmpty else { return nil }
        return reason
    }

    /// The words under a group of buttons: why those of them that can't be
    /// used now can't, one after another.
    public static func footer(_ buttons: [Navree.Button]) -> String? {
        let reasons = buttons.compactMap(reason)
        return reasons.isEmpty ? nil : reasons.joined(separator: " ")
    }
}

/// How the question a control asks before it acts is put to the person.
public enum QuestionStyle: Sendable, Equatable {
    /// An action sheet from the control, its destructive answer first and
    /// Cancel last: the question about a destructive act the person started.
    case confirmationDialog
    /// The system's alert, its answer the default and Cancel beside it.
    case alert
}

public enum QuestionRules {
    /// The control in `screen` whose question this is: the one that asks it in
    /// these words.
    public static func asker(title: String, in screen: Navree.Screen) -> Navree.Button? {
        var found: Navree.Button?
        let actions = ActionRules.expand(screen.actions)
        for node in actions + [screen.body] {
            node.walk { part in
                guard found == nil else { return }
                switch part {
                case .button(let button) where button.ask?.title == title:
                    found = button
                case .toolbar(let toolbar):
                    for item in toolbar.items where found == nil && item.ask?.title == title {
                        if case .button(let button) = ActionRules.node(for: item) { found = button }
                    }
                default:
                    break
                }
            }
            if found != nil { break }
        }
        return found
    }

    /// A destructive act's own question is an action sheet with its answer in
    /// red; any other is an alert whose answer is not.
    public static func style(asker: Navree.Button?) -> QuestionStyle {
        guard let asker else { return .alert }
        return asker.role == .destructive || asker.standard == .delete ? .confirmationDialog : .alert
    }

    /// Whether the answer that goes ahead is drawn as destructive.
    public static func confirmIsDestructive(asker: Navree.Button?) -> Bool {
        style(asker: asker) == .confirmationDialog
    }
}
