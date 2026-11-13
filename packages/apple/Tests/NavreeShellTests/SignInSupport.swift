import Foundation
import NavreeCoreBridge
import NavreeProtocol
import Testing

@testable import NavreeRuntime

@testable import NavreeShell

/// A conversation of the runtime core, as its transcript writes it.
struct Transcript {
    let name: String
    let object: [String: Any]

    init(_ name: String) throws {
        self.name = name
        let file = Fixtures.repository.appendingPathComponent("crates/navree-native-core/transcripts/\(name).json")
        object = try #require(try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
    }

    var steps: [[String: Any]] {
        object["steps"] as? [[String: Any]] ?? []
    }

    /// The core's configuration of the conversation: its replays' own, with the
    /// conversation's settings over it.
    var config: Data {
        var base: [String: Any] = [
            "origin": "https://shop.example",
            "client": ["platform": "ios", "form": "phone", "vocab": 0, "text_percent": 100] as [String: Any],
        ]
        for (key, value) in object["config"] as? [String: Any] ?? [:] { base[key] = value }
        return (try? JSONSerialization.data(withJSONObject: base)) ?? Data()
    }

    /// The event a step sends, with its fixture put in.
    static func event(_ step: [String: Any]) -> [String: Any] {
        var event = step["send"] as? [String: Any] ?? [:]
        if let body = step["body"] as? String { event["body"] = Fixtures.text(body) }
        return event
    }

    /// Every request the conversation expects the core to ask for, in order.
    var fetches: [Fetch] {
        steps.flatMap { step in
            ((step["expect"] as? [String: Any])?["effects"] as? [[String: Any]] ?? []).compactMap { effect in
                guard effect["t"] as? String == "fetch" else { return nil }
                return Fetch(
                    method: effect["method"] as? String ?? "get", url: effect["url"] as? String ?? "",
                    body: (effect["body"] as? [String: Any])?["text"] as? String
                )
            }
        }
    }

    /// Every effect of a kind the conversation expects, in order.
    func effects(_ tag: String) -> [[String: Any]] {
        steps.flatMap { step in
            ((step["expect"] as? [String: Any])?["effects"] as? [[String: Any]] ?? []).filter { $0["t"] as? String == tag }
        }
    }
}

/// The pairs of a form's body, decoded.
func formPairs(_ body: String?) -> [String: String] {
    var pairs: [String: String] = [:]
    for pair in (body ?? "").split(separator: "&") {
        let parts = pair.split(separator: "=", maxSplits: 1).map {
            String($0).replacingOccurrences(of: "+", with: " ").removingPercentEncoding ?? String($0)
        }
        pairs[parts[0]] = parts.count > 1 ? parts[1] : ""
    }
    return pairs
}

/// A request, by what it asks and what it sends.
struct Fetch: Equatable, CustomStringConvertible {
    var method: String
    var url: String
    var body: String?

    init(method: String, url: String, body: String?) {
        self.method = method
        self.url = url
        self.body = body
    }

    init(_ request: FetchRequest) {
        method = request.method
        url = request.url
        switch request.body {
        case .form(let text)?, .json(let text)?: body = text
        default: body = nil
        }
    }

    var description: String {
        "\(method) \(url) \(body ?? "")"
    }
}

/// One of the system's sign-in sheets, played: each request kept, each
/// answered with the next outcome given.
@MainActor
final class PlayedSheet: AuthorizationSheet {
    var outcomes: [SheetOutcome]
    var asked: [SheetRequest] = []
    /// Keeps the answer until `finish` is called, as a sheet the person still looks at.
    var holds = false
    private var waiting: (@MainActor (SheetOutcome) -> Void)?

    init(_ outcomes: [SheetOutcome] = []) {
        self.outcomes = outcomes
    }

    func present(_ request: SheetRequest, done: @escaping @MainActor (SheetOutcome) -> Void) {
        asked.append(request)
        if holds {
            waiting = done
            return
        }
        done(outcomes.isEmpty ? .refused(.cancelled) : outcomes.removeFirst())
    }

    func finish(_ outcome: SheetOutcome) {
        let done = waiting
        waiting = nil
        done?(outcome)
    }
}

/// The person's answer to the system's prompt to allow notifications.
@MainActor
final class PlayedPermission: NotificationPermission {
    var granted: Bool
    var asked = 0

    init(granted: Bool) {
        self.granted = granted
    }

    func ask(done: @escaping @MainActor (Bool) -> Void) {
        asked += 1
        done(granted)
    }
}

/// The system's sign-in session, played: each session it is asked to open is
/// kept, and ends when the test says how.
@MainActor
final class PlayedOpener: SignInOpener {
    struct Opened: Equatable {
        var url: URL
        var returning: SignInReturn
        var ephemeral: Bool
    }

    final class Handle: SignInHandle {
        var cancelled = false
        let done: @MainActor (URL?) -> Void

        init(done: @escaping @MainActor (URL?) -> Void) {
            self.done = done
        }

        func cancel() {
            cancelled = true
        }
    }

    var opened: [Opened] = []
    var handles: [Handle] = []
    /// Whether the system refuses to open a session.
    var refuses = false

    func open(_ url: URL, returning: SignInReturn, ephemeral: Bool, done: @escaping @MainActor (URL?) -> Void)
        -> (any SignInHandle)?
    {
        opened.append(Opened(url: url, returning: returning, ephemeral: ephemeral))
        guard !refuses else { return nil }
        let handle = Handle(done: done)
        handles.append(handle)
        return handle
    }

    /// The session open now: the last one opened, unless it was closed.
    var current: Handle? {
        handles.last.flatMap { $0.cancelled ? nil : $0 }
    }

    /// Ends the session open now, as the system would: with the address it
    /// came back on, or none. False when none is open.
    @discardableResult
    func finish(_ address: String?) -> Bool {
        guard let handle = current else { return false }
        handles.removeAll { $0 === handle }
        handle.done(address.flatMap(URL.init(string:)))
        return true
    }
}

/// Answers the core's requests at once, by address, from text the test
/// gives; keeps every request.
@MainActor
final class ScriptedTransport: Transport {
    var answers: [String: (status: Int, body: String)] = [:]
    var fetched: [FetchRequest] = []
    let jar: CookieJar? = nil

    func fetch(_ request: FetchRequest, answer: @escaping @MainActor (Response) -> Void) {
        fetched.append(request)
        let components = URLComponents(string: request.url)
        guard let found = answers[components?.percentEncodedPath ?? ""] else { return }
        let json = request.header("accept") == "application/json"
        answer(Response(
            request: request.request, status: found.status,
            contentType: json ? "application/json" : "application/vnd.navree.native+json", body: found.body
        ))
    }

    func cancel(_ request: Int) {}

    func open(
        stream: Int, url: String, headers: [(String, String)], report: @escaping @MainActor (CoreEvent.StreamState) -> Void
    ) {}

    func close(stream: Int) {}
}

/// A conversation of the core replayed with this client's own handlers in
/// the loop: the sheets, the prompt and the sign-in session are played, and
/// what the client sends for them takes the place of the transcript's own
/// event. The requests the core then asks for are kept, to be held to the
/// transcript's.
@MainActor
final class Replay: EffectRunner {
    let transcript: Transcript
    let client: NativeClient
    let calls: DeviceCalls
    let session: SignInSession
    let sheet: PlayedSheet
    let permission: PlayedPermission
    let opener = PlayedOpener()
    var fetched: [FetchRequest] = []
    var problems: [String] = []
    /// The device calls this client answered itself.
    private var answered: Set<Int> = []

    init(
        _ name: String, signedWith: Set<Entitlement>, sheet: PlayedSheet = PlayedSheet(),
        permission: PlayedPermission = PlayedPermission(granted: true)
    ) throws {
        transcript = try Transcript(name)
        let core = try #require(Core(config: transcript.config))
        let origin = URL(string: "https://shop.example")
        calls = DeviceCalls(signedWith: signedWith, origin: origin)
        session = SignInSession(signedWith: signedWith, origin: origin)
        self.sheet = sheet
        self.permission = permission
        calls.sheet = sheet
        calls.permission = permission
        session.opener = opener
        client = NativeClient(core: core)
        client.effects = self
        client.problem = { [weak self] in self?.problems.append($0) }
    }

    func run(_ effect: CoreEffect, for client: NativeClient) {
        switch effect {
        case .fetch(let request):
            fetched.append(request)
        case .device(let call, let device, let posts, let options) where Self.ours(device):
            answered.insert(call)
            calls.answer = { [weak client] call, outcome in client?.send(.deviceResult(call: call, outcome: outcome)) }
            calls.run(call: call, device: device, posts: posts, options: options)
        case .signIn(let url, let callback, let ephemeral):
            guard let address = URL(string: url) else { return }
            session.start(address, callback: callback, ephemeral: ephemeral) { [weak client] returned in
                client?.send(.signInDone(url: returned?.absoluteString))
            }
        default:
            break
        }
    }

    /// The calls this client answers with a sheet or a prompt.
    static func ours(_ device: DeviceRequest) -> Bool {
        if case .call(.registerPush) = device { return true }
        return DeviceCalls.usesSheet(device)
    }

    /// Sends every step's event, except what the client's own handlers said in its place.
    func play() {
        for step in transcript.steps {
            let event = Transcript.event(step)
            let tag = event["t"] as? String ?? "?"
            if tag == "device_result", let call = event["call"] as? Int, answered.contains(call) { continue }
            if tag == "sign_in_done", opener.finish(event["url"] as? String) { continue }
            client.send(json: (try? JSONSerialization.data(withJSONObject: event)) ?? Data(), tag: tag)
        }
    }
}
