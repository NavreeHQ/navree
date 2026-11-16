import NavreeProtocol
import Testing

@testable import NavreeDesign

/// Rows, records and parts are told apart by their ids, never by their place.
struct IdentityTests {
    static func order(_ id: String?, _ title: String) -> Navree.TableRow {
        Navree.TableRow(cells: [.text(Navree.Text(value: title))], id: id)
    }

    static let columns = [Navree.Column(label: "Order", role: .title)]

    static func rows(_ records: [Navree.TableRow]) -> [RowItem] {
        records.map { .record(RecordRules.row(columns: columns, record: $0)) }
    }

    @Test func aRecordKeepsItsNameWhenAnOrderArrivesBeforeIt() {
        let first = Self.order("1001", "CS-1001"), second = Self.order("1002", "CS-1002")
        let before = IdentityRules.told(Self.rows([first, second])) { $0.identity }
        // A new order arrives at the top of a list kept live.
        let after = IdentityRules.told(Self.rows([Self.order("1003", "CS-1003"), first, second])) { $0.identity }
        #expect(before.map(\.id) == ["#record 1001", "#record 1002"])
        #expect(after.map(\.id) == ["#record 1003", "#record 1001", "#record 1002"])
        // The row that held 1001 is still 1001's, at another place.
        #expect(after.first { $0.id == before[0].id }?.index == 1)
        #expect(after.first { $0.id == before[0].id }?.item == before[0].item)
    }

    @Test func rowsWithNoIdAreKnownByTheirPlaceAndTheSameIdTwiceStaysTwoRows() {
        #expect(IdentityRules.identities([nil, "a", "a", "", nil]) == ["@0", "#a", "#a'", "@3", "@4"])
        let twice = IdentityRules.told(Self.rows([Self.order("7", "A"), Self.order("7", "B"), Self.order(nil, "C")])) { $0.identity }
        #expect(Set(twice.map(\.id)).count == 3)
    }

    @Test func eachKindOfRowIsNamedByWhatTheTreeGivesIt() {
        let quantity = Navree.Field(id: "lines.qty[]~2", input: .text(Navree.TextInput()), name: "qty[]")
        let form = Navree.Form(action: "/orders/1/lines", id: "lines")
        #expect(RowItem.field(quantity, form: form).identity == "field lines.qty[]~2")
        #expect(RowItem.option(quantity, Navree.ChoiceOption(label: "Two", value: "2"), form: form).identity == "option lines.qty[]~2 2")
        #expect(RowItem.row(Navree.Row(id: "card-9", title: "Pack order")).identity == "row card-9")
        #expect(RowItem.row(Navree.Row(title: "Pack order")).identity == nil)
        #expect(RowItem.pending(region: "totals").identity == "pending totals")
        #expect(RowItem.more(target: "orders", href: "/orders?after=20").identity == "more orders")
        #expect(RowItem.labeled(label: "Subtotal", value: "₹7,848", muted: false).identity == nil)
        // A field the core has not named is known by its name; one it has, by its id.
        #expect(Navree.Field(input: .text(Navree.TextInput()), name: "qty[]").key == "qty[]")
        #expect(quantity.key == "lines.qty[]~2")
    }

    @Test func aFormOpenedFromARowCarriesTheTitleOfTheRowItWasTakenFrom() {
        let move = Navree.Form(
            action: "/board/moves",
            children: [
                .field(Navree.Field(input: .text(Navree.TextInput()), name: "to")),
                .button(Navree.Button(action: .submit(Navree.Action.Submit()), label: "Move")),
            ],
            method: .post
        )
        let card = Navree.Row(
            detail: [.disclosure(Navree.Disclosure(children: [.form(move)], label: "Move"))], id: "card-9",
            title: "Refund CS-1001"
        )
        let opening = SpeechRules.offers(card).openings.first
        #expect(opening?.form.action == "/board/moves")
        #expect(opening?.subject == "Refund CS-1001")
    }

    @Test func aWideTablesRecordsAndAScreensPartsAreNamedTheSameWay() {
        let records = [Self.order("1001", "CS-1001"), Self.order(nil, "CS-1002")]
        #expect(IdentityRules.told(records) { $0.id }.map(\.id) == ["#1001", "@1"])
        let parts: [Navree.Node] = [.card(Navree.Card(id: "today")), .text(Navree.Text(value: "Orders"))]
        #expect(parts.told.map(\.id) == CompositionRules.identities(parts))
    }
}
