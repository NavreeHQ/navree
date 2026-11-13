import Foundation
import NavreeProtocol

/// What happened, as the client tells the runtime core: one of these for
/// everything the person or the system did.
public enum CoreEvent: Sendable {
    /// The app starts; or "try again" on the start's failure screen.
    case start
    /// An app the system ended starts again with what was kept of it.
    case restore(snapshot: Data)
    /// A request came back, or did not.
    case response(Response)
    /// A tab, a sidebar item: the destination at this index.
    case select(destination: Int)
    /// Something with an action was tapped.
    case act(Act)
    /// The person answered the screen's question.
    case answer(screen: ScreenID, question: Int, yes: Bool)
    /// A row of a table that can be chosen was ticked or cleared; without a row, all of them.
    case choose(screen: ScreenID, table: String, row: String?, on: Bool, across: Bool)
    /// A tab of a set of tabs with an id was chosen.
    case tab(screen: ScreenID, id: String, index: Int)
    /// The secret bytes a `random` effect asked for, as hex.
    case random(request: Int, bytes: String)
    /// The system gave or changed the push token.
    case pushToken(token: String, sandbox: Bool)
    /// A row was dropped after a drag.
    case move(screen: ScreenID, item: String, from: String, to: String, before: String?)
    /// The search text of a searched choice changed, or the end of its options is near.
    case options(screen: ScreenID, form: String?, field: String, text: String?, more: Bool)
    /// A field changed: every keystroke, every toggle, every pick.
    case field(screen: ScreenID, form: String?, field: String, value: FieldValue)
    /// The screen's own search was submitted or cleared.
    case search(screen: ScreenID, text: String)
    /// The system's back.
    case back
    /// A back that took several screens away at once.
    case popTo(screen: ScreenID)
    /// A presented sheet or cover was swiped away.
    case dismissed(screen: ScreenID)
    /// A sheet, dialog or disclosure of the tree was opened or closed by the person.
    case overlay(screen: ScreenID, id: String, open: Bool)
    /// Pull to refresh; "try again" on a failed screen.
    case refresh(screen: ScreenID)
    case regionRetry(screen: ScreenID, region: String)
    /// The end of a list or table is near; with `before`, its start.
    case more(screen: ScreenID, id: String, before: Bool)
    /// A column heading, a table's previous page, a page that replaces the last.
    case regionGo(screen: ScreenID, region: String, href: String)
    case messageShown(id: Int)
    case messageAction(id: Int)
    /// A link was tapped in the web view (whose own navigation was cancelled).
    case webLink(screen: ScreenID, url: String)
    /// The app was opened with a link.
    case openLink(url: String)
    /// A notification was tapped.
    case notification(href: String)
    case window(width: Width)
    /// Only what changed.
    case environment(Environment)
    case foreground
    case background
    /// The client itself ended the session and threw the cookies away.
    case signedOut
    case timer(Int)
    case stream(stream: Int, state: StreamState)
    /// What the cache kept under `key`, or nothing.
    case cache(key: String, value: String?)
    case deviceResult(call: Int, outcome: DeviceOutcome)
    /// The sign-in session ended at this address, or the person gave up.
    case signInDone(url: String?)
    /// The focus moved as the screen asked.
    case focused(screen: ScreenID, serial: Int)
    case openUpgrade

    /// A control's action, where it stands and what it asks first.
    public struct Act: Sendable {
        /// The screen the control stands on; none for the account menu.
        public var screen: ScreenID?
        public var action: Navree.Action
        /// The nearest enclosing form's id.
        public var form: String?
        /// The nearest enclosing sheet, dialog or popover's id.
        public var overlay: String?
        /// What the control holds, when it holds text.
        public var text: String?
        /// The control's question, when it asks one first.
        public var ask: Navree.Question?
        /// The id of the part the control is.
        public var from: String?
        /// Seconds since 1970: for a device call, whose ticket may have run out.
        public var now: Int?

        public init(
            screen: ScreenID?, action: Navree.Action, form: String? = nil, overlay: String? = nil, text: String? = nil,
            ask: Navree.Question? = nil, from: String? = nil, now: Int? = nil
        ) {
            self.screen = screen
            self.action = action
            self.form = form
            self.overlay = overlay
            self.text = text
            self.ask = ask
            self.from = from
            self.now = now
        }
    }

    /// The person's settings, each only when it changed.
    public struct Environment: Sendable, Equatable {
        public var scheme: Navree.Scheme?
        public var contrast: Navree.Contrast?
        public var textPercent: Int?
        public var locale: String?
        public var tz: String?

        public init(
            scheme: Navree.Scheme? = nil, contrast: Navree.Contrast? = nil, textPercent: Int? = nil,
            locale: String? = nil, tz: String? = nil
        ) {
            self.scheme = scheme
            self.contrast = contrast
            self.textPercent = textPercent
            self.locale = locale
            self.tz = tz
        }

        public var isEmpty: Bool {
            scheme == nil && contrast == nil && textPercent == nil && locale == nil && tz == nil
        }
    }

    public enum StreamState: Sendable, Equatable {
        case open
        /// An event, by its `event:` name (none when it had no name). Its data is never read.
        case event(name: String?)
        /// The stream ended, with the HTTP status when there was one.
        case closed(status: Int?)
    }

    /// How a device call ended.
    public enum DeviceOutcome: Sendable, Equatable {
        /// Done, with the result's fields, every value as text.
        case done([Navree.Pair])
        case refused(Navree.Refusal)
    }
}

/// What came back for a `fetch`: the answer whatever its status, a redirect
/// as it came, or no answer at all.
public struct Response: Sendable, Equatable {
    public var request: Int
    public var status: Int
    public var contentType: String?
    /// A redirect's address, as it came.
    public var location: String?
    /// The answer's headers whose names start with `x-navree-`.
    public var headers: [[String]]
    public var body: String
    /// No answer: `offline`, `timeout` or `other`.
    public var error: String?

    public init(
        request: Int, status: Int, contentType: String? = nil, location: String? = nil, headers: [[String]] = [],
        body: String = ""
    ) {
        self.request = request
        self.status = status
        self.contentType = contentType
        self.location = location
        self.headers = headers
        self.body = body
        self.error = nil
    }

    /// No answer.
    public static func failed(_ request: Int, _ error: TransportError) -> Response {
        var response = Response(request: request, status: 0)
        response.error = error.rawValue
        return response
    }

    public enum TransportError: String, Sendable {
        case offline, timeout, other
    }

    /// Whether a status is a redirect the core follows itself.
    public static func isRedirect(_ status: Int) -> Bool {
        [301, 302, 303, 307, 308].contains(status)
    }
}

// MARK: - Writing

extension CoreEvent {
    /// The event as the core reads it.
    public var json: Data {
        if case .restore(let snapshot) = self {
            // The snapshot goes back exactly as the core wrote it.
            return Data(#"{"t":"restore","snapshot":"#.utf8) + snapshot + Data("}".utf8)
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return (try? encoder.encode(Written(event: self))) ?? Data(#"{"t":"unreadable"}"#.utf8)
    }

    /// The event's tag, for a log line that says nothing else about it.
    public var tag: String {
        switch self {
        case .start: "start"
        case .restore: "restore"
        case .response: "response"
        case .select: "select"
        case .act: "act"
        case .answer: "answer"
        case .choose: "choose"
        case .tab: "tab"
        case .random: "random"
        case .pushToken: "push_token"
        case .move: "move"
        case .options: "options"
        case .field: "field"
        case .search: "search"
        case .back: "back"
        case .popTo: "pop_to"
        case .dismissed: "dismissed"
        case .overlay: "overlay"
        case .refresh: "refresh"
        case .regionRetry: "region_retry"
        case .more: "more"
        case .regionGo: "region_go"
        case .messageShown: "message_shown"
        case .messageAction: "message_action"
        case .webLink: "web_link"
        case .openLink: "open_link"
        case .notification: "notification"
        case .window: "window"
        case .environment: "environment"
        case .foreground: "foreground"
        case .background: "background"
        case .signedOut: "signed_out"
        case .timer: "timer"
        case .stream: "stream"
        case .cache: "cache"
        case .deviceResult: "device_result"
        case .signInDone: "sign_in_done"
        case .focused: "focused"
        case .openUpgrade: "open_upgrade"
        }
    }
}

/// An event written out: its tag under `t`, then its fields.
private struct Written: Encodable {
    let event: CoreEvent

    func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: JSONKey.self)
        func put<T: Encodable>(_ name: String, _ value: T?) throws {
            if let value { try c.encode(value, forKey: JSONKey(name)) }
        }
        try put("t", event.tag)
        switch event {
        case .start, .restore, .back, .foreground, .background, .signedOut, .openUpgrade:
            break
        case .response(let response):
            try put("request", response.request)
            if response.status != 0 { try put("status", response.status) }
            try put("content_type", response.contentType)
            try put("location", response.location)
            if !response.headers.isEmpty { try put("headers", response.headers) }
            if !response.body.isEmpty { try put("body", response.body) }
            try put("error", response.error)
        case .select(let destination):
            try put("destination", destination)
        case .act(let act):
            try put("screen", act.screen)
            try put("action", act.action)
            try put("form", act.form)
            try put("overlay", act.overlay)
            try put("text", act.text)
            try put("ask", act.ask)
            try put("from", act.from)
            try put("now", act.now)
        case .answer(let screen, let question, let yes):
            try put("screen", screen)
            try put("question", question)
            try put("yes", yes)
        case .choose(let screen, let table, let row, let on, let across):
            try put("screen", screen)
            try put("table", table)
            try put("row", row)
            try put("on", on)
            if across { try put("across", true) }
        case .tab(let screen, let id, let index):
            try put("screen", screen)
            try put("id", id)
            try put("index", index)
        case .random(let request, let bytes):
            try put("request", request)
            try put("bytes", bytes)
        case .pushToken(let token, let sandbox):
            try put("token", token)
            if sandbox { try put("environment", "sandbox") }
        case .move(let screen, let item, let from, let to, let before):
            try put("screen", screen)
            try put("item", item)
            try put("from", from)
            try put("to", to)
            try put("before", before)
        case .options(let screen, let form, let field, let text, let more):
            try put("screen", screen)
            try put("form", form)
            try put("field", field)
            try put("text", text)
            if more { try put("more", true) }
        case .field(let screen, let form, let field, let value):
            try put("screen", screen)
            try put("form", form)
            try put("field", field)
            try put("value", value)
        case .search(let screen, let text):
            try put("screen", screen)
            try put("text", text)
        case .popTo(let screen), .dismissed(let screen), .refresh(let screen):
            try put("screen", screen)
        case .overlay(let screen, let id, let open):
            try put("screen", screen)
            try put("id", id)
            try put("open", open)
        case .regionRetry(let screen, let region):
            try put("screen", screen)
            try put("region", region)
        case .more(let screen, let id, let before):
            try put("screen", screen)
            try put("id", id)
            if before { try put("before", true) }
        case .regionGo(let screen, let region, let href):
            try put("screen", screen)
            try put("region", region)
            try put("href", href)
        case .messageShown(let id), .messageAction(let id):
            try put("id", id)
        case .webLink(let screen, let url):
            try put("screen", screen)
            try put("url", url)
        case .openLink(let url):
            try put("url", url)
        case .notification(let href):
            try put("href", href)
        case .window(let width):
            try put("width", width)
        case .environment(let environment):
            try put("scheme", environment.scheme)
            try put("contrast", environment.contrast)
            try put("text_percent", environment.textPercent)
            try put("locale", environment.locale)
            try put("tz", environment.tz)
        case .timer(let timer):
            try put("timer", timer)
        case .stream(let stream, let state):
            try put("stream", stream)
            switch state {
            case .open:
                try put("state", "open")
            case .event(let name):
                try put("state", "event")
                try put("name", name)
            case .closed(let status):
                try put("state", "closed")
                try put("status", status)
            }
        case .cache(let key, let value):
            try put("key", key)
            try put("value", value)
        case .deviceResult(let call, let outcome):
            try put("call", call)
            switch outcome {
            case .done(let fields):
                try put("ok", true)
                try put("fields", fields)
            case .refused(let refusal):
                try put("ok", false)
                try put("refusal", refusal.rawValue)
            }
        case .signInDone(let url):
            try put("url", url)
        case .focused(let screen, let serial):
            try put("screen", screen)
            try put("serial", serial)
        }
    }
}
