import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// Rows chosen in the system's edit mode and rows dragged into a new order, as
// the core is told them.

private func corpusScreen(_ name: String) throws -> Navree.Screen {
    let url = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("crates/navree-native/corpus/\(name)")
    return try #require(try WireDecoder.decode(Navree.Envelope.self, from: Data(contentsOf: url)).screen)
}

@Test func onlyTheRowsThatCanBeChosenChangeAndEachChangeIsTold() throws {
    let table = try firstTable(try corpusScreen("screen-list.json"))
    #expect(EditRules.chosen(table) == ["1041"])
    // 1042 can't be chosen: ticking it says nothing.
    #expect(EditRules.choosable(table) == ["1041"])
    let changes = EditRules.changes(table, to: ["1042"])
    #expect(changes.map(\.row) == ["1041"])
    #expect(changes.map(\.on) == [false])
    #expect(RecordRules.row(columns: table.columns, record: table.rows[0]).choosable == false)
}

@Test func everyMatchingRecordIsOfferedOnceEveryRowHeldIsChosen() throws {
    let table = try firstTable(try corpusScreen("screen-list.json"))
    #expect(EditRules.offersAcross(table, taken: false))
    #expect(!EditRules.offersAcross(table, taken: true))
    var none = table
    none.rows = none.rows.map { var row = $0; row.selected = false; return row }
    #expect(!EditRules.offersAcross(none, taken: false))
}

@Test func aDraggedRowIsToldByTheRowItNowStandsBefore() {
    let ids = ["a", "b", "c", "d"]
    #expect(EditRules.move(ids: ids, from: [3], to: 1)! == ("d", "b"))
    #expect(EditRules.move(ids: ids, from: [0], to: 4)! == ("a", nil))
    #expect(EditRules.move(ids: ids, from: [1], to: 1) == nil)
    #expect(EditRules.move(ids: ids, from: [1], to: 2) == nil)
}

@Test func theBoardsColumnsAreListsThatCanBeReorderedOneSectionEach() throws {
    let board = try answerScreen("board-root")
    let sections = SectionRules.sections(board.body)
    let columns = sections.filter { $0.list?.reorder != nil }
    #expect(columns.map(\.header) == ["Backlog", "Ready", "In progress", "Review", "Done"])
    let backlog = try #require(columns.first?.list)
    #expect(EditRules.movable(backlog)?.first == "columns-item-1")
}
