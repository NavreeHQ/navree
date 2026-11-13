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

    static var all: [URL] {
        json(in: corpus) + json(in: recorded)
    }

    static func isShell(_ file: URL) -> Bool {
        file.lastPathComponent.hasPrefix("shell")
    }

    static func isFromTheFuture(_ file: URL) -> Bool {
        file.lastPathComponent.contains("future")
    }

    static func envelope(_ file: URL) throws -> Navree.Envelope {
        try WireDecoder.decode(Navree.Envelope.self, from: Data(contentsOf: file))
    }
}

/// Every variant read as `unknown` anywhere in `value`, as `Union:tag`: a
/// node, an action, a command, an op, an input, a device call. Found by
/// reflection, so no place a union can stand is missed.
func unknowns(in value: Any) -> [String] {
    var found: [String] = []
    var pending: [Any] = [value]
    while let next = pending.popLast() {
        let mirror = Mirror(reflecting: next)
        if mirror.displayStyle == .enum, let child = mirror.children.first, child.label == "unknown" {
            let tag = Mirror(reflecting: child.value).children.first { $0.label == "tag" }?.value as? String ?? "?"
            found.append("\(type(of: next)):\(tag)")
        }
        pending.append(contentsOf: mirror.children.map(\.value))
    }
    return found
}

@Test func theCorpusIsWhereTheTestsLookForIt() {
    #expect(Examples.json(in: Examples.corpus).count >= 20)
    #expect(Examples.json(in: Examples.recorded).count >= 10)
}

@Test(arguments: Examples.all)
func everyExampleDecodes(_ file: URL) throws {
    let data = try Data(contentsOf: file)
    if Examples.isShell(file) {
        let shell = try WireDecoder.decode(Navree.Shell.self, from: data)
        #expect(!shell.name.isEmpty || shell.destinations.isEmpty)
    } else {
        let envelope = try WireDecoder.decode(Navree.Envelope.self, from: data)
        #expect(envelope.navree == Wire.major)
        #expect(envelope.screen != nil || !envelope.ops.isEmpty || envelope.web != nil || envelope.upgrade != nil)
    }
}

/// What the models write, they read back as the same value. Writing a tree
/// and comparing two of them take as much stack as reading one, so both are
/// done on a thread with room for it.
@Test(arguments: Examples.all)
func everyExampleIsWrittenAndReadBackTheSame(_ file: URL) throws {
    let data = try Data(contentsOf: file)
    let (same, _) = peakStack(size: 16 << 20) { () -> Bool in
        if Examples.isShell(file) {
            guard let shell = try? JSONDecoder().decode(Navree.Shell.self, from: data),
                let written = try? JSONEncoder().encode(shell)
            else { return false }
            return (try? JSONDecoder().decode(Navree.Shell.self, from: written)) == shell
        }
        guard let envelope = try? JSONDecoder().decode(Navree.Envelope.self, from: data),
            let written = try? JSONEncoder().encode(envelope)
        else { return false }
        return (try? JSONDecoder().decode(Navree.Envelope.self, from: written)) == envelope
    }
    #expect(same)
}

@Test func noExampleHoldsAKindThisClientDoesNotKnow() throws {
    for file in Examples.all where !Examples.isFromTheFuture(file) {
        let data = try Data(contentsOf: file)
        let value: Any = Examples.isShell(file)
            ? try WireDecoder.decode(Navree.Shell.self, from: data)
            : try WireDecoder.decode(Navree.Envelope.self, from: data)
        var kinds: [String] = []
        if let screen = (value as? Navree.Envelope)?.screen {
            screen.body.walk { if case .unknown(let part) = $0 { kinds.append(part.tag) } }
            screen.actions.forEach { $0.walk { if case .unknown(let part) = $0 { kinds.append(part.tag) } } }
        }
        #expect(kinds.isEmpty, "\(file.lastPathComponent): \(kinds)")
        // Nor an action, a command, an op, an input or a device call.
        let anything = unknowns(in: value)
        #expect(anything.isEmpty, "\(file.lastPathComponent): \(anything)")
    }
}

@Test func theAnswerFromTheFutureIsReadWithWhatThisBuildDoesNotKnowSetAside() throws {
    let envelope = try Examples.envelope(Examples.corpus.appendingPathComponent("from-the-future.json"))
    #expect(envelope.app == "app-2031-04-1a")
    let screen = try #require(envelope.screen)
    #expect(screen.title == "Warehouse")
    #expect(screen.subtitle == "Ludhiana")
    guard case .button(let teleport)? = screen.actions.first else {
        Issue.record("the screen's action is not a button")
        return
    }
    #expect(teleport.label == "Teleport")
    #expect(teleport.action == .unknown(Navree.Action.Unknown(tag: "teleport")))
    guard case .stack(let stack) = screen.body, stack.children.count == 6 else {
        Issue.record("the body is not the stack of six")
        return
    }
    #expect(stack.gap == 2)
    guard case .text(let heading) = stack.children[0] else {
        Issue.record("a known kind with a field from the future did not decode")
        return
    }
    #expect(heading.value == "Stock across Punjab")
    guard case .unknown(let hologram) = stack.children[1], case .unknown(let bare) = stack.children[2] else {
        Issue.record("the holograms are not unknown kinds")
        return
    }
    #expect(hologram.tag == "hologram")
    #expect(hologram.id == "shelves-3d")
    guard case .image(let stand)? = hologram.alt else {
        Issue.record("the hologram's stand-in is not its picture")
        return
    }
    #expect(stand.alt == "The shelves at the Ludhiana warehouse")
    #expect(bare.tag == "hologram" && bare.id == "no-stand-in" && bare.alt == nil)
    guard case .form(let form) = stack.children[3], form.children.count == 2,
        case .field(let signature) = form.children[0], case .field(let units) = form.children[1]
    else {
        Issue.record("the form did not decode")
        return
    }
    #expect(signature.input == .unknown(Navree.Input.Unknown(tag: "signature")))
    #expect(signature.input.initialValues.isEmpty)
    #expect(units.input.initialValues == ["24"])
    guard case .button(let celebrate) = stack.children[4], case .commands(let commands)? = celebrate.action else {
        Issue.record("the celebration is not a button with commands")
        return
    }
    #expect(commands.list == [.unknown(Navree.Command.Unknown(tag: "confetti")), .toast(Navree.Command.Toast(text: "Moved"))])
    guard case .button(let measure) = stack.children[5], case .device(let call)? = measure.action else {
        Issue.record("the measurement is not a device call")
        return
    }
    #expect(call.call == .unknown(Navree.Call.Unknown(tag: "lidar_scan")))
    #expect(call.alt == .unknown(Navree.Alt.Unknown(tag: "glow")))
    #expect(envelope.ops.count == 3)
    #expect(envelope.ops[0] == .unknown(Navree.Op.Unknown(tag: "confetti")))
    #expect(envelope.ops[1] == .device(Navree.Op.Device(call: .unknown(Navree.ReplyCall.Unknown(tag: "locate_once")))))
    guard case .toast(let toast) = envelope.ops[2] else {
        Issue.record("the toast did not decode")
        return
    }
    #expect(toast.text == "Synced" && toast.tone == .success)
    #expect(
        Set(unknowns(in: envelope)) == [
            "Action:teleport", "Node:hologram", "Input:signature", "Command:confetti", "Call:lidar_scan", "Alt:glow",
            "Op:confetti", "ReplyCall:locate_once",
        ]
    )
}

@Test func theOrdersAnswerIsATableOfRecordsWithAQuery() throws {
    let screen = try #require(try Examples.envelope(Examples.recorded.appendingPathComponent("orders.json")).screen)
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
          {"t":"button","label":"Go","role":"celebrate","action":{"t":"teleport"}},
          {"t":"text","value":"Hello","style":"whisper","colour":{"hue":"blue","to":"saffron"}}
        ]}
        """
    let node = try WireDecoder.decode(Navree.Node.self, from: Data(json.utf8))
    guard case .stack(let stack) = node else {
        Issue.record("not a stack")
        return
    }
    guard case .unknown(let marquee) = stack.children[0] else {
        Issue.record("the marquee is not an unknown kind")
        return
    }
    #expect(marquee.tag == "marquee")
    #expect(marquee.id == "m")
    #expect(marquee.alt?.kind == "text")
    guard case .unknown(let hologram) = stack.children[1] else {
        Issue.record("the hologram is not an unknown kind")
        return
    }
    #expect(hologram.alt == nil)
    guard case .badge(let badge) = stack.children[2], case .button(let button) = stack.children[3],
        case .text(let text) = stack.children[4]
    else {
        Issue.record("a known kind with unknown values did not decode")
        return
    }
    // A word this build does not know is read as the field's default, or as
    // none where the field may be left out.
    #expect(badge.tone == nil)
    #expect(button.role == Navree.Role.none)
    #expect(button.action == .unknown(Navree.Action.Unknown(tag: "teleport")))
    #expect(text.style == .body)
    #expect(text.colour == Navree.Colour(hue: .blue))
}

@Test func aListOfWordsLeavesOutTheOnesThisBuildDoesNotKnow() throws {
    let input = try WireDecoder.decode(
        Navree.Input.self, from: Data(#"{"t":"date","off_weekdays":["sunday","someday","monday"]}"#.utf8)
    )
    guard case .date(let date) = input else {
        Issue.record("not a date")
        return
    }
    #expect(date.offWeekdays == [.sunday, .monday])
    let scan = try WireDecoder.decode(Navree.ScanCode.self, from: Data(#"{"formats":["qr","hologram"]}"#.utf8))
    #expect(scan.formats == [.qr])
    // Only the word is forgiven: a value of another shape is still an error.
    #expect(throws: DecodingError.self) {
        try WireDecoder.decode(Navree.ScanCode.self, from: Data(#"{"formats":[7]}"#.utf8))
    }
}

@Test func opsAndCommandsFromTheFutureArePassedOver() throws {
    let json = """
        {"navree":1,"build":"b","vocab":1,"ops":[
          {"op":"confetti","target":"x"},
          {"op":"errors","form":"refund","fields":{"amount":["Enter an amount above zero."]}},
          {"op":"open","target":"refund"}
        ]}
        """
    let envelope = try WireDecoder.decode(Navree.Envelope.self, from: Data(json.utf8))
    #expect(envelope.ops.count == 3)
    #expect(envelope.ops[0] == .unknown(Navree.Op.Unknown(tag: "confetti")))
    #expect(envelope.ops[1] == .errors(Navree.Op.Errors(fields: ["amount": ["Enter an amount above zero."]], form: "refund")))
    let action = try WireDecoder.decode(
        Navree.Action.self, from: Data(#"{"t":"commands","list":[{"c":"juggle"},{"c":"open","id":"refund"}]}"#.utf8)
    )
    #expect(
        action == .commands(Navree.Action.Commands(list: [.unknown(Navree.Command.Unknown(tag: "juggle")), .open(Navree.Command.Open(id: "refund"))]))
    )
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
