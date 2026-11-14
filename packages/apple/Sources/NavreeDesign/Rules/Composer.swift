import Foundation
import NavreeProtocol

/// A conversation's reply, as Messages keeps its field: resting at the foot
/// of the conversation, above the tab bar and the keyboard, while the
/// messages scroll under it.
///
/// The protocol has no word for a composer, so it is read from what the
/// screen holds: a conversation (messages) followed, as the last thing on the
/// screen, by a form that posts, with a field for many lines of text and a
/// main button that sends it.
public enum ComposerRules {
    /// The composer and the screen's body without it.
    public struct Found: Sendable, Equatable {
        /// The reply form, drawn as a bar at the foot of the screen.
        public var form: Navree.Form
        /// Everything else the body holds, in its order.
        public var rest: Navree.Node
        /// The field for the reply's words.
        public var message: Navree.Field
        /// The button that sends it.
        public var send: Navree.Button
    }

    /// The reply form a body ends with, when the body is a conversation.
    public static func composer(in body: Navree.Node) -> Found? {
        guard case .stack(let stack) = body, !stack.hidden else { return nil }
        guard let last = stack.children.lastIndex(where: { !SplitRules.isOverlay($0) }),
            case .form(let form) = stack.children[last], form.method == .post
        else { return nil }
        guard let message = multilineField(in: form), let send = sendButton(in: form) else { return nil }
        let before = stack.children[..<last]
        guard before.contains(where: holdsMessages) else { return nil }
        var rest = stack
        rest.children.remove(at: last)
        return Found(form: form, rest: .stack(rest), message: message, send: send)
    }

    /// The form's field for many lines of text.
    static func multilineField(in form: Navree.Form) -> Navree.Field? {
        var found: Navree.Field?
        Navree.Node.form(form).walk { node in
            guard found == nil, case .field(let field) = node, case .text(let text) = field.input, text.kind == .multiline
            else { return }
            found = field
        }
        return found
    }

    /// The form's main button, which sends what was written.
    static func sendButton(in form: Navree.Form) -> Navree.Button? {
        var found: Navree.Button?
        for node in form.children { walkOutsidePopovers(node) { part in
            guard found == nil, case .button(let button) = part, button.role == .primary || button.role == .confirm,
                case .submit? = button.action
            else { return }
            found = button
        } }
        return found
    }

    /// Whether a part holds messages: the conversation the reply answers.
    static func holdsMessages(_ node: Navree.Node) -> Bool {
        node.first { if case .message = $0 { true } else { false } } != nil
    }

    /// Visits a part and what it holds, leaving out what a popover holds.
    static func walkOutsidePopovers(_ node: Navree.Node, _ visit: (Navree.Node) -> Void) {
        var pending: [Navree.Node] = [node]
        while let next = pending.popLast() {
            if case .popover = next { continue }
            visit(next)
            pending.append(contentsOf: next.children.reversed())
        }
    }

    /// The composer's parts other than its field for words and its send
    /// button (a choice of how to send, a button that opens saved replies),
    /// which stand in a row over the field.
    public static func others(_ found: Found) -> [Navree.Node] {
        var parts: [Navree.Node] = []
        func each(_ nodes: [Navree.Node]) {
            for node in nodes {
                switch node {
                case .stack(let stack) where !stack.hidden: each(stack.children)
                case .grid(let grid): each(grid.children)
                case .field(let field) where field.name == found.message.name: break
                case .button(let button) where button == found.send: break
                // A popover stands by the button that opens it, never in the row.
                case .popover, .sheet, .dialog: break
                case .stack: break
                default: parts.append(node)
                }
            }
        }
        each(found.form.children)
        return parts
    }

    /// The symbol of the reply's send button, inside the field's trailing
    /// end: the filled arrow Messages and Mail send with, whatever icon the
    /// page gives its button (its words stay its name for VoiceOver).
    public static let sendSymbol = "arrow.up.circle.fill"

    /// Whether the reply can be sent: when it holds words, as Messages' send
    /// button waits for them.
    public static func canSend(_ text: String) -> Bool {
        !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}
