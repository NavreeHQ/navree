import Foundation
import NavreeProtocol
import Testing

/// Example envelopes kept in the repository: the corpus written from the
/// protocol crate's types, and answers recorded from the store demo.
enum Examples {
    /// The repository's root, found from this file's place in it.
    static let root: URL = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()  // NavreeProtocolTests
        .deletingLastPathComponent()  // Tests
        .deletingLastPathComponent()  // apple
        .deletingLastPathComponent()  // packages
        .deletingLastPathComponent()

    static let corpus = root.appendingPathComponent("crates/navree-native/corpus")
    static let recorded = root.appendingPathComponent("packages/apple/Demo/NavreeDemo/Recorded")

    static func json(in folder: URL) -> [URL] {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
        return names.filter { $0.hasSuffix(".json") }.sorted().map { folder.appendingPathComponent($0) }
    }

    static func isShell(_ file: URL) -> Bool {
        file.lastPathComponent.hasPrefix("shell")
    }
}

@Test func theCorpusIsWhereTheTestsLookForIt() {
    #expect(Examples.json(in: Examples.corpus).count >= 10)
    #expect(Examples.json(in: Examples.recorded).count >= 10)
}

@Test(arguments: Examples.json(in: Examples.corpus) + Examples.json(in: Examples.recorded))
func everyExampleDecodes(_ file: URL) throws {
    let data = try Data(contentsOf: file)
    if Examples.isShell(file) {
        let shell = try WireDecoder.decode(ShellDocument.self, from: data)
        #expect(!shell.name.isEmpty || shell.destinations.isEmpty)
    } else {
        let envelope = try WireDecoder.decode(Envelope.self, from: data)
        #expect(envelope.navree == Wire.major)
        #expect(envelope.screen != nil || !envelope.ops.isEmpty || envelope.web != nil || envelope.upgrade != nil)
    }
}

@Test func noExampleHoldsAKindThisClientDoesNotKnow() throws {
    for file in Examples.json(in: Examples.corpus) + Examples.json(in: Examples.recorded)
    where !Examples.isShell(file) && !file.lastPathComponent.contains("future") {
        let envelope = try WireDecoder.decode(Envelope.self, from: Data(contentsOf: file))
        var unknown: [String] = []
        if let screen = envelope.screen {
            screen.body.walk { if case .unknown(let part) = $0 { unknown.append(part.kind) } }
            screen.actions.forEach { $0.walk { if case .unknown(let part) = $0 { unknown.append(part.kind) } } }
        }
        #expect(unknown.isEmpty, "\(file.lastPathComponent): \(unknown)")
    }
}

@Test func theOrdersAnswerIsATableOfRecordsWithAQuery() throws {
    let data = try Data(contentsOf: Examples.recorded.appendingPathComponent("orders.json"))
    let screen = try #require(try WireDecoder.decode(Envelope.self, from: data).screen)
    #expect(screen.title == "Orders")
    #expect(screen.nav == "/orders")
    let table = try #require(screen.body.first { $0.kind == "table" })
    guard case .table(let orders) = table else { return }
    #expect(orders.rows.count == 25)
    #expect(orders.columns.first?.role == .title)
    #expect(orders.more != nil)
    var searchName: String?
    for case .field(let field) in orders.query?.children ?? [] {
        guard case .text(let text) = field.input, text.kind == .search else { continue }
        searchName = field.name
    }
    #expect(searchName == "search")
}

@Test func aKindFromTheFutureIsDrawnFromItsAltOrNotAtAll() throws {
    let json = """
        {"t":"stack","children":[
          {"t":"marquee","id":"m","speed":3,"alt":{"t":"text","value":"Sale today"}},
          {"t":"hologram"},
          {"t":"badge","text":"Paid","tone":"glowing","sparkle":true},
          {"t":"button","label":"Go","role":"celebrate","action":{"t":"teleport"}}
        ]}
        """
    let node = try WireDecoder.decode(Node.self, from: Data(json.utf8))
    guard case .stack(let stack) = node else {
        Issue.record("not a stack")
        return
    }
    guard case .unknown(let marquee) = stack.children[0] else {
        Issue.record("the marquee is not an unknown kind")
        return
    }
    #expect(marquee.kind == "marquee")
    #expect(marquee.id == "m")
    #expect(marquee.alt?.kind == "text")
    guard case .unknown(let hologram) = stack.children[1] else {
        Issue.record("the hologram is not an unknown kind")
        return
    }
    #expect(hologram.alt == nil)
    guard case .badge(let badge) = stack.children[2], case .button(let button) = stack.children[3] else {
        Issue.record("a known kind with unknown values did not decode")
        return
    }
    #expect(badge.tone == .neutral)
    #expect(button.role == .plain)
    #expect(button.action == .unrecognized("teleport"))
}

@Test func opsAndCommandsFromTheFutureArePassedOver() throws {
    let json = """
        {"navree":1,"build":"b","vocab":1,"ops":[
          {"op":"confetti","target":"x"},
          {"op":"errors","form":"refund","fields":{"amount":["Enter an amount above zero."]}},
          {"op":"open","target":"refund"}
        ]}
        """
    let envelope = try WireDecoder.decode(Envelope.self, from: Data(json.utf8))
    #expect(envelope.ops.count == 3)
    #expect(envelope.ops[0] == .unrecognized("confetti"))
    #expect(envelope.ops[1] == .errors(form: "refund", fields: ["amount": ["Enter an amount above zero."]]))
    let action = try WireDecoder.decode(Action.self, from: Data(#"{"t":"commands","list":[{"c":"juggle"},{"c":"open","id":"refund"}]}"#.utf8))
    #expect(action == .commands([.unrecognized("juggle"), .open(id: "refund")]))
}

@Test func theDescriptorIsTokensSeparatedBySemicolons() {
    let descriptor = ClientDescriptor(
        platform: "ios", os: "26.5", form: "phone", app: "0.1.0", lib: "0.1.0",
        scheme: "dark", contrast: "normal", text: 1.0, locale: "en-IN", timeZone: "Asia/Kolkata",
        caps: ["share", "haptic"]
    )
    #expect(
        descriptor.header
            == "platform=ios; os=26.5; form=phone; app=0.1.0; lib=0.1.0; vocab=0; scheme=dark; contrast=normal; text=1.0; locale=en-IN; tz=Asia/Kolkata; caps=share+haptic"
    )
    let odd = ClientDescriptor(platform: "ios", os: "26 beta; x=1", form: "phone", app: "1", lib: "1")
    #expect(odd.header.hasPrefix("platform=ios; os=26betax1; form=phone"))
}
