import Foundation
import NavreeCoreBridge
import Testing

/// An iPhone app against a store demo served on this machine, as the demo
/// app is configured.
enum Local {
    static let origin = "http://127.0.0.1:8095"

    static var configuration: [String: Any] {
        [
            "origin": origin,
            "loopback_http": true,
            "app_id": "com.navree.demo",
            "client": [
                "platform": "ios", "os": "26.0", "form": "phone", "app": "0.1.0", "lib": "0.1.0", "vocab": 0,
                "scheme": "light", "contrast": "normal", "text_percent": 100, "locale": "en-IN",
                "tz": "Asia/Kolkata", "caps": ["clipboard", "share", "open_url", "auth_session"],
            ] as [String: Any],
            "back_to_first": false,
            "sign_in": ["paths": ["/auth/*/redirect"], "native": ["apple"]],
        ]
    }

    static func core() throws -> Core {
        try #require(Core(config: json(configuration)))
    }
}

/// The store demo's answers the core's own conversations are replayed with.
enum Store {
    static let folder = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()  // NavreeCoreBridgeTests
        .deletingLastPathComponent()  // Tests
        .deletingLastPathComponent()  // apple
        .deletingLastPathComponent()  // packages
        .deletingLastPathComponent()
        .appendingPathComponent("crates/navree-native-core/transcripts/fixtures/store")

    /// The `response` event for request `request`, answered with `name`.
    static func answer(_ request: Int, with name: String) throws -> [String: Any] {
        let body = try String(contentsOf: folder.appendingPathComponent("\(name).json"), encoding: .utf8)
        return [
            "t": "response", "request": request, "status": 200,
            "content_type": "application/vnd.navree.native+json", "headers": [] as [Any], "body": body,
        ]
    }
}

func json(_ value: Any) -> Data {
    (try? JSONSerialization.data(withJSONObject: value)) ?? Data()
}

func object(_ data: Data) throws -> [String: Any] {
    try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any], "\(String(decoding: data, as: UTF8.self))")
}

extension Core {
    /// Sends `event` and reads the update, which must not be an error.
    func send(_ event: [String: Any]) throws -> [String: Any] {
        let update = try object(send(json(event)))
        #expect(update["error"] == nil, "\(update)")
        return update
    }

    /// Reads the part `query` names, which must not be an error.
    func read(_ query: [String: Any]) throws -> [String: Any] {
        let part = try object(view(json(query)))
        #expect(part["error"] == nil, "\(part)")
        return part
    }
}

extension [String: Any] {
    /// The entries of the update's `changed`, written as `app`, `nav`, `screen 1`.
    var changes: [String] {
        (self["changed"] as? [[String: Any]] ?? []).map { change in
            [change["t"] as? String, (change["id"] as? Int).map(String.init)].compactMap { $0 }.joined(separator: " ")
        }
    }

    var effects: [[String: Any]] {
        self["effects"] as? [[String: Any]] ?? []
    }
}

extension [[String: Any]] {
    /// The `fetch` effects, as their request numbers and addresses.
    var fetches: [Fetch] {
        filter { $0["t"] as? String == "fetch" }.map {
            Fetch(request: $0["request"] as? Int ?? 0, method: $0["method"] as? String ?? "", url: $0["url"] as? String ?? "")
        }
    }
}

struct Fetch: Equatable {
    var request: Int
    var method: String
    var url: String
}

@Test func aConfigurationThatDoesNotReadMakesNoCore() {
    #expect(Core(config: Data("{".utf8)) == nil)
    #expect(Core(config: Data()) == nil)
    #expect(Core(config: json(["client": ["platform": "ios"]])) == nil)
}

@Test func startingAsksForTheShell() throws {
    let core = try Local.core()
    let update = try core.send(["t": "start"])
    #expect(update.changes.isEmpty)
    #expect(update.effects.fetches == [Fetch(request: 1, method: "get", url: "\(Local.origin)/_navree/native/shell")])

    let fetch = try #require(update.effects.first)
    let headers = (fetch["headers"] as? [[String]] ?? []).map { ($0.first ?? "", $0.last ?? "") }
    #expect(headers.contains { $0 == ("x-navree-native", "1") })
    #expect(headers.contains { $0 == ("accept", "application/vnd.navree.native+json") })
    let descriptor = try #require(headers.first { $0.0 == "x-navree-client" }?.1)
    #expect(descriptor.hasPrefix("platform=ios;"), "\(descriptor)")
    #expect(fetch["timeout_ms"] as? Int ?? 0 > 0)

    // Starting twice asks once.
    let again = try core.send(["t": "start"])
    #expect(again.changes.isEmpty && again.effects.isEmpty)
}

@Test func theWholeViewStateReadsWithAnEmptyQuery() throws {
    let core = try Local.core()
    _ = try core.send(["t": "start"])
    let state = try object(core.view())
    for key in ["stage", "destinations", "selected", "modals", "can_go_back", "window", "screens", "messages"] {
        #expect(state[key] != nil, "\(key) is missing from \(state.keys.sorted())")
    }
    #expect(state["stage"] as? String == "starting")
    #expect((state["destinations"] as? [Any])?.isEmpty == true)
    #expect((state["modals"] as? [Any])?.isEmpty == true)
    #expect((state["screens"] as? [String: Any])?.isEmpty == true)
    #expect(state["can_go_back"] as? Bool == false)
    #expect((state["window"] as? [String: Any])?["width"] is String)
}

@Test func whatDoesNotReadIsAnErrorAndTheCoreGoesOn() throws {
    let core = try Local.core()
    for event in [Data(#"{"t":"teleport"}"#.utf8), Data("not an event".utf8), Data()] {
        let answer = try object(core.send(event))
        #expect(answer["error"] is String, "\(answer)")
        #expect(answer["changed"] == nil)
    }
    let query = try object(core.view(Data(#"{"part":"nowhere"}"#.utf8)))
    #expect(query["error"] is String, "\(query)")

    let update = try core.send(["t": "start"])
    #expect(update.effects.fetches.map(\.request) == [1])
}

@Test func theShellBecomesTheNavigation() throws {
    let core = try Local.core()
    _ = try core.send(["t": "start"])
    let update = try core.send(Store.answer(1, with: "shell"))
    #expect(update.changes == ["app", "nav", "screen 1"])
    #expect(update.effects.fetches == [Fetch(request: 2, method: "get", url: "\(Local.origin)/")])

    let app = try core.read(["part": "app"])
    #expect(app["name"] as? String == "Charkha Supply Co.")
    #expect((app["user"] as? [String: Any])?["name"] as? String == "Ravneet Kaur")

    let nav = try core.read(["part": "nav"])
    #expect(nav["stage"] as? String == "ready")
    let destinations = try #require(nav["destinations"] as? [[String: Any]])
    #expect(destinations.map { $0["label"] as? String } == ["Dashboard", "Orders", "Inventory"])
    #expect(destinations.map { $0["href"] as? String } == ["/", "/orders", "/inventory"])
    #expect(destinations.first?["stack"] as? [Int] == [1])
    #expect(nav["selected"] as? Int == 0)
    #expect(nav["front"] as? Int == 1)
    #expect(nav["can_go_back"] as? Bool == false)

    let screen = try core.read(["part": "screen", "id": 1])
    #expect(screen["loading"] as? Bool == true)
    #expect((screen["content"] as? [String: Any])?["t"] as? String == "blank")
}

@Test func aScreenIsPushedAndTheBackGestureTakesItOff() throws {
    let core = try Local.core()
    _ = try core.send(["t": "start"])
    _ = try core.send(Store.answer(1, with: "shell"))

    // The dashboard's answer sends it to the web view, at its address on the origin.
    #expect(try core.send(Store.answer(2, with: "dashboard")).changes == ["screen 1"])
    let dashboard = try #require(try core.read(["part": "screen", "id": 1])["content"] as? [String: Any])
    #expect(dashboard["t"] as? String == "web")
    #expect(dashboard["url"] as? String == "\(Local.origin)/")

    // Orders, the second destination, is drawn natively.
    let selected = try core.send(["t": "select", "destination": 1])
    #expect(selected.effects.fetches == [Fetch(request: 3, method: "get", url: "\(Local.origin)/orders")])
    #expect(try core.send(Store.answer(3, with: "orders")).changes == ["screen 2"])
    let orders = try core.read(["part": "screen", "id": 2])
    #expect((orders["content"] as? [String: Any])?["t"] as? String == "native")
    #expect(orders["loading"] as? Bool == false)

    // A row's link pushes the order over the list.
    let opened = try core.send(["t": "act", "screen": 2, "action": ["t": "navigate", "href": "/orders/1"]])
    #expect(opened.changes == ["nav", "screen 3"])
    #expect(opened.effects.fetches == [Fetch(request: 4, method: "get", url: "\(Local.origin)/orders/1")])
    var nav = try core.read(["part": "nav"])
    #expect(nav["selected"] as? Int == 1)
    #expect((nav["destinations"] as? [[String: Any]])?[1]["stack"] as? [Int] == [2, 3])
    #expect(nav["front"] as? Int == 3)
    #expect(nav["can_go_back"] as? Bool == true)

    _ = try core.send(Store.answer(4, with: "order"))
    let back = try core.send(["t": "back"])
    #expect(back.changes.contains("nav"))
    nav = try core.read(["part": "nav"])
    #expect((nav["destinations"] as? [[String: Any]])?[1]["stack"] as? [Int] == [2])
    #expect(nav["front"] as? Int == 2)
    #expect(nav["can_go_back"] as? Bool == false)
}

@Test func coresAreMadeAndEndedOneAfterAnother() throws {
    for _ in 0..<32 {
        let core = try Local.core()
        _ = try core.send(["t": "start"])
        _ = try core.send(Store.answer(1, with: "shell"))
    }
}
