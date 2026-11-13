import Foundation
import NavreeCoreBridge
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

/// The runtime core's own recorded answers.
enum Fixtures {
    static let repository = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()  // NavreeShellTests
        .deletingLastPathComponent()  // Tests
        .deletingLastPathComponent()  // apple
        .deletingLastPathComponent()  // packages
        .deletingLastPathComponent()

    static let folder = repository.appendingPathComponent("crates/navree-native-core/transcripts/fixtures")

    static func text(_ path: String) -> String {
        (try? String(contentsOf: folder.appendingPathComponent(path), encoding: .utf8)) ?? ""
    }
}

/// Answers the core's requests from recorded files, at once, by address.
@MainActor
final class FakeTransport: Transport {
    var answers: [String: (status: Int, file: String)] = [:]
    var fetched: [FetchRequest] = []
    var cancelled: [Int] = []
    var streams: [Int: String] = [:]
    var closed: [Int] = []
    let jar: CookieJar? = CookieJar(origin: "https://shop.example", persistent: false)

    func fetch(_ request: FetchRequest, answer: @escaping @MainActor (Response) -> Void) {
        fetched.append(request)
        let components = URLComponents(string: request.url)
        let address = (components?.percentEncodedPath ?? "") + (components?.percentEncodedQuery.map { "?" + $0 } ?? "")
        guard let found = answers[address] else { return }
        answer(Response(
            request: request.request, status: found.status, contentType: "application/vnd.navree.native+json",
            body: Fixtures.text(found.file)
        ))
    }

    func cancel(_ request: Int) {
        cancelled.append(request)
    }

    func open(
        stream: Int, url: String, headers: [(String, String)], report: @escaping @MainActor (CoreEvent.StreamState) -> Void
    ) {
        streams[stream] = url
    }

    func close(stream: Int) {
        closed.append(stream)
    }
}

/// A client on a fresh core whose effects the system's runner carries out,
/// over a transport that answers from recorded files.
@MainActor
struct Shell {
    let client: NativeClient
    let transport: FakeTransport
    let effects: SystemEffects
    let folder: URL
    let problems: Problems

    final class Problems {
        var list: [String] = []
    }

    init(answers: [String: (status: Int, file: String)] = Shell.store) throws {
        let configuration = ShellConfiguration(origin: "https://shop.example", appID: "com.navree.demo", appVersion: "1.0")
        let core = try #require(Core(config: CoreConfiguration.json(configuration, device: DeviceTraits())))
        transport = FakeTransport()
        transport.answers = answers
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-\(UUID().uuidString)")
        effects = SystemEffects(
            transport: transport, cache: KeptFiles(folder: folder.appendingPathComponent("cache")),
            snapshots: KeptFiles(folder: folder.appendingPathComponent("snapshot"))
        )
        client = NativeClient(core: core, effects: effects)
        let problems = Problems()
        self.problems = problems
        client.problem = { problems.list.append($0) }
    }

    /// The store's shell, dashboard, orders and an order.
    static let store: [String: (status: Int, file: String)] = [
        "/_navree/native/shell": (200, "store/shell.json"),
        "/": (200, "store/dashboard.json"),
        "/orders": (200, "store/orders.json"),
        "/orders/1": (200, "store/order.json"),
        "/orders/1/refund": (422, "made/refund-refused.json"),
    ]

    /// The order, open on the orders' stack.
    func orderOpen() -> ScreenID {
        client.begin()
        client.send(.select(destination: 1))
        client.send(.act(.init(screen: client.nav.front ?? 0, action: .navigate(.init(href: "/orders/1")))))
        return client.nav.front ?? 0
    }
}
