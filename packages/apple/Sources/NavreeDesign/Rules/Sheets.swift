import NavreeProtocol

/// How a sheet rests: a glance at half height that the person swipes away, or
/// a task at full height that they finish or cancel.
public enum SheetHeight: Sendable, Equatable {
    /// Half height first, pulled up to full, with a grabber.
    case half
    /// Full height only.
    case full
}

/// The button that takes a sheet away when it has no cancel of its own.
public enum SheetDismissal: Sendable, Equatable {
    /// Cancel: what the person typed is lost.
    case cancel
    /// Close: nothing is lost by closing.
    case close
}

/// What a task's form gives its bar: the form, and its submit buttons as the
/// sheet's confirming actions (Done, Save, Send at the top trailing corner).
public struct LiftedSubmits: Sendable, Equatable {
    public var form: Navree.Form?
    public var confirms: [Navree.Button]
}

public enum SheetRules {
    /// A task rests at full height; a glance at half height, pulled up with
    /// its grabber and swiped away.
    public static func height(task: Bool) -> SheetHeight {
        task ? .full : .half
    }

    /// A sheet's own form: the one form in its parts that does not stand in
    /// a sheet, dialog or popover of its own, nor in a table (a record's
    /// inline edit). A sheet of several forms (a card with its checklist and
    /// its column) has no one task to finish: none.
    public static func ownForm(_ nodes: [Navree.Node]) -> Navree.Form? {
        var found: [Navree.Form] = []
        func find(_ node: Navree.Node) {
            switch node {
            case .form(let form): found.append(form)
            case .sheet, .dialog, .popover, .table: return
            default: node.children.forEach(find)
            }
        }
        nodes.forEach(find)
        return found.count == 1 ? found[0] : nil
    }

    /// The submit buttons of a sheet's own form, lifted to its bar: there is
    /// no submit button at the foot of a form on Apple's systems. The task's
    /// main submit confirms it (Done at the top trailing corner): the primary
    /// one, else the first that neither cancels nor destroys, whatever the
    /// web's look made its role.
    public static func lift(_ nodes: [Navree.Node]) -> LiftedSubmits {
        guard let form = ownForm(nodes) else { return LiftedSubmits(form: nil, confirms: []) }
        var submits: [Navree.Button] = []
        form.walkFields { node in
            if case .button(let button) = node, case .submit? = button.action { submits.append(button) }
        }
        let main = submits.firstIndex { $0.role == .primary || $0.role == .confirm }
            ?? submits.firstIndex { $0.role != .destructive && $0.role != .cancel }
        if let main { submits[main].role = .confirm }
        return LiftedSubmits(form: form, confirms: submits)
    }

    /// Whether a sheet's parts hold something to fill in: a field of a form.
    public static func holdsInput(_ nodes: [Navree.Node]) -> Bool {
        nodes.contains { node in node.first { if case .field = $0 { return true } else { return false } } != nil }
    }

    /// A task that holds input is cancelled; anything else is closed.
    public static func dismissal(task: Bool, holdsInput: Bool) -> SheetDismissal {
        task && holdsInput ? .cancel : .close
    }
}
