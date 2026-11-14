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

@Test func aDetailInAMacWindowStopsAtTheGroupedFormsWidthAndADestinationOrAPhoneKeepsTheWindows() {
    // A pushed screen, a screen beside its list, a sheet: no wider than a grouped form's sections.
    #expect(CompositionRules.readableWidth(bar: .window, depth: .pushed) == 704)
    #expect(CompositionRules.readableWidth(bar: .window, depth: .sheet) == 704)
    // A destination's own screen (a dashboard of tiles) and iPhone and iPad: the window's width.
    #expect(CompositionRules.readableWidth(bar: .window, depth: .root) == nil)
    #expect(CompositionRules.readableWidth(bar: .navigation, depth: .pushed) == nil)
    #expect(CompositionRules.readableWidth(bar: .navigation, depth: .root) == nil)
    // The room in a wide detail column (1220 points) and in a narrow one (740).
    #expect(CompositionRules.room(width: 1220, readable: 704) == 704)
    #expect(CompositionRules.room(width: 740, readable: 704) == 700)
    // With no readable width the room is what it always was.
    #expect(CompositionRules.room(width: 1220, readable: nil) == CompositionRules.room(width: 1220))
    #expect(CompositionRules.room(width: 402, readable: nil) == 370)
}

@Test func inAMacWindowTheWordsOnTheGroundStandWithTheWordsInItsSurfacesAndOnAPhoneAtTheSurfacesEdge() throws {
    #expect(CompositionRules.groundWordsInset(bar: .window) == CompositionRules.surfaceInset)
    #expect(CompositionRules.surfaceInset == 16)
    // iPhone and iPad: a heading starts at its card's edge, as before.
    #expect(CompositionRules.groundWordsInset(bar: .navigation) == 0)
    // The live order: its progress (a timeline in a section with no surface)
    // is words on the ground; its card and its sections hold their own, and
    // the facts in an inset section stand on that section's surface.
    let order = try corpusScreen("store-order1.json")
    var words: [String] = []
    var held: [String] = []
    if case .stack(let page) = order.body {
        for part in page.children {
            switch part {
            case .sheet, .dialog: continue
            default: break
            }
            if CompositionRules.wordsOnGround(part) { words.append(part.id ?? "") } else { held.append(part.id ?? "") }
            if case .section(let section) = part, !CompositionRules.standsOnSurface(part) {
                for inner in section.children where CompositionRules.wordsOnGround(inner) { words.append(inner.id ?? "") }
            }
        }
    }
    #expect(words == ["order-steps"])
    #expect(held == ["order-state", "order-items", "order-customer", "order-delivery", "order-progress", "order-others"])
}

@Test func aLabelledValueStandsAtTheTrailingEndInAMacWindowAsOnAPhone() {
    // A Mac's own labelled content sets the value just after its label outside a form.
    #expect(CompositionRules.valuePlace(bar: .window) == .trailing)
    // iPhone and iPad set it at the trailing end themselves.
    #expect(CompositionRules.valuePlace(bar: .navigation) == .system)
}

@Test func aSheetInAMacWindowTakesTheSystemsSizeForAFormAndOnAPhoneItsDetents() {
    // A Mac sheet takes its content's own height otherwise, and a list has none.
    #expect(PresentationRules.sheetSizing(bar: .window) == .form)
    #expect(PresentationRules.sheetSizing(bar: .navigation) == .detents)
}

@Test func aMacSheetTakesTheFormsSizeWithinTheRoomOfTheWindowItStandsOn() {
    let form = PresentationRules.formSize
    #expect(form == CGSize(width: 470, height: 400))
    // A full window has room for a form.
    #expect(PresentationRules.sheetSize(form: form, room: CGSize(width: 960, height: 628)) == form)
    // The window under an item in the menu bar is 400 points wide: the sheet
    // takes that width, never hanging over the window's edges.
    #expect(PresentationRules.sheetSize(form: form, room: CGSize(width: 400, height: 508)) == CGSize(width: 400, height: 400))
    // A window shorter than a form: the sheet stops at the window's foot and its content scrolls.
    #expect(PresentationRules.sheetSize(form: form, room: CGSize(width: 400, height: 300)) == CGSize(width: 400, height: 300))
    // A window that says nothing of its room, or has none yet, leaves the form's size.
    #expect(PresentationRules.sheetSize(form: form, room: nil) == form)
    #expect(PresentationRules.sheetSize(form: form, room: .zero) == form)
}

@Test func aMacSheetIsAsTallAsItsContentUpToAFormAndWithinItsWindow() {
    let form = PresentationRules.formSize
    // A sheet of one field (its bar, one row, its buttons) is that tall, not a form's 400.
    #expect(PresentationRules.sheetSize(form: form, fitting: 212, room: CGSize(width: 960, height: 628)) == CGSize(width: 470, height: 212))
    // Content taller than a form: the form's height, the content scrolling.
    #expect(PresentationRules.sheetSize(form: form, fitting: 900, room: CGSize(width: 960, height: 628)) == form)
    // In the window under an item in the menu bar: its width, the content's height.
    #expect(PresentationRules.sheetSize(form: form, fitting: 212, room: CGSize(width: 400, height: 508)) == CGSize(width: 400, height: 212))
    // A window shorter than the content: the window's room.
    #expect(PresentationRules.sheetSize(form: form, fitting: 380, room: CGSize(width: 400, height: 300)) == CGSize(width: 400, height: 300))
    // Until a list has said how tall it is (it has no height of its own), the
    // form's size, so the list shows rather than the sheet shrinking to its bar.
    #expect(PresentationRules.sheetSize(form: form, fitting: nil, room: CGSize(width: 960, height: 628)) == form)
    #expect(PresentationRules.sheetSize(form: form, fitting: 0, room: CGSize(width: 960, height: 628)) == form)
    // Moved from the 400-point window under the item into a full window, the
    // same sheet takes the full window's room: a form's width again.
    let panel = PresentationRules.sheetSize(form: form, fitting: 300, room: CGSize(width: 400, height: 508))
    let window = PresentationRules.sheetSize(form: form, fitting: 300, room: CGSize(width: 960, height: 628))
    #expect(panel.width == 400 && window.width == 470)
}

@Test func escapeCancelsAMacSheetEvenWhileAFieldIsBeingTypedIn() {
    // The system's own form sheets on a Mac cancel on the first Escape.
    #expect(PresentationRules.escapeWhileTyping(bar: .window, composing: false) == .cancelsSheet)
    // A character being composed with an input method keeps its Escape.
    #expect(PresentationRules.escapeWhileTyping(bar: .window, composing: true) == .system)
    // iPhone and iPad keep the system's own.
    #expect(PresentationRules.escapeWhileTyping(bar: .navigation, composing: false) == .system)
}

@Test func aSheetsButtonsAreWordsOnAMacAndSymbolsOnAPhone() {
    // A Mac sheet's push buttons: "Cancel" and the action's own words, the
    // confirming one the default button.
    #expect(PresentationRules.sheetButtons(bar: .window) == .words)
    // iPhone and iPad keep ✕ and ✓ in the sheet's bar.
    #expect(PresentationRules.sheetButtons(bar: .navigation) == .symbols)
}

@Test func aClickOnARowOfAMacsTableChoosesItAndOpensItsRecord() throws {
    #expect(ContentRules.tableOpening(bar: .window) == .choosing)
    // iPad keeps the tap on the record's name.
    #expect(ContentRules.tableOpening(bar: .navigation) == .firstCell)
    // The live store's orders: every record opens its glance, so any of its rows opens on a click.
    let orders = try corpusScreen("store-orders.json")
    var records: [Navree.TableRow] = []
    orders.body.walk { node in
        if case .table(let table) = node { records += table.rows }
    }
    #expect(!records.isEmpty)
    #expect(records.allSatisfy { $0.action != nil })
}

@Test func aMacsTableNamesARecordInWordsWhereItsRowOpensItAndSetsItsControlsAsSmallPushButtons() throws {
    let orders = try corpusScreen("store-orders.json")
    var first: Navree.TableRow?
    orders.body.walk { node in
        if first == nil, case .table(let table) = node { first = table.rows.first }
    }
    let record = try #require(first)
    // The order's name cell holds the web page's button to its glance; on a Mac the row's click is that way in.
    #expect(ContentRules.nameWords(record.cells[0], opening: .choosing, opens: true) == ["CS-220480", "Tejinder Randhawa"])
    // iPad keeps the name's own button, and a record that opens nothing keeps its cell as it is.
    #expect(ContentRules.nameWords(record.cells[0], opening: .firstCell, opens: true) == nil)
    #expect(ContentRules.nameWords(record.cells[0], opening: .choosing, opens: false) == nil)
    // A name cell of words only is drawn as it is.
    #expect(ContentRules.nameWords(.text(Navree.Text(value: "Chakla belan")), opening: .choosing, opens: true) == nil)
    #expect(ContentRules.cellControlLook(bar: .window) == .smallPushButton)
    #expect(ContentRules.cellControlLook(bar: .navigation) == .borderless)
}

@Test func aMacDetailsReadableWidthGrowsWithThePersonsText() {
    #expect(CompositionRules.readableWidth(bar: .window, depth: .pushed, textScale: 1) == 704)
    // Text a quarter larger: a line holds as many words.
    #expect(CompositionRules.readableWidth(bar: .window, depth: .pushed, textScale: 1.25) == 880)
    // Never narrower than the grouped form's own width.
    #expect(CompositionRules.readableWidth(bar: .window, depth: .sheet, textScale: 0.9) == 704)
    #expect(CompositionRules.readableWidth(bar: .navigation, depth: .pushed, textScale: 1.25) == nil)
}

@Test func aMacListsSymbolsStandInOneColumnAndItsBorderlessButtonsReadAsControls() {
    // Customer, Payment and Ships from start at one edge, whatever each symbol's width.
    #expect(ContentRules.symbolColumn(bar: .window) == 20)
    #expect(ContentRules.symbolColumn(bar: .navigation) == nil)
    // "Saved replies" in the accent on a Mac; the system's own tint on iPhone and iPad.
    #expect(ContentRules.lowProminenceWords(bar: .window) == .accent)
    #expect(ContentRules.lowProminenceWords(bar: .navigation) == .system)
}

@Test func aClickAnywhereOnAMacRowOpensWhatItsTitleButtonOpens() {
    // A board card on a Mac opens its peek from a click anywhere on it.
    #expect(ContentRules.rowOpening(bar: .window) == .click)
    // On iPhone and iPad the button and VoiceOver's default action.
    #expect(ContentRules.rowOpening(bar: .navigation) == .button)
}

@Test func aMacWindowsDetailKeepsTheWidthEveryScreenIsLaidOutFor() {
    // The width of the window under an item in the menu bar: the window holds
    // the sidebar's least beside it rather than squeezing the detail.
    #expect(NavigationRules.detailMinimumWidth == 400)
}
