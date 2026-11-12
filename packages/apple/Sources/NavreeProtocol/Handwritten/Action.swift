/// A value sent with a request.
public struct Pair: Sendable, Hashable, Decodable {
    public var name: String
    public var value: String

    public init(name: String, value: String) {
        self.name = name
        self.value = value
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        name = try c.required("name")
        value = try c.field("value", or: "")
    }
}

/// What a control does. The list is closed; nothing in it is code.
public enum Action: Sendable, Equatable, Decodable {
    /// Goes to a screen.
    case navigate(href: String, present: Present)
    /// Sends a form: `form` names it, or it is the one the control stands in.
    case submit(form: String?, href: String?, with: Pair?)
    /// Sends a request of its own: a button that posts.
    case request(method: HTTPMethod, href: String, fields: [Pair])
    /// Changes the screen in front, with no request.
    case commands([Command])
    /// Goes back one screen.
    case back
    /// Fetches the screen again.
    case refresh
    /// Closes the sheet or dialog the control stands in.
    case dismiss
    /// An action added after this client was built: the control does nothing.
    case unrecognized(String)

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        let kind: String = try c.required("t")
        switch kind {
        case "navigate":
            self = .navigate(href: try c.required("href"), present: try c.field("present", or: .push))
        case "submit":
            self = .submit(form: try c.optional("form"), href: try c.optional("href"), with: try c.optional("with"))
        case "request":
            self = .request(
                method: try c.field("method", or: .get),
                href: try c.required("href"),
                fields: try c.field("fields", or: [])
            )
        case "commands":
            self = .commands(try c.field("list", or: []))
        case "back":
            self = .back
        case "refresh":
            self = .refresh
        case "dismiss":
            self = .dismiss
        default:
            self = .unrecognized(kind)
        }
    }
}

/// One change a client makes on its own, with no request.
public enum Command: Sendable, Equatable, Decodable {
    case toggle(id: String)
    case show(id: String)
    case hide(id: String)
    /// Presents a sheet, a dialog or a popover, or opens a disclosure.
    case open(id: String)
    /// Dismisses a sheet, a dialog or a popover, or closes a disclosure.
    case close(id: String)
    case copy(text: String)
    case set(id: String, state: NodeState, on: Bool)
    case flip(id: String, state: NodeState)
    case focus(id: String)
    case submit(id: String)
    case text(id: String, text: String)
    case count(id: String, max: Int?)
    case value(id: String, value: String)
    case toast(tone: Tone, text: String)
    /// A command added after this client was built: passed over.
    case unrecognized(String)

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        let name: String = try c.required("c")
        switch name {
        case "toggle": self = .toggle(id: try c.required("id"))
        case "show": self = .show(id: try c.required("id"))
        case "hide": self = .hide(id: try c.required("id"))
        case "open": self = .open(id: try c.required("id"))
        case "close": self = .close(id: try c.required("id"))
        case "copy": self = .copy(text: try c.required("text"))
        case "set":
            self = .set(id: try c.required("id"), state: try c.required("state"), on: try c.field("on", or: false))
        case "flip": self = .flip(id: try c.required("id"), state: try c.required("state"))
        case "focus": self = .focus(id: try c.required("id"))
        case "submit": self = .submit(id: try c.required("id"))
        case "text": self = .text(id: try c.required("id"), text: try c.required("text"))
        case "count": self = .count(id: try c.required("id"), max: try c.optional("max"))
        case "value": self = .value(id: try c.required("id"), value: try c.required("value"))
        case "toast": self = .toast(tone: try c.field("tone", or: .neutral), text: try c.required("text"))
        default: self = .unrecognized(name)
        }
    }
}
