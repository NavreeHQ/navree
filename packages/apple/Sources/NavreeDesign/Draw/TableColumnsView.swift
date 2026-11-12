import NavreeProtocol
import SwiftUI

/// A table on a wide window: the system's `Table`, a column for each of the
/// server's, the record's own action on a tap of its first cell.
struct TableColumnsView: View {
    let table: Node.Table
    let region: String?

    struct Record: Identifiable {
        let id: Int
        let cells: [Node]
        let action: Action?
    }

    struct Column: Identifiable, Hashable {
        let id: Int
        let label: String
    }

    var body: some View {
        let records = table.rows.enumerated().map { Record(id: $0.offset, cells: $0.element.cells, action: $0.element.action) }
        let columns = table.columns.enumerated().map { Column(id: $0.offset, label: $0.element.label) }
        VStack(spacing: 0) {
            Table(records) {
                TableColumnForEach(columns) { (column: Column) in
                    TableColumn(column.label) { (record: Record) in
                        Cell(record: record, index: column.id)
                    }
                }
            }
            if let more = table.more {
                ProgressView()
                    .frame(maxWidth: .infinity)
                    .padding()
                    .task { context.send(.more(target: region ?? table.id ?? "", href: more)) }
            }
        }
    }

    @Environment(\.navree) private var context

    struct Cell: View {
        let record: Record
        let index: Int

        var body: some View {
            if index < record.cells.count {
                if index == 0 {
                    Opening(action: record.action, place: Place()) { NodeView(record.cells[index]) }
                } else {
                    NodeView(record.cells[index])
                }
            }
        }
    }
}
