import Foundation
import NavreeProtocol
import Testing

// A tree as deep as the protocol allows is read whole, and a deeper one is
// refused with an error and never by running out of stack. How much stack a
// reading takes is measured here, by filling a thread's stack with a pattern
// and finding the deepest byte the reading changed.

/// Trees as deep as the protocol allows, in each shape of nesting it has.
enum DeepTrees {
    /// The most nodes inside one another an answer may hold.
    static let maxDepth = 64

    /// One level of a shape around `inner`.
    static func wrap(_ shape: Int, _ inner: String) -> String {
        switch shape % shapes.count {
        case 0: #"{"t":"stack","gap":2,"children":["# + inner + "]}"
        case 1: #"{"t":"section","title":"Orders","style":"card","children":["# + inner + "]}"
        case 2: #"{"t":"list","children":[{"t":"row","title":"Tejinder Randhawa","subtitle":"Ludhiana","detail":["# + inner + "]}]}"
        case 3: #"{"t":"table","columns":[{"label":"Order"}],"rows":[{"id":"r","cells":["# + inner + "]}]}"
        case 4: #"{"t":"form","method":"post","action":"/x","children":[{"t":"field","name":"q","input":{"t":"text"}},"# + inner + "]}"
        case 5: #"{"t":"sheet","id":"s","open":true,"children":["# + inner + "]}"
        case 6: #"{"t":"link","label":"Open","action":{"t":"navigate","href":"/x"},"children":["# + inner + "]}"
        case 7: #"{"t":"disclosure","label":"More","open":true,"children":["# + inner + "]}"
        case 8: #"{"t":"row","title":"Order","leading":"# + inner + "}"
        case 9: #"{"t":"card","children":["# + inner + "]}"
        default: #"{"t":"figure","label":"Sleep","value":"7h 12m","aside":"# + inner + "}"
        }
    }

    static let shapes = [
        "stacks", "sections", "rows of lists", "tables", "forms", "sheets", "links", "disclosures", "leading rows",
        "cards", "figures",
    ]

    /// How many nodes deep a tree written by `wrap` goes: every node says its
    /// kind once, and a control and an action are not nodes.
    static func nodes(_ tree: String) -> Int {
        func count(_ part: String) -> Int { tree.components(separatedBy: part).count - 1 }
        return count(#"{"t":"#) - count(#"{"t":"field""#) - count(#"{"t":"text"}"#) - count(#"{"t":"navigate""#)
    }

    static func page(_ body: String) -> Data {
        Data((#"{"navree":1,"build":"b1","vocab":0,"screen":{"title":"Deep","body":"# + body + "}}").utf8)
    }

    /// A tree of one shape (or of every shape in turn when `nil`), as deep as
    /// both limits allow: 64 nodes, and 127 levels of JSON.
    static func deepest(_ shape: Int?) -> String {
        var node = #"{"t":"text","id":"leaf","value":"the bottom"}"#
        for at in 0... {
            let next = wrap(shape ?? at, node)
            if Wire.nesting(page(next)) > Wire.maxNesting || nodes(next) > maxDepth { return node }
            node = next
        }
        return node
    }

    /// A chain of nodes held one inside the next with nothing between, as
    /// deep as the JSON limit alone allows: the most stack an answer the
    /// limit lets through can take.
    static func deepestByNesting() -> String {
        var node = #"{"t":"text","id":"leaf","value":"the bottom"}"#
        while true {
            let next = wrap(10, node)
            if Wire.nesting(page(next)) > Wire.maxNesting { return node }
            node = next
        }
    }

    /// Whether the decoded page reaches the leaf.
    static func reachesTheLeaf(_ envelope: Navree.Envelope) -> Bool {
        envelope.screen?.body.find("leaf", kinds: ["text"]) != nil
    }
}

/// Runs `work` on a thread with `size` bytes of stack and returns the most
/// stack it used, in bytes, with what it returned.
func peakStack<T>(size: Int, _ work: @escaping @Sendable () -> T) -> (T, Int) {
    let out = Measured<T>()
    let done = DispatchSemaphore(value: 0)
    let thread = Thread {
        let me = pthread_self()
        let top = UInt(bitPattern: pthread_get_stackaddr_np(me))
        let bottom = top - UInt(pthread_get_stacksize_np(me))
        var marker: UInt8 = 0
        let here = withUnsafeMutablePointer(to: &marker) { UInt(bitPattern: $0) }
        // Fill everything below this frame but a margin with the pattern.
        let low = bottom + 4096
        let high = here - 16 * 1024
        memset(UnsafeMutableRawPointer(bitPattern: low)!, 0xA5, Int(high - low))
        let result = work()
        var touched = low
        while touched < high, UnsafePointer<UInt8>(bitPattern: touched)!.pointee == 0xA5 { touched += 1 }
        out.value = (result, Int(top - touched))
        done.signal()
    }
    thread.stackSize = size
    thread.start()
    done.wait()
    return out.value!
}

/// What a measured thread hands back: written there before the semaphore is
/// signalled, read here after it is waited on.
final class Measured<T>: @unchecked Sendable {
    var value: (T, Int)?
}

/// The stack a plain reading of `data` takes, on a thread with plenty.
func readingStack(_ data: Data) -> (Bool, Int) {
    peakStack(size: 32 << 20) { (try? JSONDecoder().decode(Navree.Envelope.self, from: data)) != nil }
}

func kib(_ bytes: Int) -> String {
    "\(bytes / 1024) KiB"
}

@Test func theDeepestTreeOfEveryShapeIsReadWholeOnTheReadingThread() throws {
    for shape in Array(0..<DeepTrees.shapes.count).map(Optional.some) + [nil] {
        let tree = DeepTrees.deepest(shape)
        let name = shape.map { DeepTrees.shapes[$0] } ?? "every shape in turn"
        #expect(DeepTrees.nodes(tree) >= DeepTrees.maxDepth - 2 || Wire.nesting(DeepTrees.page(tree)) >= Wire.maxNesting - 1, "\(name)")
        let envelope = try WireDecoder.decode(Navree.Envelope.self, from: DeepTrees.page(tree))
        #expect(DeepTrees.reachesTheLeaf(envelope), "\(name)")
    }
}

@Test func anAnswerNestedDeeperThanTheProtocolAllowsIsRefusedBeforeItIsRead() throws {
    let deepest = DeepTrees.deepestByNesting()
    let page = DeepTrees.page(deepest)
    #expect(Wire.nesting(page) == Wire.maxNesting)
    #expect(DeepTrees.reachesTheLeaf(try WireDecoder.decode(Navree.Envelope.self, from: page)))
    let deeper = DeepTrees.page(DeepTrees.wrap(10, deepest))
    #expect(throws: WireError.tooDeep(nesting: Wire.maxNesting + 1)) {
        try WireDecoder.decode(Navree.Envelope.self, from: deeper)
    }
    var far = deepest
    for _ in 0..<2000 { far = DeepTrees.wrap(0, far) }
    #expect(throws: WireError.tooDeep(nesting: Wire.maxNesting + 4000)) {
        try WireDecoder.decode(Navree.Envelope.self, from: DeepTrees.page(far))
    }
    #expect(Wire.nesting(Data(#"{"t":"stack","children":[{"t":"text","value":"[{"}]}"#.utf8)) == 3)
    #expect(Wire.nesting(Data("7".utf8)) == 0)
}

/// The numbers behind the reading thread's size. A secondary thread on Apple's
/// systems has 512 KiB of stack and an iPhone's main thread 1 MiB; in a debug
/// build the deepest trees take more than either.
@Test func theReadingThreadHasRoomForTheDeepestAnswerTheLimitsLetThrough() throws {
    var deepestAllowed = 0
    for shape in Array(0..<DeepTrees.shapes.count).map(Optional.some) + [nil] {
        let tree = DeepTrees.deepest(shape)
        let page = DeepTrees.page(tree)
        let (read, used) = readingStack(page)
        #expect(read)
        deepestAllowed = max(deepestAllowed, used)
        let name = shape.map { DeepTrees.shapes[$0] } ?? "every shape in turn"
        print("reading stack: \(name), \(DeepTrees.nodes(tree)) nodes, nesting \(Wire.nesting(page)): \(kib(used))")
    }
    var corpus = 0
    for file in Examples.json(in: Examples.corpus) where !Examples.isShell(file) {
        let (read, used) = readingStack(try Data(contentsOf: file))
        #expect(read, "\(file.lastPathComponent)")
        corpus = max(corpus, used)
    }
    // Writing the deepest tree, and comparing two of them, which SwiftUI and
    // Observation may do with what a view holds.
    let deepest = DeepTrees.page(DeepTrees.deepest(10))
    let (_, writing) = peakStack(size: 32 << 20) { () -> Bool in
        guard let tree = try? JSONDecoder().decode(Navree.Envelope.self, from: deepest) else { return false }
        return (try? JSONEncoder().encode(tree)) != nil
    }
    let (_, comparing) = peakStack(size: 32 << 20) { () -> Bool in
        let one = try? JSONDecoder().decode(Navree.Envelope.self, from: deepest)
        let two = try? JSONDecoder().decode(Navree.Envelope.self, from: deepest)
        return one == two
    }
    print("reading stack: writing the deepest tree \(kib(writing)); comparing two of them \(kib(comparing))")
    let chain = DeepTrees.page(DeepTrees.deepestByNesting())
    let (read, byNesting) = readingStack(chain)
    #expect(read)
    print("reading stack: deepest tree the protocol allows \(kib(deepestAllowed)); deepest corpus answer \(kib(corpus)); deepest answer the nesting limit lets through \(kib(byNesting)); a 512 KiB thread fits the deepest allowed tree: \(deepestAllowed < 512 * 1024)")
    #expect(corpus < 512 * 1024)
    #expect(byNesting >= deepestAllowed)
    // The reading thread holds the worst with half again to spare.
    #expect(byNesting + byNesting / 2 < WireDecoder.stackSize)
}
