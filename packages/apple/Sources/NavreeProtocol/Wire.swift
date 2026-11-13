import Foundation

/// The names and numbers a client and a Navree server agree on.
public enum Wire {
    /// The protocol's major version: the value of ``nativeHeader`` and of an
    /// envelope's `navree`. Each side refuses a major it does not know.
    public static let major = Navree.major

    /// The vocabulary this client draws: the number it sends as `vocab`. It
    /// stays 0 until the vocabulary is frozen.
    public static let vocab = Navree.vocab

    /// The deepest objects and arrays may nest in an answer's JSON: a client
    /// refuses a deeper answer before reading it, and a server answers a deeper
    /// page with the web view.
    public static let maxNesting = 127

    /// The request header that says "answer me as a native client".
    public static let nativeHeader = "X-Navree-Native"

    /// The request header that carries the client's descriptor.
    public static let clientHeader = "X-Navree-Client"

    /// The media type of an envelope and of the shell document.
    public static let contentType = "application/vnd.navree.native+json"

    /// Where the app's shell document is.
    public static let shellPath = "/_navree/native/shell"

    /// The product a client's web view adds to its user agent, so the server
    /// writes a page without the chrome the client draws itself. A web view
    /// sends custom headers on its first request only, so a header can't say it.
    public static let userAgentProduct = "NavreeNative/\(major)"

    /// The whole token: `NavreeNative/1 (platform=ios; lib=0.1.0)`.
    public static func userAgentToken(platform: String, lib: String) -> String {
        "\(userAgentProduct) (platform=\(ClientDescriptor.token(platform)); lib=\(ClientDescriptor.token(lib)))"
    }

    /// How deep the objects and arrays of `json` nest, read without recursion:
    /// the same count the protocol crate makes.
    public static func nesting(_ json: Data) -> Int {
        var depth = 0
        var deepest = 0
        var inText = false
        var escaped = false
        for byte in json {
            if inText {
                if escaped {
                    escaped = false
                } else if byte == UInt8(ascii: "\\") {
                    escaped = true
                } else if byte == UInt8(ascii: "\"") {
                    inText = false
                }
                continue
            }
            switch byte {
            case UInt8(ascii: "\""):
                inText = true
            case UInt8(ascii: "{"), UInt8(ascii: "["):
                depth += 1
                deepest = max(deepest, depth)
            case UInt8(ascii: "}"), UInt8(ascii: "]"):
                depth = max(0, depth - 1)
            default:
                break
            }
        }
        return deepest
    }
}

/// Why protocol JSON was not read.
public enum WireError: Error, Equatable, Sendable {
    /// Its objects and arrays nest deeper than ``Wire/maxNesting``: it is
    /// refused before it is read, so it costs no stack.
    case tooDeep(nesting: Int)
}

/// Reads protocol JSON with the generated models.
///
/// The models' decoders are derived, and each level of a tree costs several
/// frames: the deepest tree the protocol allows takes more stack in a debug
/// build than a secondary thread on Apple's systems has (512 KiB), and more
/// than an iPhone's main thread has (1 MiB). So every reading runs on a thread
/// of its own with ``stackSize`` of stack, and an answer that nests deeper
/// than the protocol allows is refused before it is read.
public enum WireDecoder {
    /// The stack a reading is given: room for the deepest answer the nesting
    /// limit lets through, in a debug build, with a margin.
    public static let stackSize = 4 << 20

    /// Decodes an envelope, a shell document or any other protocol type.
    /// `limit` is how deep the JSON may nest: the protocol's limit for an
    /// answer, a few levels more for a part of the runtime core's view state,
    /// which holds a tree the core has read and checked a few levels down.
    public static func decode<T: Decodable & Sendable>(
        _ type: T.Type, from data: Data, limit: Int = Wire.maxNesting
    ) throws -> T {
        let nesting = Wire.nesting(data)
        guard nesting <= limit else { throw WireError.tooDeep(nesting: nesting) }
        return try onReadingThread { try JSONDecoder().decode(type, from: data) }
    }

    /// Runs `work` on a thread of ``stackSize`` and waits for it.
    static func onReadingThread<T: Sendable>(_ work: @escaping @Sendable () throws -> T) throws -> T {
        let outcome = Outcome<T>()
        let done = DispatchSemaphore(value: 0)
        let thread = Thread {
            outcome.result = Result(catching: work)
            done.signal()
        }
        thread.name = "navree.wire.reading"
        thread.stackSize = stackSize
        thread.start()
        done.wait()
        return try outcome.result!.get()
    }

    /// What the reading thread hands back. It is written on that thread before
    /// the semaphore is signalled and read on the caller's after it is waited
    /// on, never both at once.
    private final class Outcome<T>: @unchecked Sendable {
        var result: Result<T, any Error>?
    }
}
