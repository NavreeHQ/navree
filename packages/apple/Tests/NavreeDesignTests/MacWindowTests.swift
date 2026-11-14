import Foundation
import NavreeDesign
import NavreeProtocol
import SwiftUI
import Testing

// A record's own actions and where a Mac window puts them, what the app's
// accent colours, and where a screen beside its list says its name: each
// rule with the case that proves it.

/// The public corpus's screens and the demos' answers kept beside these tests.
private func everyScreen() throws -> [(String, Navree.Screen)] {
    let here = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    let corpus = here.deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().appendingPathComponent("crates/navree-native/corpus")
    let answers = here.appendingPathComponent("Answers")
    var found: [(String, Navree.Screen)] = []
    for folder in [corpus, answers] {
        for file in try FileManager.default.contentsOfDirectory(atPath: folder.path).sorted() where file.hasSuffix(".json") {
            let data = try Data(contentsOf: folder.appendingPathComponent(file))
            if let screen = try? WireDecoder.decode(Navree.Envelope.self, from: data).screen {
                found.append((file, screen))
            }
        }
    }
    return found
}

private func corpusScreen(_ name: String) throws -> Navree.Screen {
    try #require(try everyScreen().first { $0.0 == name }?.1)
}

private func labels(_ nodes: [Navree.Node]) -> [String] {
    nodes.compactMap { node in
        if case .button(let button) = node { return button.label }
        return nil
    }
}

@Test func aRowOfButtonsAtAPagesHeadIsTheScreensOwnActions() throws {
    let order = try corpusScreen("screen-detail.json")
    #expect(order.actions.isEmpty)
    let lifted = ActionRules.lifted(order, depth: .pushed)
    #expect(labels(lifted.actions) == ["Refund", "Cancel order", "Resend confirmation"])
    // The row has left the content; the states above it and everything after stay.
    var rows = 0
    lifted.body.walk { node in
        if case .button(let button) = node, ["Refund", "Cancel order", "Resend confirmation"].contains(button.label) { rows += 1 }
    }
    // What stays are the dialogs' own answers: Cancel order in its question,
    // Refund as the refund form's submit.
    #expect(rows == 2)
    guard case .stack(let before) = order.body, case .stack(let after) = lifted.body else {
        Issue.record("the page is not a stack")
        return
    }
    #expect(after.children.count == before.children.count - 1)
    #expect(after.children.first == before.children.first)
}

@Test func aPresentedScreenKeepsItsButtonsWhereTheyAre() throws {
    let order = try corpusScreen("screen-detail.json")
    #expect(ActionRules.lifted(order, depth: .sheet) == order)
    #expect(ActionRules.lifted(order, depth: .cover) == order)
    #expect(ActionRules.lifted(order, depth: .root).actions.count == 3)
}

@Test func onlyARowAboveWhichStandOnlyWordsAndStatesIsAHead() throws {
    var lifted: [String] = []
    for (name, screen) in try everyScreen() where ActionRules.head(screen.body) != nil {
        lifted.append(name)
    }
    // Every other page of the corpus and of the demos keeps its content: a
    // form's submit, a card's buttons, a thread's controls, a list's rows.
    #expect(lifted == ["screen-detail.json"])
    let row = Navree.Node.stack(Navree.Stack(axis: .horizontal, children: [.button(Navree.Button(label: "Archive"))]))
    let words = Navree.Node.text(Navree.Text(value: "Placed today"))
    let list = Navree.Node.list(Navree.List())
    #expect(ActionRules.head(.stack(Navree.Stack(children: [words, row]))) != nil)
    #expect(ActionRules.head(.stack(Navree.Stack(children: [list, row]))) == nil)
    #expect(ActionRules.head(.stack(Navree.Stack(axis: .horizontal, children: [row]))) == nil)
}

@Test func theLiveOrdersActionsAreAlreadyTheScreensAndStayAsTheyAre() throws {
    let order = try answerScreen("store-order1")
    #expect(ActionRules.lifted(order, depth: .pushed) == order)
    #expect(labels(order.actions) == ["Resend confirmation", "Refund", "Cancel order"])
}

@Test func onAMacWindowEveryActionIsInTheToolbarAndWhatDestroysOnlyInTheMenu() throws {
    let lifted = ActionRules.lifted(try corpusScreen("screen-detail.json"), depth: .pushed)
    let traits = lifted.actions.compactMap(ActionTraits.init)
    // A navigation bar: the overflow menu, and a red section at the end of the content.
    #expect(PlacementRules.place(traits, depth: .pushed, bar: .navigation) == [.overflow, .destructiveSection, .overflow])
    // A Mac window: the toolbar, and the menu bar's menu alone for what destroys.
    #expect(PlacementRules.place(traits, depth: .pushed, bar: .window) == [.window, .menu, .window])
    // Making something new and the one main act keep their places on both.
    let make = [ActionTraits(role: .create), ActionTraits(role: .primary), ActionTraits(role: .primary)]
    #expect(PlacementRules.place(make, depth: .root, bar: .window) == [.create, .primary, .window])
    // A sheet is a sheet on both: Cancel and Done, what destroys at its end.
    let sheet = [ActionTraits(role: .cancel), ActionTraits(role: .confirm), ActionTraits(role: .destructive)]
    #expect(PlacementRules.place(sheet, depth: .sheet, bar: .window) == [.cancel, .confirm, .destructiveSection])
    #expect(PlacementRules.place(sheet, depth: .sheet, bar: .navigation) == [.cancel, .confirm, .destructiveSection])
}

@Test func theMenuBarHoldsTheHeadsActionsWithWhatDestroysLast() throws {
    let lifted = ActionRules.lifted(try corpusScreen("screen-detail.json"), depth: .pushed)
    let leaves = CommandRules.leaves(lifted.actions)
    let entries = CommandRules.entries(leaves)
    // Resend confirmation does nothing yet (no action): it is not a command.
    #expect(entries.map(\.title) == ["Refund", "Cancel Order"])
    #expect(entries.map(\.slot) == [.screen, .destructive])
}

@Test func onlyTheMainActAndTheConfirmingOneKeepTheAccentInABar() {
    let tinted: [Slot] = [.primary, .confirm]
    let plain: [Slot] = [.create, .cancel, .overflow, .destructiveSection, .leading, .bar, .window, .menu]
    #expect(tinted.allSatisfy(TintRules.keepsAccent))
    #expect(!plain.contains(where: TintRules.keepsAccent))
    // A Mac takes the accent the person chose; iPhone and iPad the app's.
    #expect(TintRules.appliesAppAccent(bar: .navigation))
    #expect(!TintRules.appliesAppAccent(bar: .window))
}

@Test func aScreenBesideItsListOnAMacSaysItsNameAtItsHead() {
    #expect(NavigationRules.titleInContent(bar: .window, besideList: true))
    #expect(!NavigationRules.titleInContent(bar: .window, besideList: false))
    #expect(!NavigationRules.titleInContent(bar: .navigation, besideList: true))
}

@Test func theBarsAreTheSystemsOwn() {
    #if os(macOS)
    #expect(ActionBar.current == .window)
    #else
    #expect(ActionBar.current == .navigation)
    #endif
}
