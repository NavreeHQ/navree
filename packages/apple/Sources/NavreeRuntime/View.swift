import Foundation
import NavreeProtocol

// The runtime core's view state, as the client reads it: everything the app
// draws from, part by part, as the core last said it. Read like the
// protocol's models: a field these do not know is passed over, and a word
// outside a closed list is read as its field's default.
//
// None of these types is `Equatable`. The observed model stores them, and
// Observation compares an `Equatable` value with the one it replaces before it
// tells the views: for a screen that is a comparison of two whole trees, on
// the main thread, which a tree as deep as the protocol allows does not fit.
// The core already says what changed.

/// A screen's number in the core: the same for as long as the screen lives.
public typealias ScreenID = Int

/// How far the app has got in starting.
public enum Stage: String, Sendable {
    case starting, ready, failed, upgrade
}

/// Why something could not be loaded. The words for each are the client's.
public enum Failure: String, Sendable {
    case offline, timeout, server, `protocol`, config
}

/// A message of the core's own, worded by the client.
public enum CoreNotice: String, Sendable {
    case offline, timeout, failed, unsaved, loading, copied
    case fieldsNeedAttention = "fields_need_attention"
    case loaded
    case secondStep = "second_step"
}

/// The window's width class, as the client said it.
public enum Width: String, Sendable, Codable {
    case compact, medium, expanded
}

/// The app's own part of the chrome: its name, theme and the person signed in.
public struct AppPart: Sendable {
    public var name: String
    public var context: String?
    public var home: String
    public var theme: Navree.Theme?
    public var user: Navree.User?
    /// The shell's routes, when the core says them.
    public var routes: [Navree.Route] = []
}

/// A top-level destination and the screens stacked in it, the first at the bottom.
public struct Destination: Sendable {
    public var label: String
    public var icon: Navree.Icon?
    public var href: String
    public var badge: String?
    public var group: String?
    public var items: [Navree.NavItem]
    public var stack: [ScreenID]
}

/// Screens presented over everything else, with a stack of their own.
public struct Modal: Sendable {
    /// `sheet`, `full` or `task`; a word this build does not know is a sheet.
    public var present: Navree.Present
    /// The person may not swipe it away.
    public var locked: Bool
    public var stack: [ScreenID]
}

/// Where the person is: everything about navigation, and no screen.
public struct NavPart: Sendable {
    public var stage: Stage = .starting
    public var failure: Failure?
    public var upgrade: Navree.Upgrade?
    public var destinations: [Destination] = []
    public var selected: Int = 0
    public var modals: [Modal] = []
    public var front: ScreenID?
    public var canGoBack = false
    public var width: Width = .compact

    public init() {}

    /// Every screen some stack holds.
    public var held: Set<ScreenID> {
        Set(destinations.flatMap(\.stack) + modals.flatMap(\.stack))
    }
}

/// What a screen shows.
public enum ScreenContent: Sendable {
    /// Nothing yet.
    case blank
    /// A tree of parts, ready to draw.
    case native(Navree.Screen)
    /// The page in the web view; it is loaded again whenever `serial` moves on.
    case web(url: String, title: String?, why: [String], serial: Int)
}

/// A file the person picked, kept by the client under its own `token`.
public struct FileRef: Sendable, Codable, Hashable {
    public var token: String
    public var name: String
    public var type: String
    public var size: Int

    public init(token: String, name: String, type: String = "", size: Int = 0) {
        self.token = token
        self.name = name
        self.type = type
        self.size = size
    }
}

/// What a field holds, as the core keeps it and as a `field` event says it.
public enum FieldValue: Sendable, Hashable {
    case text(String)
    case flag(Bool)
    case many([String])
    case files([FileRef])
    case none
    case phone(text: String, country: String)
    case range(from: String, to: String)

    /// The value as a form's values: what a control shows.
    public var values: [String] {
        switch self {
        case .text(let text): [text]
        case .flag(let on): on ? ["on"] : []
        case .many(let values): values
        case .files(let files): files.map(\.name)
        case .none: []
        case .phone(let text, _): [text]
        case .range(let from, let to): [from, to]
        }
    }

    /// The value a control's new values make, in the shape the core keeps for
    /// this field: a switch stays a flag, several choices stay many, a choice
    /// that is cleared is none. A control says only its values; the field's
    /// own kind is the core's.
    public func replaced(with values: [String]) -> FieldValue {
        switch self {
        case .flag: return .flag(!values.isEmpty)
        case .many: return .many(values)
        case .files: return self
        case .range: return .range(from: values.first ?? "", to: values.dropFirst().first ?? "")
        case .phone(_, let country): return .phone(text: values.first ?? "", country: country)
        case .none: return values.first.map(FieldValue.text) ?? .none
        case .text: return .text(values.first ?? "")
        }
    }
}

/// What one field holds and what is wrong with it, and the options a search found for it.
public struct FieldView: Sendable {
    public var name: String
    public var value: FieldValue
    public var errors: [String]
    /// While present, the option list in place of the input's own.
    public var options: [Navree.ChoiceOption]?
    /// The searched list goes on.
    public var more: Bool
    /// A search for options is on its way.
    public var searching: Bool
}

/// The state of one form: whether it is out, and its fields by id.
public struct FormView: Sendable {
    public var pending: Bool
    public var fields: [String: FieldView]

    /// The id of the field with `name`, when one field has it.
    public func field(named name: String) -> (id: String, view: FieldView)? {
        let found = fields.filter { $0.value.name == name }.sorted { $0.key < $1.key }
        return found.first.map { ($0.key, $0.value) }
    }
}

/// The state of a part that fetches itself.
public struct RegionView: Sendable {
    public var loading: Bool
    public var failure: Failure?
}

/// The state of a list or table that loads more by cursor, at its end and before its first row.
public struct ListView: Sendable {
    public var loading: Bool
    public var failed: Bool
    public var loadingBefore: Bool
    public var failedBefore: Bool
    /// Every row of a table is chosen, also those not loaded.
    public var across: Bool
}

/// The confirmation the core asks the person for, in the server's words.
/// Every way out of it is an `answer`.
public struct Question: Sendable, Identifiable {
    public var id: Int
    public var title: String
    public var text: String?
    public var confirm: String
    public var cancel: String?
}

/// A request to move the focus to a field; answered with `focused`.
public struct Focus: Sendable {
    public var target: String
    public var serial: Int
}

/// One screen.
public struct ScreenState: Sendable {
    public var id: ScreenID
    public var url: String
    /// The title of the page the screen last showed: for the bar while it has nothing to show.
    public var title: String?
    public var content: ScreenContent
    /// A fetch is out: the content stays as it is.
    public var loading: Bool
    public var refreshing: Bool
    /// An old tree shown as a head start.
    public var cached: Bool
    public var failure: Failure?
    /// The HTTP status of the page: a 404 page is a screen too.
    public var status: Int
    /// Whether the app's bars stand around the screen.
    public var chrome: Bool
    /// The sheets, dialogs and disclosures of the tree that are open.
    public var overlays: [String]
    /// States of nodes that have no field for them, by node id.
    public var states: [String: [Navree.State]]
    public var forms: [String: FormView]
    public var regions: [String: RegionView]
    public var lists: [String: ListView]
    public var focus: Focus?
    public var question: Question?
    /// The id of the part on the screen under it this screen was opened from.
    public var from: String?

    /// The tree, when the screen shows one.
    public var screen: Navree.Screen? {
        if case .native(let screen) = content { return screen }
        return nil
    }

    /// Whether the core keeps `state` for the node with this id.
    public func has(_ state: Navree.State, _ id: String?) -> Bool {
        guard let id else { return false }
        return states[id]?.contains(state) ?? false
    }
}

/// A short message waiting to be shown; when it has gone, `message_shown`.
public struct Message: Sendable, Identifiable {
    public var id: Int
    public var tone: Navree.Tone
    public var text: String?
    public var title: String?
    public var notice: CoreNotice?
    /// The words of the message's button, when it has one.
    public var action: String?
}

/// The whole view state, as an empty query reads it.
public struct ViewState: Sendable {
    public var app: AppPart?
    public var nav: NavPart
    public var screens: [ScreenID: ScreenState]
    public var messages: [Message]
}

// MARK: - Reading

/// Coding keys made from any text.
struct JSONKey: CodingKey {
    var stringValue: String
    var intValue: Int? { nil }
    init(_ name: String) { stringValue = name }
    init?(stringValue: String) { self.stringValue = stringValue }
    init?(intValue: Int) { nil }
}

extension KeyedDecodingContainer where K == JSONKey {
    /// A value that may be missing or of a shape this build does not read:
    /// either way, `fallback`.
    func lenient<T: Decodable>(_ name: String, _ fallback: T) -> T {
        (try? decodeIfPresent(T.self, forKey: JSONKey(name))) ?? fallback
    }

    func optional<T: Decodable>(_ name: String) -> T? {
        try? decodeIfPresent(T.self, forKey: JSONKey(name))
    }

    /// A word of a closed list; one this build does not know is `nil`.
    func word<T: RawRepresentable>(_ name: String) -> T? where T.RawValue == String {
        (try? decodeIfPresent(String.self, forKey: JSONKey(name))).flatMap { $0.flatMap(T.init(rawValue:)) }
    }
}

extension AppPart: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        name = c.lenient("name", "")
        context = c.optional("context")
        home = c.lenient("home", "/")
        theme = c.optional("theme")
        user = try c.decodeIfPresent(Navree.User.self, forKey: JSONKey("user"))
        routes = c.lenient("routes", [])
    }
}

extension Destination: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        label = c.lenient("label", "")
        icon = c.optional("icon")
        href = c.lenient("href", "/")
        badge = c.optional("badge")
        group = c.optional("group")
        items = c.lenient("items", [])
        stack = c.lenient("stack", [])
    }
}

extension Modal: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        present = c.word("present") ?? .sheet
        locked = c.lenient("locked", false)
        stack = c.lenient("stack", [])
    }
}

extension NavPart: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        stage = c.word("stage") ?? .starting
        failure = c.word("failure")
        upgrade = c.optional("upgrade")
        destinations = try c.decodeIfPresent([Destination].self, forKey: JSONKey("destinations")) ?? []
        selected = c.lenient("selected", 0)
        modals = try c.decodeIfPresent([Modal].self, forKey: JSONKey("modals")) ?? []
        front = c.optional("front")
        canGoBack = c.lenient("can_go_back", false)
        let window = try? c.nestedContainer(keyedBy: JSONKey.self, forKey: JSONKey("window"))
        width = window?.word("width") ?? .compact
    }
}

extension ScreenContent: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        switch c.lenient("t", "blank") {
        case "native":
            self = .native(try c.decode(Navree.Screen.self, forKey: JSONKey("screen")))
        case "web":
            self = .web(
                url: c.lenient("url", ""), title: c.optional("title"), why: c.lenient("why", []),
                serial: c.lenient("serial", 0)
            )
        default:
            self = .blank
        }
    }
}

extension FieldValue: Codable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        switch c.lenient("t", "none") {
        case "text": self = .text(c.lenient("text", ""))
        case "flag": self = .flag(c.lenient("on", false))
        case "many": self = .many(c.lenient("values", []))
        case "files": self = .files(c.lenient("files", []))
        case "phone": self = .phone(text: c.lenient("text", ""), country: c.lenient("country", ""))
        case "range": self = .range(from: c.lenient("from", ""), to: c.lenient("to", ""))
        default: self = .none
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: JSONKey.self)
        switch self {
        case .text(let text):
            try c.encode("text", forKey: JSONKey("t"))
            try c.encode(text, forKey: JSONKey("text"))
        case .flag(let on):
            try c.encode("flag", forKey: JSONKey("t"))
            try c.encode(on, forKey: JSONKey("on"))
        case .many(let values):
            try c.encode("many", forKey: JSONKey("t"))
            try c.encode(values, forKey: JSONKey("values"))
        case .files(let files):
            try c.encode("files", forKey: JSONKey("t"))
            try c.encode(files, forKey: JSONKey("files"))
        case .none:
            try c.encode("none", forKey: JSONKey("t"))
        case .phone(let text, let country):
            try c.encode("phone", forKey: JSONKey("t"))
            try c.encode(text, forKey: JSONKey("text"))
            try c.encode(country, forKey: JSONKey("country"))
        case .range(let from, let to):
            try c.encode("range", forKey: JSONKey("t"))
            try c.encode(from, forKey: JSONKey("from"))
            try c.encode(to, forKey: JSONKey("to"))
        }
    }
}

extension FieldView: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        name = c.lenient("name", "")
        value = c.lenient("value", FieldValue.none)
        errors = c.lenient("errors", [])
        options = c.optional("options")
        more = c.lenient("more", false)
        searching = c.lenient("searching", false)
    }
}

extension FormView: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        pending = c.lenient("pending", false)
        fields = try c.decodeIfPresent([String: FieldView].self, forKey: JSONKey("fields")) ?? [:]
    }
}

extension RegionView: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        loading = c.lenient("loading", false)
        failure = c.word("failure")
    }
}

extension ListView: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        loading = c.lenient("loading", false)
        failed = c.lenient("failed", false)
        loadingBefore = c.lenient("loading_before", false)
        failedBefore = c.lenient("failed_before", false)
        across = c.lenient("across", false)
    }
}

extension Question: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        id = try c.decode(Int.self, forKey: JSONKey("id"))
        title = c.lenient("title", "")
        text = c.optional("text")
        confirm = c.lenient("confirm", "")
        cancel = c.optional("cancel")
    }
}

extension Focus: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        target = try c.decode(String.self, forKey: JSONKey("target"))
        serial = try c.decode(Int.self, forKey: JSONKey("serial"))
    }
}

extension ScreenState: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        id = try c.decode(Int.self, forKey: JSONKey("id"))
        url = c.lenient("url", "")
        title = c.optional("title")
        // A tree that does not read is a fault to report, never a screen drawn blank in silence.
        content = try c.decodeIfPresent(ScreenContent.self, forKey: JSONKey("content")) ?? .blank
        loading = c.lenient("loading", false)
        refreshing = c.lenient("refreshing", false)
        cached = c.lenient("cached", false)
        failure = c.word("failure")
        status = c.lenient("status", 0)
        chrome = c.lenient("chrome", true)
        overlays = c.lenient("overlays", [])
        let words: [String: [String]] = c.lenient("states", [:])
        states = words.mapValues { $0.compactMap(Navree.State.init(rawValue:)) }
        forms = try c.decodeIfPresent([String: FormView].self, forKey: JSONKey("forms")) ?? [:]
        regions = c.lenient("regions", [:])
        lists = c.lenient("lists", [:])
        focus = c.optional("focus")
        question = c.optional("question")
        from = c.optional("from")
    }
}

extension Message: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        id = try c.decode(Int.self, forKey: JSONKey("id"))
        tone = c.word("tone") ?? .neutral
        text = c.optional("text")
        title = c.optional("title")
        notice = c.word("notice")
        action = c.optional("action")
    }
}

extension ViewState: Decodable {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: JSONKey.self)
        app = try c.decodeIfPresent(AppPart.self, forKey: JSONKey("app"))
        nav = try NavPart(from: decoder)
        let screens = try c.decodeIfPresent([String: ScreenState].self, forKey: JSONKey("screens")) ?? [:]
        self.screens = Dictionary(screens.values.map { ($0.id, $0) }, uniquingKeysWith: { _, last in last })
        messages = try c.decodeIfPresent([Message].self, forKey: JSONKey("messages")) ?? []
    }
}
