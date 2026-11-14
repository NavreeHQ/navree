import NavreeProtocol
import SwiftUI
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// A table on a wide window: the system's `Table`, a column for each of the
/// server's, each as wide as what it holds asks, the record's own action on a
/// tap of its first cell. The next page is asked for as the last record comes
/// into view, and a spinner stands under the table only while it comes.
struct TableColumnsView: View {
    let table: Navree.Table
    let region: String?

    struct Record: Identifiable {
        let id: Int
        let cells: [Navree.Node]
        let action: Navree.Action?
    }

    struct Column: Identifiable, Hashable {
        let id: Int
        let label: String
        let width: ColumnWidth
    }

    @Environment(\.navree) private var context

    private var target: String { region ?? table.id ?? "" }

    var body: some View {
        let records = table.rows.enumerated().map { Record(id: $0.offset, cells: $0.element.cells, action: $0.element.action) }
        let columns = ContentRules.shownColumns(table).map { index in
            Column(id: index, label: table.columns[index].label, width: Self.width(of: index, in: table))
        }
        let last = records.last?.id
        VStack(spacing: 0) {
            Table(records) {
                TableColumnForEach(columns) { (column: Column) in
                    TableColumn(column.label) { (record: Record) in
                        Cell(record: record, index: column.id)
                            .onAppear {
                                // The last record in view: the next page, once for each address.
                                if record.id == last, column.id == columns.first?.id, let more = table.more,
                                    !context.list(target).loading, !context.list(target).failed
                                {
                                    context.send(.more(target: target, href: more))
                                }
                            }
                    }
                    .width(min: column.width.min, ideal: column.width.ideal, max: column.width.max ?? .infinity)
                }
            }
            if let more = table.more {
                let status = context.list(target)
                if status.failed {
                    RetryRow(title: "Couldn't Load More") { context.send(.more(target: target, href: more)) }
                } else if status.loading {
                    ProgressView()
                        .frame(maxWidth: .infinity)
                        .padding()
                }
            }
        }
    }

    /// A column's width from its widest words, its heading's or a cell's.
    static func width(of index: Int, in table: Navree.Table) -> ColumnWidth {
        let column = table.columns[index]
        let cells = table.rows.prefix(60).compactMap { index < $0.cells.count ? $0.cells[index] : nil }
        let controls = cells.contains { !RecordRules.controls(in: $0).isEmpty }
        let words = [column.label] + cells.compactMap { RecordRules.words(in: $0).first }
        return ContentRules.columnWidth(ContentRules.columnKind(column, controls: controls), widest: CellType.widest(words))
    }

    struct Cell: View {
        let record: Record
        let index: Int

        var body: some View {
            if index < record.cells.count {
                if index == 0 {
                    Opening(action: record.action, place: Place()) { NodeView(record.cells[index]) }
                } else if !RecordRules.controls(in: record.cells[index]).isEmpty {
                    CellControls(cell: record.cells[index], title: RecordRules.words(in: record.cells[0]).first ?? "")
                } else {
                    NodeView(record.cells[index])
                }
            }
        }
    }
}

/// The type a table's cells are written in, and how wide the widest of some
/// words is in it: measured, never guessed from how many letters they have.
enum CellType {
    static func widest(_ words: [String]) -> Double {
        #if canImport(UIKit)
        let font = UIFont.preferredFont(forTextStyle: .body)
        #else
        let font = NSFont.preferredFont(forTextStyle: .body)
        #endif
        let widths = words.map { ($0 as NSString).size(withAttributes: [.font: font]).width }
        return Double((widths.max() ?? 0).rounded(.up))
    }
}
