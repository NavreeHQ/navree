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

    /// A sheet's own form: the first in its parts that does not stand in a
    /// sheet, dialog or popover of its own, nor in a table (a record's inline edit).
    public static func ownForm(_ nodes: [Navree.Node]) -> Navree.Form? {
        func find(_ node: Navree.Node) -> Navree.Form? {
            switch node {
            case .form(let form): return form
            case .sheet, .dialog, .popover, .table: return nil
            default: return node.children.lazy.compactMap(find).first
            }
        }
        return nodes.lazy.compactMap(find).first
    }

    /// The submit buttons of a sheet's own form, lifted to its bar: there is
    /// no submit button at the foot of a form on Apple's systems. A submit
    /// with no role, or the primary one, confirms.
    public static func lift(_ nodes: [Navree.Node]) -> LiftedSubmits {
        guard let form = ownForm(nodes) else { return LiftedSubmits(form: nil, confirms: []) }
        var confirms: [Navree.Button] = []
        form.walkFields { node in
            if case .button(var button) = node, case .submit? = button.action {
                if button.role == Navree.Role.none || button.role == .primary { button.role = .confirm }
                confirms.append(button)
            }
        }
        return LiftedSubmits(form: form, confirms: confirms)
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
