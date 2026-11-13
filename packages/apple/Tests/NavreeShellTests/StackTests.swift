import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

@testable import NavreeShell

// The client's state is observed: each time it is set, Observation compares
// the new state with the old one, on the main thread, and a part's derived
// comparison takes tens of KiB of stack for each level of a tree in a debug
// build. An iPhone's main thread has 1 MiB.

/// What a measured thread hands back: written there before the semaphore is
/// signalled, read here after it is waited on.
final class Taken: @unchecked Sendable {
    var bytes = 0
}

/// The most stack `work` takes, found by filling a thread's stack with a
/// pattern and looking for the deepest byte it changed.
func stackTaken(_ work: @escaping @Sendable () -> Void) -> Int {
    let taken = Taken()
    let done = DispatchSemaphore(value: 0)
    let thread = Thread {
        let me = pthread_self()
        let top = UInt(bitPattern: pthread_get_stackaddr_np(me))
        let bottom = top - UInt(pthread_get_stacksize_np(me))
        var marker: UInt8 = 0
        let here = withUnsafeMutablePointer(to: &marker) { UInt(bitPattern: $0) }
        let low = bottom + 4096
        let high = here - 16 * 1024
        memset(UnsafeMutableRawPointer(bitPattern: low)!, 0xA5, Int(high - low))
        work()
        var touched = low
        while touched < high, UnsafePointer<UInt8>(bitPattern: touched)!.pointee == 0xA5 { touched += 1 }
        taken.bytes = Int(top - touched)
        done.signal()
    }
    thread.stackSize = 16 << 20
    thread.start()
    done.wait()
    return taken.bytes
}

@Test func comparingTheStateAfterAnOpChangedTheOrderStaysInsideAMainThreadsStack() throws {
    var (runtime, order) = try orderOpen()
    _ = runtime.send(.intent(.act(.commands(Navree.Action.Commands(list: [.open(Navree.Command.Open(id: "refund"))])), Place()), at: order))
    let form = try #require(runtime.formRef("refund", at: order))
    _ = runtime.send(.intent(.change(form: form, name: "amount", values: ["0"]), at: order))
    let post = runtime.send(.intent(.act(.submit(Navree.Action.Submit()), Place(form: form, overlay: "refund")), at: order))
    let before = runtime.state
    _ = runtime.answer(post, with: [try Store.reply("post-refund-refused", status: 422)])
    let after = runtime.state
    #expect(field("amount", in: runtime, at: order)?.error != nil)
    let used = stackTaken { _ = before == after }
    print("comparing the state after the refused refund: \(used / 1024) KiB")
    // SwiftUI's own frames stand under the comparison: a quarter of the
    // thread is left for them.
    #expect(used < 768 * 1024)
}
