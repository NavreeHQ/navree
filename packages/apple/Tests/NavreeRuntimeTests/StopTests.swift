import Foundation
import NavreeProtocol
import Testing

@testable import NavreeRuntime

/// A core that fails inside stops for good, and says so in a mark a client
/// reads without reading the words.
@MainActor
struct StopTests {
    /// What the client makes of an answer that is not an update.
    static func failure(_ answer: String) -> CoreError? {
        if case .failure(let error) = Update.read(Data(answer.utf8)) { return error }
        return nil
    }

    @Test func aStopIsReadByItsMarkAndARefusalIsNotOne() {
        let words = CoreAnswer.stoppedWords
        #expect(Self.failure(#"{"error":"\#(words)","stopped":true}"#) == .stopped(words))
        // The words may change: the mark is what is read.
        #expect(Self.failure(#"{"error":"said another way","stopped":true}"#) == .stopped("said another way"))
        // What merely does not read, a freed core and no core are errors, and the core goes on.
        for refusal in ["the event does not read: missing field `t`", "the query does not read: no part", "the core was freed", "no core"] {
            #expect(Self.failure(#"{"error":"\#(refusal)"}"#) == .refused(refusal))
        }
        #expect(Self.failure("]") == .unreadable)
    }

    @Test func withoutAMarkTheCoresOwnWordsForAStopAreRead() {
        let words = CoreAnswer.stoppedWords
        #expect(Self.failure(#"{"error":"\#(words)"}"#) == .stopped(words))
        // A mark that says no is believed over the words.
        #expect(Self.failure(#"{"error":"\#(words)","stopped":false}"#) == .refused(words))
        #expect(CoreAnswer.isStop(Data(#"{"error":"\#(words)","stopped":true}"#.utf8)))
        #expect(!CoreAnswer.isStop(Data(#"{"changed":[],"effects":[]}"#.utf8)))
        #expect(!CoreAnswer.isStop(Data("null".utf8)))
    }

    @Test func theWordsOfAStopAreTheOnesTheCoreWrites() throws {
        let source = Transcripts.folder.deletingLastPathComponent().appendingPathComponent("src/ffi.rs")
        let rust = try String(contentsOf: source, encoding: .utf8)
        #expect(rust.contains(#"const STOPPED: &str = "\#(CoreAnswer.stoppedWords)";"#))
    }

    @Test func aStoppedCoreIsSaidOnceAndToldNothingMore() throws {
        let h = try Harness()
        try h.started()
        let said = Said()
        h.client.onStop = { said.times += 1 }
        #expect(!h.client.stopped)
        h.recorder.effects.removeAll()

        h.client.noteStop(CoreAnswer.stoppedWords)
        h.client.noteStop(CoreAnswer.stoppedWords)

        #expect(h.client.stopped)
        #expect(said.times == 1)
        #expect(h.problems.list.count == 1)
        // What it last said stays to be drawn from; nothing more reaches it.
        #expect(h.client.nav.stage == .ready)
        h.client.send(.select(destination: 1))
        h.client.send(.refresh(screen: h.client.nav.front ?? 0))
        #expect(h.client.nav.selected == 0)
        #expect(h.recorder.effects.isEmpty)
        // It has no snapshot to give, which says nothing of what was stored before.
        #expect(h.client.snapshot() == nil)
    }

    @MainActor
    final class Said {
        var times = 0
    }
}
