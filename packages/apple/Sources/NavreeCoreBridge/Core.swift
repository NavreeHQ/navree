import Foundation
import NavreeCore

/// The runtime core: everything the client does that is not drawing,
/// placement or talking to the system. JSON in, JSON out, over the C
/// interface in `navree_core.h`.
///
/// Make one from the configuration, ``send(_:)`` it one event a call in the
/// order things happened, and read each part the update names with
/// ``view(_:)``. Every answer may be `{"error":"…"}`: an event or a query that
/// does not read, or a failure inside the core, after which it answers only
/// errors. That is a bug to log, never something to draw from.
///
/// One caller at a time: a core is not `Sendable`, so Swift keeps each one in
/// the single isolation domain that made it (the app's main actor), and two
/// calls can never run at once or out of order. The calling thread needs
/// 512 KiB of stack, which every thread of an Apple system has; the core
/// reads each answer on a thread of its own.
public final class Core {
    private let handle: OpaquePointer

    /// A core for `config`, or `nil` when the configuration does not read.
    /// The bytes are copied.
    public init?(config: Data) {
        guard let made = config.withBytes({ navree_core_new($0, $1) }) else { return nil }
        handle = made
    }

    deinit {
        navree_core_free(handle)
    }

    /// Tells the core what happened, such as `{"t":"start"}`, and answers the
    /// update: `{"changed":[…],"effects":[…]}`.
    public func send(_ event: Data) -> Data {
        Self.take(event.withBytes { navree_core_send(handle, $0, $1) })
    }

    /// Reads a part of the view state, such as `{"part":"nav"}`; an empty
    /// query reads all of it.
    public func view(_ query: Data = Data()) -> Data {
        Self.take(query.withBytes { navree_core_view(handle, $0, $1) })
    }

    /// Copies bytes the core owns and gives them back to it.
    private static func take(_ bytes: NavreeBytes) -> Data {
        defer { navree_bytes_free(bytes) }
        guard let start = bytes.ptr, bytes.len > 0 else { return Data() }
        return Data(bytes: start, count: bytes.len)
    }
}

/// A core belongs to the isolation domain that made it.
@available(*, unavailable)
extension Core: Sendable {}

extension Data {
    /// Calls `body` with the bytes' address (`nil` when empty) and count.
    fileprivate func withBytes<Result>(_ body: (UnsafePointer<UInt8>?, Int) -> Result) -> Result {
        withUnsafeBytes { raw in
            raw.withMemoryRebound(to: UInt8.self) { body($0.baseAddress, $0.count) }
        }
    }
}
