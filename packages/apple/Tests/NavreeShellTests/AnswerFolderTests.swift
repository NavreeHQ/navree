import Foundation
import NavreeCoreBridge
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

@MainActor
struct AnswerFolderTests {
    @Test func aManifestIsReadByItsHeadersAndPassesOverOtherColumns() throws {
        let rows = try #require(AnswerFolder.rows("url\tbytes\tstatus\tfile\n/\t10\t200\ttoday.json\n/sleep\t9\t404\tgone.json\n\n"))
        #expect(rows == [
            AnswerFolder.Row(file: "today.json", status: 200, url: "/"),
            AnswerFolder.Row(file: "gone.json", status: 404, url: "/sleep"),
        ])
        #expect(AnswerFolder.rows("file\tstatus\n") == nil)
    }

    @Test func rowsOfOneAddressAreServedInOrderAndThenTheLastAgain() throws {
        let folder = try Self.folder([("a.json", "{}"), ("b.json", "{}")], manifest: "file\tstatus\turl\na.json\t200\t/\nb.json\t200\t/\n")
        let answers = try #require(AnswerFolder(folder: folder))
        #expect(answers.next(for: "https://answers.example/")?.file == "a.json")
        #expect(answers.next(for: "https://answers.example/")?.file == "b.json")
        #expect(answers.next(for: "https://answers.example/")?.file == "b.json")
        #expect(answers.next(for: "https://answers.example/?date=2026-11-11") == nil)
        #expect(AnswerFolder.origin(of: folder) == nil)
    }

    @Test func thePublicCorpusInAFolderIsDrawnThroughTheCore() async throws {
        let corpus = Fixtures.repository.appendingPathComponent("crates/navree-native/corpus")
        let files = ["shell.json", "screen-list.json", "screen-today.json", "screen-detail.json"]
        let folder = try Self.folder(
            files.map { ($0, (try? String(contentsOf: corpus.appendingPathComponent($0), encoding: .utf8)) ?? "") },
            manifest: "file\tstatus\tbytes\turl\nshell.json\t200\t1\t/_navree/native/shell\nscreen-today.json\t200\t1\t/\n"
                + "screen-list.json\t200\t1\t/orders\nscreen-detail.json\t200\t1\t/orders/1042\n"
        )
        let answers = try #require(AnswerFolder(folder: folder))
        let configuration = ShellConfiguration(origin: "https://answers.example", appVersion: "1.0", keepsSession: false)
        let core = try #require(Core(config: CoreConfiguration.json(configuration, device: DeviceTraits())))
        let effects = SystemEffects(
            transport: answers, cache: KeptFiles(folder: folder.appendingPathComponent("cache")),
            snapshots: KeptFiles(folder: folder.appendingPathComponent("snapshot"))
        )
        let client = NativeClient(core: core, effects: effects)
        var problems: [String] = []
        client.problem = { problems.append($0) }
        client.begin()
        try await Self.until { client.screen(1).map { !$0.loading } ?? false }
        #expect(client.app?.name == "Charkha Supply Co.")
        #expect(client.nav.destinations.map(\.label) == ["Dashboard", "Orders", "Inventory"])
        // Today is a health screen of rings and a day clock: drawn natively.
        #expect(client.screen(1)?.screen?.title == "Today")

        client.send(.select(destination: 1))
        let orders = try #require(client.nav.front)
        try await Self.until { client.screen(orders).map { !$0.loading } ?? false }
        // The orders hold a range of days, which this client draws: natively too.
        #expect(client.screen(orders)?.screen?.title == "Orders")
        client.send(.act(.init(screen: orders, action: .navigate(.init(href: "/orders/1042")))))
        let order = try #require(client.nav.front)
        try await Self.until { client.screen(order).map { !$0.loading } ?? false }
        #expect(client.screen(order)?.screen?.title == "Order CH-1042")
        #expect(problems.isEmpty, "\(problems)")
    }

    /// A folder of answers laid out for the test.
    static func folder(_ files: [(String, String)], manifest: String) throws -> URL {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("answers-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for (name, text) in files {
            try text.write(to: folder.appendingPathComponent(name), atomically: true, encoding: .utf8)
        }
        try manifest.write(to: folder.appendingPathComponent("manifest.tsv"), atomically: true, encoding: .utf8)
        return folder
    }

    /// Waits, a little at a time, for what the answers bring.
    static func until(_ done: @MainActor () -> Bool) async throws {
        for _ in 0..<200 where !done() {
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(done())
    }
}

/// Every screen of the public corpus, served as a destination's first page,
/// reads through the core with nothing refused: drawn natively, or the web
/// view's when it holds what this client does not draw yet.
@MainActor
@Test(arguments: [
    "screen-board.json", "screen-dashboard.json", "screen-designed-health.json", "screen-designed-store.json",
    "screen-detail.json", "screen-device.json", "screen-fitness.json", "screen-form.json", "screen-inbox.json",
    "screen-kinds.json", "screen-list.json", "screen-sleep.json", "screen-today.json", "screen-with-toast.json",
])
func aScreenOfTheCorpusReadsThroughTheCore(_ file: String) async throws {
    let corpus = Fixtures.repository.appendingPathComponent("crates/navree-native/corpus")
    let shell = try Shell(answers: [:])
    shell.transport.answers = [:]
    let folder = try AnswerFolderTests.folder(
        [("shell.json", try String(contentsOf: corpus.appendingPathComponent("shell.json"), encoding: .utf8)),
         (file, try String(contentsOf: corpus.appendingPathComponent(file), encoding: .utf8))],
        manifest: "file\tstatus\turl\nshell.json\t200\t/_navree/native/shell\n\(file)\t200\t/\n"
    )
    let answers = try #require(AnswerFolder(folder: folder))
    let configuration = ShellConfiguration(origin: "https://answers.example", appVersion: "1.0", keepsSession: false)
    let core = try #require(Core(config: CoreConfiguration.json(configuration, device: DeviceTraits())))
    let client = NativeClient(core: core, effects: SystemEffects(
        transport: answers, cache: KeptFiles(folder: folder.appendingPathComponent("cache")),
        snapshots: KeptFiles(folder: folder.appendingPathComponent("snapshot"))
    ))
    var problems: [String] = []
    client.problem = { problems.append($0) }
    client.begin()
    try await AnswerFolderTests.until { client.screen(1).map { !$0.loading } ?? false }
    #expect(problems.isEmpty, "\(file): \(problems)")
    switch client.screen(1)?.content {
    case .native?, .web?: break
    default: Issue.record("\(file) is \(String(describing: client.screen(1)?.content)), failure \(String(describing: client.screen(1)?.failure))")
    }
    _ = shell
}
