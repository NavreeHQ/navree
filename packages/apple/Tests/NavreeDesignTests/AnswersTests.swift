import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// Today's answers of the store, inbox and board demos, as their servers sent
// them to an iPhone: each rule the drawing reads them by.

/// An answer kept beside these tests.
func answer(_ name: String) throws -> Navree.Envelope {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("Answers/\(name).json")
    return try WireDecoder.decode(Navree.Envelope.self, from: Data(contentsOf: url))
}

func answerScreen(_ name: String) throws -> Navree.Screen {
    try #require(try answer(name).screen)
}

func firstTable(_ screen: Navree.Screen) throws -> Navree.Table {
    guard case .table(let table) = try #require(screen.body.first { $0.kind == "table" }) else {
        throw Missing()
    }
    return table
}

struct Missing: Error {}

@Test func anOrderReadsAsARowByWhatItsColumnsAre() throws {
    let orders = try firstTable(try answerScreen("store-orders"))
    let row = RecordRules.row(columns: orders.columns, record: orders.rows[0])
    #expect(row.title == "CS-220480")
    // The customer is the subtitle column's; the title cell's own copy of it is not repeated.
    #expect(row.subtitle.first == "Tejinder Randhawa")
    #expect(row.subtitle.filter { $0 == "Tejinder Randhawa" }.count == 1)
    #expect(row.trailing == "₹507")
    #expect(row.trailingLabel == "Total")
    #expect(row.statuses.map(\.text) == ["Open", "Unfulfilled"])
    // Payment and Ships to may be left out of a narrow window.
    #expect(!row.subtitle.contains("Paid"))
    #expect(!row.subtitle.contains { $0.contains("Ludhiana") })
    #expect(row.controls.isEmpty)
    // The row opens what the server says: its peek, which presents a sheet.
    guard case .request(let request)? = row.action else {
        Issue.record("the row does not open its peek")
        return
    }
    #expect(request.href == "/orders/200000/peek")
}

@Test func aProductKeepsItsRestockFormAsAnInlineEdit() throws {
    let products = try firstTable(try answerScreen("store-inventory"))
    let row = RecordRules.row(columns: products.columns, record: products.rows[0])
    #expect(row.title == "Chakla belan, Amritsar")
    #expect(row.subtitle.first == "KIT-307-000")
    #expect(row.trailing == "0")
    #expect(row.trailingLabel == "In stock")
    #expect(row.statuses.map(\.text) == ["Out of stock"])
    #expect(row.controls.count == 1)
}

@Test func aColumnIsNeverReadFromItsShape() throws {
    // A column of numbers with no role does not stand at the row's end, and a
    // button in a cell with no role is not an action.
    let table = try decode(Navree.Table.self, """
        {"columns":[{"label":"Item","role":"title"},{"label":"Qty","numeric":true},{"label":"Note"}],
         "rows":[{"cells":[{"t":"text","value":"Phulkari dupatta"},{"t":"text","value":"2"},
                           {"t":"button","label":"Gift wrapped"}]}]}
        """)
    let row = RecordRules.row(columns: table.columns, record: table.rows[0])
    #expect(row.trailing == nil)
    #expect(row.subtitle == ["Qty 2", "Gift wrapped"])
    #expect(row.controls.isEmpty)
    #expect(row.action == nil)
}

@Test func aRecordsOwnActionsAndItsActionsColumnAreWhatItCanDo() throws {
    let table = try decode(Navree.Table.self, """
        {"columns":[{"label":"Item","role":"title"},{"label":"","role":"actions"},{"label":"Who","role":"leading"}],
         "rows":[{"cells":[{"t":"text","value":"Phulkari dupatta"},{"t":"button","label":"Archive"},{"t":"avatar","name":"Harleen Kaur"}],
                  "actions":[{"t":"button","label":"Delete","role":"destructive"}]}]}
        """)
    let row = RecordRules.row(columns: table.columns, record: table.rows[0])
    #expect(row.controls.count == 2)
    #expect(row.leading?.kind == "avatar")
}

@Test func theQueryFormIsReadByWhatEachFieldIsFor() throws {
    let orders = try firstTable(try answerScreen("store-orders"))
    let query = try #require(orders.query)
    let fields = QueryRules.fields(query)
    #expect(fields.search?.name == "search")
    #expect(!fields.filters.contains { $0.name == "limit" })
    #expect(fields.filters.map(\.name).contains("filter[status][in]"))
    // A choice of numbers is a filter when the server says so.
    let numbers = try decode(Navree.Form.self, """
        {"action":"/x","children":[{"t":"field","name":"floor","purpose":"filter",
          "input":{"t":"choice","options":[{"value":"1","label":"1"},{"value":"2","label":"2"}]}}]}
        """)
    #expect(QueryRules.fields(numbers).filters.map(\.name) == ["floor"])
}
