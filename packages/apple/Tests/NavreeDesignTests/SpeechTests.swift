import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// What VoiceOver says of the parts, over the corpus and the demos' answers:
// each rule with the case that proves it.

private func corpus(_ name: String) throws -> Navree.Screen {
    let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
    let data = try Data(contentsOf: root.appendingPathComponent("crates/navree-native/corpus/\(name)"))
    return try #require(try WireDecoder.decode(Navree.Envelope.self, from: data).screen)
}

private func rows(_ node: Navree.Node) -> [Navree.Row] {
    var found: [Navree.Row] = []
    node.walk { if case .row(let row) = $0 { found.append(row) } }
    return found
}

@Test func aFactIsOneStopOfItsLabelAndItsValue() throws {
    #expect(SpeechRules.fact(label: "Subtotal", value: "₹428") == "Subtotal, ₹428")
    #expect(SpeechRules.fact(label: "Ships to", value: " ") == "Ships to")
    // Every fact of the corpus order reads as its label and its value.
    var facts: [Navree.Fact] = []
    try corpus("screen-detail.json").body.walk { if case .facts(let part) = $0 { facts += part.items } }
    #expect(!facts.isEmpty)
    for fact in facts {
        let value = fact.value.flatMap { RecordRules.words(in: $0) }.joined(separator: " ")
        #expect(SpeechRules.fact(label: fact.label, value: value).hasPrefix(fact.label + ", "))
    }
}

@Test func aPartsValueDoesNotSayItsNameAgain() throws {
    #expect(SpeechRules.value("Health score: 79 of 100. Good.", after: "Health score") == "79 of 100. Good.")
    #expect(SpeechRules.value("Recovery: 66%. Moderate.", after: "Recovery") == "66%. Moderate.")
    // Only a whole word at the start goes.
    #expect(SpeechRules.value("Recoveryish 3", after: "Recovery") == "Recoveryish 3")
    #expect(SpeechRules.value("8,324 steps", after: "Steps by hour") == "8,324 steps")
    #expect(SpeechRules.value("Health score", after: "Health score") == "Health score")
    #expect(SpeechRules.value("79", after: nil) == "79")
    // Every ring, gauge and chart of the public health screens says its name once.
    for file in ["screen-designed-health.json", "screen-today.json", "screen-sleep.json", "screen-fitness.json"] {
        try corpus(file).body.walk { node in
            var pair: (String?, String?)?
            switch node {
            case .ring(let ring): pair = (ring.label, ring.summary)
            case .rings(let rings): pair = (rings.title, rings.summary)
            case .gauge(let gauge): pair = (gauge.label, gauge.summary)
            case .meter(let meter): pair = (meter.title, meter.summary)
            case .dayClock(let clock): pair = (clock.title ?? clock.label, clock.summary)
            default: break
            }
            guard let (label, summary) = pair, let label, let summary, !label.isEmpty else { return }
            let said = SpeechRules.value(summary, after: label)
            #expect(!said.lowercased().hasPrefix(label.lowercased() + ":"), "\(file): \(label) / \(said)")
        }
    }
}

@Test func aRowsBarSaysItsScoreOnceAfterTheRowsNameAndState() {
    let words = SpeechRules.barInRow("HRV: Balanced, 58 of 100", title: "HRV", state: "Balanced")
    #expect(words == "58 of 100")
    #expect(SpeechRules.barInRow("Resting HR: Normal, 75 of 100", title: "Resting HR", state: "Normal") == "75 of 100")
    // Words the row has not said stay.
    #expect(SpeechRules.barInRow("3 of 5 done", title: "Packing", state: nil) == "3 of 5 done")
    #expect(SpeechRules.barInRow(nil, title: "Packing", state: nil) == nil)
    // The number at the row's end is not said again.
    #expect(!SpeechRules.saysTrailing("58", bar: words))
    #expect(!SpeechRules.saysTrailing("75", bar: "75 of 100"))
    #expect(SpeechRules.saysTrailing("09:41", bar: "75 of 100"))
    #expect(SpeechRules.saysTrailing("12", bar: "75 of 100"))
    #expect(SpeechRules.saysTrailing("58", bar: nil))
    #expect(SpeechRules.saysTrailing("Next", bar: "58 of 100"))
    #expect(!SpeechRules.saysTrailing("2,137 steps", bar: "2,137 steps"))
    #expect(SpeechRules.saysTrailing("7", bar: "75 of 100"))
    #expect(!SpeechRules.saysTrailing("33 / 40", bar: "33 of 40 points"))
    #expect(SpeechRules.saysTrailing("40 / 33", bar: "33 of 40 points"))
}

@Test func aPersonIsNamedOnce() throws {
    #expect(!SpeechRules.saysAvatar("Tejinder Randhawa", beside: ["Tejinder Randhawa"]))
    #expect(!SpeechRules.saysAvatar("Neha Kapoor", beside: ["Neha"]))
    #expect(SpeechRules.saysAvatar("Neha Kapoor", beside: ["Offline order sheets"]))
    // The live order's customer: a row led by their picture and titled with their name.
    let customer = try #require(rows(try answerScreen("store-order1").body).first { $0.title == "Tanvi Sharma" })
    guard case .avatar(let avatar)? = customer.leading else {
        Issue.record("the customer row is not led by a picture")
        return
    }
    #expect(!SpeechRules.saysAvatar(avatar.name, beside: [customer.title]))
    // Every card of the board names its person once, by their whole name.
    var people: [String] = []
    try answerScreen("board-root").body.walk { node in
        if case .stack(let stack) = node, let name = SpeechRules.person(in: stack) { people.append(name) }
    }
    #expect(people.count >= 20)
    #expect(people.contains("Neha Kapoor"))
    #expect(people.allSatisfy { $0.contains(" ") })
}

@Test func aBoardCardSaysItsWordsWithMoveAsAnAction() throws {
    let cards = rows(try answerScreen("board-root").body)
    #expect(cards.count >= 20)
    for card in cards {
        let offers = SpeechRules.offers(card)
        // Move is an action that opens its form, never a heading to expand.
        #expect(offers.openings.map(\.label) == ["Move"], "\(card.title)")
        #expect(offers.openings.first?.submit.label == "Move")
        #expect(offers.openings.first?.form.action == "/board/moves")
        // The button that says the card's title again is the card's own action.
        #expect(offers.titleButton?.label == card.title)
        #expect(offers.titleForm?.action.hasSuffix("/peek") == true)
    }
    // A row with nothing but words offers nothing.
    let order = rows(try corpus("screen-detail.json").body)
    #expect(order.allSatisfy { SpeechRules.offers($0) == SpeechRules.RowOffers() })
}

@Test func aStepsStateIsSaidInWordsNeverAsASelection() {
    #expect(SpeechRules.step(.done) == "Done")
    #expect(SpeechRules.step(.current) == "Current")
    #expect(SpeechRules.step(.upcoming) == "Upcoming")
}

/// The health app's recorded answers, when a folder of them is named at run
/// time (`NAVREE_HEALTH_ANSWERS`); nothing of them is kept here.
@Test func theRecordedHealthRowsSayEachScoreOnce() throws {
    guard let folder = ProcessInfo.processInfo.environment["NAVREE_HEALTH_ANSWERS"] else { return }
    var checked = 0
    for name in ["health-today-ios-phone.json", "health-sleep-ios-phone.json", "health-fitness-ios-phone.json"] {
        let url = URL(fileURLWithPath: folder).appendingPathComponent(name)
        guard let data = try? Data(contentsOf: url),
            let screen = try WireDecoder.decode(Navree.Envelope.self, from: data).screen
        else { continue }
        for row in rows(screen.body) {
            var bar: Navree.Progress?
            for part in row.detail { part.walk { if case .progress(let found) = $0, bar == nil { bar = found } } }
            guard let bar, let label = bar.label else { continue }
            let said = try #require(SpeechRules.barInRow(label, title: row.title, state: row.badge?.text))
            #expect(!said.hasPrefix(row.title), "\(row.title): \(said)")
            if case .text(let end)? = row.trailing {
                #expect(!SpeechRules.saysTrailing(end.value, bar: said), "\(row.title): \(end.value) after \(said)")
            }
            checked += 1
        }
    }
    #expect(checked > 0)
}
