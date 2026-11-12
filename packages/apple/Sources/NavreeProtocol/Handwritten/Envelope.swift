/// What a server answers a native client with: a screen, ops for the screen
/// in front, both, or a word to show the page in the web view.
public struct Envelope: Sendable, Equatable, Decodable {
    /// The protocol's major version.
    public var navree: Int
    /// The server's build.
    public var build: String
    /// The vocabulary the answer is written in.
    public var vocab: Int
    /// A stamp of the app, when the server sends one: a client drops the trees
    /// it kept when it changes.
    public var app: String?
    /// A stamp of the shell document: a client fetches the shell again when it
    /// changes.
    public var shell: String?
    public var screen: Screen?
    /// Changes to the screen in front, in order; after `screen` when both are here.
    public var ops: [Op]
    public var web: Web?
    public var upgrade: Upgrade?
    /// Icon shapes by name, sent once an envelope: an icon in the tree that has
    /// a name and no paths takes its shape from here.
    public var icons: [String: IconShape]

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        navree = try c.field("navree", or: Wire.major)
        build = try c.field("build", or: "")
        vocab = try c.field("vocab", or: Wire.vocab)
        app = try? c.optional("app")
        shell = try? c.optional("shell")
        screen = try c.optional("screen")
        ops = try c.field("ops", or: [])
        web = try c.optional("web")
        upgrade = try c.optional("upgrade")
        icons = (try? c.optional("icons")) ?? [:]
    }
}

/// An icon's shape: the paths to stroke and to fill, in their box.
public struct IconShape: Sendable, Equatable, Decodable {
    public var paths: [String]
    public var fills: [String]
    public var viewBox: String?

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        paths = try c.field("paths", or: [])
        fills = try c.field("fills", or: [])
        viewBox = try c.optional("view_box")
    }
}

/// A whole page, as parts. Its actions carry roles, never positions.
public struct Screen: Sendable, Equatable, Decodable {
    public var title: String
    public var subtitle: String?
    /// The id of the screen's content, when the page gave one.
    public var id: String?
    /// The screen before this one in the app's own order.
    public var back: Back?
    /// The whole trail to this screen, first screen first.
    public var trail: [Back]
    /// The address of the shell's destination this screen belongs to.
    public var nav: String?
    /// What a reader may do with the screen as a whole: buttons and menus.
    public var actions: [Node]
    public var search: Search?
    /// Whether pulling down fetches the screen again.
    public var refresh: Bool
    public var body: Node

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        title = try c.field("title", or: "")
        subtitle = try c.optional("subtitle")
        id = try c.optional("id")
        back = try c.optional("back")
        trail = try c.field("trail", or: [])
        nav = try c.optional("nav")
        actions = try c.field("actions", or: [])
        search = try c.optional("search")
        refresh = try c.field("refresh", or: false)
        body = try c.required("body")
    }
}

/// A way back: the screen before this one in the app's own order.
public struct Back: Sendable, Equatable, Decodable {
    public var title: String
    public var href: String

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        title = try c.field("title", or: "")
        href = try c.required("href")
    }
}

/// A search a screen offers: the words go as `name`, with `hidden`, to `action`.
public struct Search: Sendable, Equatable, Decodable {
    public var name: String
    public var value: String
    public var placeholder: String?
    public var action: String
    public var hidden: [Pair]

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        name = try c.required("name")
        value = try c.field("value", or: "")
        placeholder = try c.optional("placeholder")
        action = try c.field("action", or: "")
        hidden = try c.field("hidden", or: [])
    }
}

/// "Show this in the web view."
public struct Web: Sendable, Equatable, Decodable {
    public var url: String
    public var title: String?
    /// What could not be drawn natively, for a developer's overlay.
    public var why: [String]

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        url = try c.required("url")
        title = try c.optional("title")
        why = try c.field("why", or: [])
    }
}

/// "This app is too old to serve safely."
public struct Upgrade: Sendable, Equatable, Decodable {
    public var minApp: String
    public var url: String

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        minApp = try c.field("min_app", or: "")
        url = try c.field("url", or: "")
    }
}

/// A short message that leaves by itself, on the web. Apple systems have no
/// such thing; the design layer decides what it becomes.
public struct Toast: Sendable, Equatable, Decodable {
    public var tone: Tone
    public var text: String
    public var title: String?
    /// A later toast of the same id takes this one's place.
    public var id: String?
    public var action: ToastAction?

    public init(tone: Tone, text: String, title: String? = nil, id: String? = nil) {
        self.tone = tone
        self.text = text
        self.title = title
        self.id = id
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        tone = try c.field("tone", or: .neutral)
        text = try c.field("text", or: "")
        title = try c.optional("title")
        id = try c.optional("id")
        action = try c.optional("action")
    }
}

/// What a toast's button does.
public struct ToastAction: Sendable, Equatable, Decodable {
    public var label: String
    public var href: String
    public var post: Bool

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        label = try c.field("label", or: "")
        href = try c.required("href")
        post = try c.field("post", or: false)
    }
}

/// One change the server makes to the screen in front.
public enum Op: Sendable, Equatable, Decodable {
    /// Puts `node` in place of `target`, keeping what the reader did.
    case morph(target: String, node: Node)
    /// Puts `node` in place of `target`, as new.
    case replace(target: String, node: Node)
    case append(target: String, nodes: [Node], more: String?)
    case prepend(target: String, nodes: [Node])
    case remove(target: String)
    /// Messages at the fields of a form; empty `fields` clears them.
    case errors(form: String, fields: [String: [String]])
    case toast(Toast)
    case open(target: String)
    case close(target: String)
    case focus(target: String)
    case url(href: String, mode: URLMode)
    case navigate(href: String)
    case navigateExternal(href: String)
    case reload
    case invalidate(target: String)
    /// An op added after this client was built: passed over.
    case unrecognized(String)

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        let name: String = try c.required("op")
        switch name {
        case "morph": self = .morph(target: try c.required("target"), node: try c.required("node"))
        case "replace": self = .replace(target: try c.required("target"), node: try c.required("node"))
        case "append":
            self = .append(target: try c.required("target"), nodes: try c.field("nodes", or: []), more: try c.optional("more"))
        case "prepend": self = .prepend(target: try c.required("target"), nodes: try c.field("nodes", or: []))
        case "remove": self = .remove(target: try c.required("target"))
        case "errors": self = .errors(form: try c.required("form"), fields: try c.field("fields", or: [:]))
        case "toast": self = .toast(try Toast(from: decoder))
        case "open": self = .open(target: try c.required("target"))
        case "close": self = .close(target: try c.required("target"))
        case "focus": self = .focus(target: try c.required("target"))
        case "url": self = .url(href: try c.required("href"), mode: try c.field("mode", or: .push))
        case "navigate": self = .navigate(href: try c.required("href"))
        case "navigate_external": self = .navigateExternal(href: try c.required("href"))
        case "reload": self = .reload
        case "invalidate": self = .invalidate(target: try c.required("target"))
        default: self = .unrecognized(name)
        }
    }
}
