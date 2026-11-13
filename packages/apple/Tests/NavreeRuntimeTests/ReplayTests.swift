import Foundation
import NavreeCoreBridge
import NavreeProtocol
import Testing

@testable import NavreeRuntime

/// Every conversation of the runtime core, replayed through the client's
/// loop: after each event the model the client kept, part by part as the
/// update named the parts, is what the core holds when read whole; every
/// effect reads; nothing the core says is refused.
@MainActor
@Test(arguments: Transcripts.names)
func aConversationOfTheCoreLeavesTheModelAsTheCoreHoldsIt(_ name: String) throws {
    let transcript = try Transcripts.load(name)
    let config = Transcripts.config(transcript["config"] as? [String: Any] ?? [:])
    var harness = try Harness(config: config)
    var kept: Data?
    let steps = transcript["steps"] as? [[String: Any]] ?? []
    #expect(!steps.isEmpty)
    for (at, step) in steps.enumerated() {
        if step["restart"] as? Bool == true {
            // The system ended the app: it starts again from what was stored.
            harness = try Harness(config: config)
        }
        var event = try #require(step["send"] as? [String: Any])
        if step["last"] as? Bool == true, let kept {
            event["snapshot"] = try JSONSerialization.jsonObject(with: kept)
        }
        if let body = step["body"] as? String {
            let text = try Transcripts.fixture(body)
            if event["t"] as? String == "cache" { event["value"] = text } else { event["body"] = text }
        }
        harness.recorder.effects.removeAll()
        harness.client.send(json: json(event), tag: event["t"] as? String ?? "?")

        #expect(harness.problems.list.isEmpty, "\(name), step \(at + 1): \(harness.problems.list)")
        harness.problems.list.removeAll()
        for effect in harness.recorder.effects {
            if case .unknown(let tag) = effect {
                Issue.record("\(name), step \(at + 1): an effect this client does not know: \(tag)")
            }
        }
        let expected = (step["expect"] as? [String: Any])?["effects"] as? [Any] ?? []
        #expect(
            harness.recorder.effects.count == expected.count,
            "\(name), step \(at + 1): \(harness.recorder.effects.count) effects read of \(expected.count)"
        )
        let kept0 = harness.client.fingerprint
        let whole = try harness.core.wholeFingerprint()
        #expect(kept0 == whole, "\(name), step \(at + 1): the model is not what the core holds\n\(kept0)\n---\n\(whole)")
        if step["snapshot"] != nil {
            kept = harness.client.snapshot()
        }
    }
}

/// Every effect of every conversation reads as one this client knows, and
/// every change as a part it reads.
@Test func everyEffectAndChangeTheCoreSaysReads() throws {
    var effects = 0
    for name in Transcripts.names {
        let transcript = try Transcripts.load(name)
        for step in transcript["steps"] as? [[String: Any]] ?? [] {
            let expect = step["expect"] as? [String: Any] ?? [:]
            let update = Update.read(json(expect))
            guard case .success(let read) = update else {
                Issue.record("\(name): \(update)")
                continue
            }
            #expect(read.unread.isEmpty, "\(name): \(read.unread)")
            for change in read.changed { #expect(change != .unknown, "\(name): \(expect["changed"] ?? "")") }
            for effect in read.effects {
                effects += 1
                if case .unknown(let tag) = effect { Issue.record("\(name): \(tag)") }
                if case .device(_, let device, _, _) = effect, !device.isKnown { Issue.record("\(name): a call \(device.name)") }
            }
        }
    }
    #expect(effects > 700)
}
