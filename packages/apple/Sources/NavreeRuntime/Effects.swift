import Foundation
import NavreeProtocol

/// What the runtime core asks the client to do. Whatever comes of one is a
/// later event.
public enum CoreEffect: Sendable {
    case fetch(FetchRequest)
    /// Abort a request if it can be; an answer that still comes is ignored.
    case cancel(request: Int)
    /// The system browser or the handler of the address; with `inside`, the
    /// system's browser inside the app.
    case openURL(url: String, inside: Bool)
    /// The system's sign-in session at exactly this address, coming back on `callback`.
    case signIn(url: String, callback: String?, ephemeral: Bool)
    /// A message to show; the same list is the view state's `messages`.
    case message(Message)
    case haptic(kind: String)
    /// A device call: the protocol's call, whether its result is posted, and
    /// what the server answered the core for it.
    case device(call: Int, device: DeviceRequest, posts: Bool, options: Navree.JSONValue?)
    /// That many secret bytes from the system's generator, answered at once.
    case random(request: Int, bytes: Int)
    /// The person signed out: every cookie and all web storage of the app go.
    case forgetSession
    case cacheRead(key: String)
    /// Without a value, the key is deleted.
    case cacheWrite(key: String, value: String?)
    case cacheClear
    case announce(text: String?, notice: CoreNotice?, count: Int?)
    case copy(text: String, sensitive: Bool)
    case timer(Int, ms: Int)
    case cancelTimer(Int)
    case streamOpen(stream: Int, url: String, headers: [(String, String)])
    case streamClose(stream: Int)
    /// Nothing to go back to: the system may have the gesture.
    case exit
    /// Something to log: an answer that did not read, a kind never heard of.
    case report(what: String, detail: String)
    /// An effect this build does not know, by its tag.
    case unknown(String)
}

/// What a device call asks for: a call a tap started, or one a reply may start.
public enum DeviceRequest: Sendable, Equatable {
    case call(Navree.Call)
    case reply(Navree.ReplyCall)

    /// The call's name, as the protocol has it.
    public var name: String {
        switch self {
        case .call(.unknown(let unknown)): unknown.tag
        case .reply(.unknown(let unknown)): unknown.tag
        case .reply(.requestReview): "request_review"
        case .reply(.haptic): "haptic"
        case .reply(.setBadge): "set_badge"
        case .reply(.cancelNotification): "cancel_notification"
        case .call(let call):
            switch call {
            case .haptic: "haptic"
            case .copy: "copy"
            case .share: "share"
            case .openUrl: "open_url"
            case .authSession: "auth_session"
            case .registerPush: "register_push"
            case .notify: "notify"
            case .cancelNotification: "cancel_notification"
            case .setBadge: "set_badge"
            case .openSettings: "open_settings"
            case .passkey: "passkey"
            case .confirm: "confirm"
            case .locateOnce: "locate_once"
            case .scanCode: "scan_code"
            case .platformSignIn: "platform_sign_in"
            case .unknown(let unknown): unknown.tag
            }
        }
    }

    /// Whether this build's models know the call.
    public var isKnown: Bool {
        switch self {
        case .call(.unknown), .reply(.unknown): false
        default: true
        }
    }
}

/// A request the core asks for, to be sent exactly as it is.
public struct FetchRequest: Sendable {
    public var request: Int
    /// `get`, `post` or `delete`.
    public var method: String
    public var url: String
    public var headers: [(String, String)]
    public var body: Body?
    public var timeoutMs: Int
    /// Add `x-xsrf-token` from the `XSRF-TOKEN` cookie.
    public var xsrf: Bool

    public init(
        request: Int, method: String = "get", url: String, headers: [(String, String)] = [], body: Body? = nil,
        timeoutMs: Int = 30_000, xsrf: Bool = false
    ) {
        self.request = request
        self.method = method
        self.url = url
        self.headers = headers
        self.body = body
        self.timeoutMs = timeoutMs
        self.xsrf = xsrf
    }

    /// What a request sends.
    public enum Body: Sendable {
        /// Sent as the body's bytes, even when empty; the content type is in the headers.
        case form(String)
        /// The same, for JSON.
        case json(String)
        /// Parts in order; a file's bytes are the ones the client keeps under its token.
        case multipart([Part])
    }

    public enum Part: Sendable {
        case text(name: String, value: String)
        case file(name: String, token: String, filename: String, type: String)
    }

    /// The value of a header, by its name in any case.
    public func header(_ name: String) -> String? {
        headers.first { $0.0.caseInsensitiveCompare(name) == .orderedSame }?.1
    }
}

/// A part of the view state that an event changed: read it again.
public enum Change: Sendable, Equatable {
    case app
    case nav
    case screen(ScreenID)
    case form(screen: ScreenID, form: String)
    case rows(screen: ScreenID, id: String)
    case redraw(screen: ScreenID, id: String)
    case messages
    /// A part this build does not know: everything is read again.
    case unknown
}

/// What one event did: the parts it changed and the effects to carry out, in order.
public struct Update: Sendable {
    public var changed: [Change]
    public var effects: [CoreEffect]
    /// Effects that did not read, by what was wrong: each a fault to log.
    public var unread: [String]
}

// MARK: - Reading

extension Update {
    /// Reads an update, or the core's `{"error":…}`.
    static func read(_ data: Data) -> Result<Update, CoreError> {
        guard let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            return .failure(.unreadable)
        }
        if let error = object["error"] as? String {
            return .failure(.refused(error))
        }
        let changed = (object["changed"] as? [[String: Any]] ?? []).map(Change.init)
        var effects: [CoreEffect] = []
        var unread: [String] = []
        for effect in object["effects"] as? [Any] ?? [] {
            do {
                effects.append(try CoreEffect.read(effect))
            } catch {
                unread.append((effect as? [String: Any])?["t"] as? String ?? "?")
            }
        }
        return .success(Update(changed: changed, effects: effects, unread: unread))
    }
}

/// What went wrong with the core itself: a bug to log, never something to draw.
public enum CoreError: Error, Equatable, Sendable {
    /// The core answered `{"error":…}`.
    case refused(String)
    /// What came back is not JSON the client reads.
    case unreadable
}

extension Change {
    init(_ object: [String: Any]) {
        let screen = object["screen"] as? Int
        switch object["t"] as? String {
        case "app": self = .app
        case "nav": self = .nav
        case "screen": self = (object["id"] as? Int).map(Change.screen) ?? .unknown
        case "form":
            if let screen, let form = object["form"] as? String { self = .form(screen: screen, form: form) } else { self = .unknown }
        case "rows":
            if let screen, let id = object["id"] as? String { self = .rows(screen: screen, id: id) } else { self = .unknown }
        case "redraw":
            if let screen, let id = object["id"] as? String { self = .redraw(screen: screen, id: id) } else { self = .unknown }
        case "messages": self = .messages
        default: self = .unknown
        }
    }
}

/// An effect that did not read.
struct UnreadEffect: Error {}

extension CoreEffect {
    static func read(_ value: Any) throws -> CoreEffect {
        guard let o = value as? [String: Any], let t = o["t"] as? String else { throw UnreadEffect() }
        func s(_ name: String) -> String? { o[name] as? String }
        func i(_ name: String) -> Int? { (o[name] as? NSNumber)?.intValue }
        func b(_ name: String) -> Bool { (o[name] as? Bool) ?? false }
        func need<T>(_ value: T?) throws -> T {
            guard let value else { throw UnreadEffect() }
            return value
        }
        switch t {
        case "fetch":
            return .fetch(FetchRequest(
                request: try need(i("request")), method: s("method") ?? "get", url: try need(s("url")),
                headers: pairs(o["headers"]), body: try body(o["body"]), timeoutMs: i("timeout_ms") ?? 30_000,
                xsrf: b("xsrf")
            ))
        case "cancel": return .cancel(request: try need(i("request")))
        case "open_url": return .openURL(url: try need(s("url")), inside: b("inside"))
        case "sign_in": return .signIn(url: try need(s("url")), callback: s("callback"), ephemeral: b("ephemeral"))
        case "message": return .message(try decode(Message.self, o))
        case "haptic": return .haptic(kind: s("kind") ?? "light")
        case "device":
            let raw = try need(o["device"])
            var device = DeviceRequest.call(try decode(Navree.Call.self, raw))
            if !device.isKnown, let reply = try? decode(Navree.ReplyCall.self, raw) {
                device = .reply(reply)
            }
            let options = try o["options"].flatMap { $0 is NSNull ? nil : try decode(Navree.JSONValue.self, $0) }
            return .device(call: try need(i("call")), device: device, posts: b("posts"), options: options)
        case "random": return .random(request: try need(i("request")), bytes: i("bytes") ?? 48)
        case "forget_session": return .forgetSession
        case "cache_read": return .cacheRead(key: try need(s("key")))
        case "cache_write": return .cacheWrite(key: try need(s("key")), value: s("value"))
        case "cache_clear": return .cacheClear
        case "announce": return .announce(text: s("text"), notice: s("notice").flatMap(CoreNotice.init), count: i("count"))
        case "copy": return .copy(text: try need(s("text")), sensitive: b("sensitive"))
        case "timer": return .timer(try need(i("timer")), ms: i("ms") ?? 0)
        case "cancel_timer": return .cancelTimer(try need(i("timer")))
        case "stream_open":
            return .streamOpen(stream: try need(i("stream")), url: try need(s("url")), headers: pairs(o["headers"]))
        case "stream_close": return .streamClose(stream: try need(i("stream")))
        case "exit": return .exit
        case "report": return .report(what: s("what") ?? "", detail: s("detail") ?? "")
        default: return .unknown(t)
        }
    }

    static func pairs(_ value: Any?) -> [(String, String)] {
        (value as? [[Any]] ?? []).compactMap { pair in
            guard let name = pair.first as? String else { return nil }
            return (name, pair.dropFirst().first as? String ?? "")
        }
    }

    static func body(_ value: Any?) throws -> FetchRequest.Body? {
        guard let o = value as? [String: Any] else { return nil }
        switch o["t"] as? String {
        case "form": return .form(o["text"] as? String ?? "")
        case "json": return .json(o["text"] as? String ?? "")
        case "multipart":
            let parts = (o["parts"] as? [[String: Any]] ?? []).compactMap { part -> FetchRequest.Part? in
                switch part["t"] as? String {
                case "text":
                    return .text(name: part["name"] as? String ?? "", value: part["value"] as? String ?? "")
                case "file":
                    return .file(
                        name: part["name"] as? String ?? "", token: part["token"] as? String ?? "",
                        filename: part["filename"] as? String ?? "",
                        type: part["type"] as? String ?? "application/octet-stream"
                    )
                default:
                    return nil
                }
            }
            return .multipart(parts)
        default:
            throw UnreadEffect()
        }
    }

    /// Reads a value of the generated models from what `JSONSerialization` read.
    static func decode<T: Decodable>(_ type: T.Type, _ value: Any) throws -> T {
        let data = try JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed])
        return try JSONDecoder().decode(type, from: data)
    }
}
