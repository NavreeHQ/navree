import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// Each rule of the design layer, as Apple's guidelines put it, with the case
// that proves it.

@Test func onAScreenTheCreateActionIsATrailingSymbolAndTheRestOverflow() {
    let slots = PlacementRules.place(
        [ActionTraits(role: .secondary), ActionTraits(role: .create, prominence: .high), ActionTraits(role: Navree.Role.none)],
        depth: .root
    )
    #expect(slots == [.overflow, .create, .overflow])
}

@Test func onlyOneActionIsEverProminent() {
    let slots = PlacementRules.place(
        [ActionTraits(role: .primary), ActionTraits(role: .primary), ActionTraits(role: .confirm)],
        depth: .pushed
    )
    #expect(slots == [.primary, .overflow, .overflow])
}

@Test func inASheetCancelIsTopLeadingAndTheConfirmingActionTopTrailing() {
    let slots = PlacementRules.place(
        [ActionTraits(role: .confirm), ActionTraits(role: .cancel)],
        depth: .sheet
    )
    #expect(slots == [.confirm, .cancel])
}

@Test func aDestructiveActionGetsASectionOfItsOwnAndIsNeverProminent() {
    let slots = PlacementRules.place(
        [ActionTraits(role: .destructive, prominence: .high), ActionTraits(role: .primary)],
        depth: .pushed
    )
    #expect(slots == [.destructiveSection, .primary])
    #expect(PlacementRules.place([ActionTraits(role: Navree.Role.none, standard: .delete)], depth: .root) == [.destructiveSection])
}

@Test func aStandardActionIsPlacedByWhatItIsWhateverItsRole() {
    let slots = PlacementRules.place(
        [ActionTraits(role: Navree.Role.none, standard: .add), ActionTraits(role: Navree.Role.none, standard: .done)],
        depth: .pushed
    )
    #expect(slots == [.create, .primary])
}

@Test func aDestructiveActionThatCanBeUndoneAsksNothingFirst() throws {
    let undoable = try decode(Navree.Node.self, #"{"t":"button","label":"Archive","role":"destructive","undoable":true}"#)
    let final = try decode(Navree.Node.self, #"{"t":"button","label":"Delete","role":"destructive"}"#)
    let plain = try decode(Navree.Node.self, #"{"t":"button","label":"Save","role":"primary"}"#)
    guard case .button(let a) = undoable, case .button(let b) = final, case .button(let c) = plain else {
        Issue.record("not buttons")
        return
    }
    #expect(PresentationRules.asksFirst(a) == false)
    #expect(PresentationRules.asksFirst(b) == true)
    #expect(PresentationRules.asksFirst(c) == false)
}

@Test func theTabBarIsChosenFromTheWidthNeverTheDevice() {
    #expect(NavigationRules.structure(destinations: 3, width: .compact) == .tabBar(visible: 3))
    #expect(NavigationRules.structure(destinations: 8, width: .compact) == .tabBar(visible: 5))
    #expect(NavigationRules.structure(destinations: 3, width: .regular) == .sidebarAdaptable)
    #expect(NavigationRules.structure(destinations: 1, width: .regular) == .single)
    #expect(NavigationRules.structure(destinations: 0, width: .compact) == .single)
}

@Test func aDestinationsFirstScreenHasALargeTitleAndTheRestInline() {
    #expect(NavigationRules.titleMode(.root) == .large)
    #expect(NavigationRules.titleMode(.pushed) == .inline)
    #expect(NavigationRules.titleMode(.sheet) == .inline)
    #expect(NavigationRules.title("Charkha Supply Co.", appName: "Charkha Supply Co.") == "")
}

@Test func searchSitsUnderTheTitleWhenCompactAndInTheToolbarWhenRegular() {
    #expect(PresentationRules.search(width: .compact) == .underTitle)
    #expect(PresentationRules.search(width: .regular) == .toolbar)
}

@Test func aFieldUnderTheTitleJoinsTheBarOnceTheScreenHasAppearedSoARootScreenOpensLarge() {
    #expect(PresentationRules.searchWaitsForFirstAppearance(.underTitle))
    #expect(!PresentationRules.searchWaitsForFirstAppearance(.toolbar))
}

@Test func aMediumSheetRestsAtHalfHeightAndAFullOneCoversTheScreen() {
    #expect(PresentationRules.sheet(.medium) == .resizable)
    #expect(PresentationRules.sheet(.large) == .large)
    #expect(PresentationRules.sheet(.full) == .fullScreen)
}

@Test func aDialogWithAFormIsASheetADestructiveQuestionAnActionSheetAnyOtherAnAlert() throws {
    let order = try recordedScreen("orders_1.json")
    let refund = try #require(order.body.find("refund", kinds: ["dialog"]))
    let cancel = try #require(order.body.find("cancel", kinds: ["dialog"]))
    let resend = try #require(order.body.find("resend", kinds: ["dialog"]))
    guard case .dialog(let r) = refund, case .dialog(let c) = cancel, case .dialog(let s) = resend else {
        Issue.record("not dialogs")
        return
    }
    #expect(PresentationRules.dialog(r) == .sheet)
    #expect(PresentationRules.dialog(c) == .confirmationDialog)
    #expect(PresentationRules.dialog(s) == .alert)
}

@Test func thereIsNoToastAFailureIsAnAlertAndAnythingElseAnInlineStatus() {
    #expect(PresentationRules.message(.danger) == .alert)
    for tone in [Navree.Tone.success, .info, .warning, .neutral, .accent] {
        #expect(PresentationRules.message(tone) == .inlineStatus)
    }
    #expect(PresentationRules.haptic(.success) == .success)
    #expect(PresentationRules.haptic(.info) == nil)
}

@Test func aScreenOfRecordsIsPlainRowsAndARecordsFactsAreInsetGroups() throws {
    #expect(ContentRules.list(try recordedScreen("orders.json").body) == .plain)
    #expect(ContentRules.list(try recordedScreen("inventory.json").body) == .plain)
    #expect(ContentRules.list(try recordedScreen("orders_1.json").body) == .insetGrouped)
}

@Test func aTableIsRowsWhenCompactAndColumnsWhenRegular() throws {
    guard case .table(let orders) = try #require(try recordedScreen("orders.json").body.first { $0.kind == "table" }) else {
        Issue.record("no table")
        return
    }
    #expect(ContentRules.table(orders, width: .compact) == .rows)
    #expect(ContentRules.table(orders, width: .regular) == .columns)
    // At an accessibility text size, rows at every width.
    #expect(ContentRules.table(orders, width: .regular, accessibilityText: true) == .rows)
    #expect(ContentRules.table(orders, width: .compact, accessibilityText: true) == .rows)
}

@Test func aWideTablesColumnsAreAsWideAsWhatTheyHoldAndTheNameTakesWhatIsLeft() {
    func column(_ role: Navree.ColumnRole?, numeric: Bool = false) -> Navree.Column {
        Navree.Column(label: "x", numeric: numeric, role: role)
    }
    #expect(ContentRules.columnKind(column(.title), controls: false) == .title)
    #expect(ContentRules.columnKind(column(.trailing, numeric: true), controls: false) == .number)
    #expect(ContentRules.columnKind(column(.status), controls: false) == .status)
    #expect(ContentRules.columnKind(column(nil), controls: true) == .controls)
    #expect(ContentRules.columnKind(column(nil), controls: false) == .words)
    // The name asks for room and has no most: it takes what the others leave.
    let name = ContentRules.columnWidth(.title, widest: 150)
    #expect(name.max == nil && name.ideal >= 240)
    // A price asks for less than a place's name; neither for the name's room.
    let price = ContentRules.columnWidth(.number, widest: 40)
    let place = ContentRules.columnWidth(.words, widest: 130)
    #expect(price.ideal < place.ideal)
    #expect(place.ideal < name.ideal)
    #expect(price.ideal >= price.min && price.ideal <= price.max ?? .infinity)
    // A column of buttons is as wide as its buttons, no wider and no narrower.
    let restock = ContentRules.columnWidth(.controls, widest: 70)
    #expect(restock.min == restock.ideal && restock.ideal == restock.max)
    // Long words do not stretch a column without end.
    #expect(ContentRules.columnWidth(.words, widest: 900).ideal == 260)
}

@Test func iOSHasNoCheckboxesOrRadioButtons() throws {
    let checkbox = try decode(Navree.Input.self, #"{"t":"toggle","kind":"checkbox"}"#)
    let radio = try decode(Navree.Input.self, #"{"t":"choice","kind":"radio","options":[{"value":"a","label":"A"},{"value":"b","label":"B"}]}"#)
    let many = try decode(Navree.Input.self, #"{"t":"choices","options":[{"value":"a","label":"A"}]}"#)
    #expect(ContentRules.control(checkbox) == .toggle)
    #expect(ContentRules.control(radio) == .inlinePicker)
    #expect(ContentRules.control(many) == .multipleChoice)
    let password = try decode(Navree.Input.self, #"{"t":"text","kind":"password","autofill":"new-password"}"#)
    #expect(ContentRules.control(password) == .secureField)
    guard case .text(let text) = password else { return }
    #expect(ContentRules.keyboard(text).content == .newPassword)
}

@Test func aRowOfButtonsBecomesRowsOfAGroupedList() throws {
    let group = try decode([Navree.Node].self, #"[{"t":"button","label":"Refund"},{"t":"button","label":"Cancel order","role":"destructive"}]"#)
    #expect(ContentRules.isButtonGroup(group))
    #expect(!ContentRules.isButtonGroup(Array(group.prefix(1))))
}

@Test func standardActionsTakeTheSystemsOwnSymbols() {
    #expect(Symbols.standard(.add) == "plus")
    #expect(Symbols.standard(.share) == "square.and.arrow.up")
    #expect(Symbols.standard(.delete) == "trash")
    #expect(Symbols.standard(.more) == "ellipsis.circle")
    #expect(Symbols.standard(.cancel) == "xmark")
    #expect(Symbols.standard(.done) == "checkmark")
    #expect(Symbols.forRole(.create) == "plus")
    #expect(Symbols.system(for: "shopping-bag") == "bag")
    #expect(Symbols.system(for: "no-such-icon") == nil)
}

@Test func aStatusIsWordsWithASymbolInItsColourNeverAColouredPill() {
    #expect(Tones.symbol(.success) == "checkmark.circle.fill")
    #expect(Tones.symbol(.danger) == "xmark.octagon.fill")
    #expect(Tones.symbol(.warning) == "exclamationmark.triangle.fill")
    #expect(Tones.symbol(.info) == "info.circle.fill")
}

// MARK: - Helpers

func decode<T: Decodable & Sendable>(_ type: T.Type, _ json: String) throws -> T {
    try WireDecoder.decode(type, from: Data(json.utf8))
}

func recordedScreen(_ name: String) throws -> Navree.Screen {
    let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    let data = try Data(contentsOf: root.appendingPathComponent("Demo/NavreeDemo/Recorded/\(name)"))
    return try #require(try WireDecoder.decode(Navree.Envelope.self, from: data).screen)
}

@Test func aPageInTheWebViewHasAnInlineTitleEvenAtTheRoot() {
    #expect(NavigationRules.webTitleMode(.root) == .inline)
    #expect(NavigationRules.titleMode(.root) == .large)
}

@Test func aWideWindowShowsTheShellsGroupsAsSectionsOnlyWhenThereAreSeveral() {
    #expect(NavigationRules.groupsAsSections(1) == false)
    #expect(NavigationRules.groupsAsSections(2) == true)
}
