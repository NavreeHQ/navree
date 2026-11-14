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
    /// Whether a sheet or dialog of the screen is up: what a chosen row
    /// opened, while it stays chosen.
    var presenting = false

    struct Record: Identifiable {
        let id: Int
        let cells: [Navree.Node]
        let action: Navree.Action?
    }

    struct Column: Identifiable, Hashable {
        let id: Int
        let label: String
        let width: Double
        /// The name's column, which takes what the others leave.
        let flexible: Bool
    }

    @Environment(\.navree) private var context
    /// The table's own width, as it was laid out.
    @State private var available: Double?
    /// The record chosen by a click, on a Mac.
    @State private var chosen: Record.ID?

    private var target: String { region ?? table.id ?? "" }

    var body: some View {
        let records = table.rows.enumerated().map { Record(id: $0.offset, cells: $0.element.cells, action: $0.element.action) }
        let columns = Self.columns(of: table, available: available)
        let last = records.last?.id
        VStack(spacing: 0) {
            Group {
                if let selection = choosing(records) {
                    Table(records, selection: selection) { tableColumns(columns, last: last) }
                } else {
                    Table(records) { tableColumns(columns, last: last) }
                }
            }
            .onGeometryChange(for: Double.self) { Double($0.size.width) } action: { available = $0 }
            // What the chosen row opened is put away: the row is chosen no more,
            // and a click opens it again.
            .onChange(of: presenting) { _, up in if !up { chosen = nil } }
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

    private var opening: ContentRules.TableOpening { ContentRules.tableOpening(bar: .current) }

    /// A column for each shown, each cell drawing its record's part.
    @TableColumnBuilder<Record, Never>
    private func tableColumns(_ columns: [Column], last: Record.ID?) -> some TableColumnContent<Record, Never> {
        TableColumnForEach(columns) { (column: Column) in
            TableColumn(column.label) { (record: Record) in
                Cell(record: record, index: column.id, opening: opening)
                    .onAppear {
                        // The last record in view: the next page, once for each address.
                        if record.id == last, column.id == columns.first?.id, let more = table.more,
                            !context.list(target).loading, !context.list(target).failed
                        {
                            context.send(.more(target: target, href: more))
                        }
                    }
            }
            // As wide as it asks, no more and no less (the table gives
            // no heed to an ideal width alone); the name, from its
            // least, takes what the others leave.
            .width(min: column.width, max: column.flexible ? .infinity : column.width)
        }
    }

    /// Where a click chooses the row and opens it: the row chosen, and its
    /// record's action asked for as it is chosen. Elsewhere none.
    private func choosing(_ records: [Record]) -> Binding<Record.ID?>? {
        guard opening == .choosing else { return nil }
        return Binding(
            get: { chosen },
            set: { next in
                chosen = next
                if let next, let action = records.first(where: { $0.id == next })?.action {
                    context.send(.act(action, Place()))
                }
            }
        )
    }

    /// The columns shown, each as wide as its widest words ask (its heading's
    /// or a cell's), as many as the table's width holds.
    static func columns(of table: Navree.Table, available: Double?) -> [Column] {
        let asked = ContentRules.shownColumns(table).map { index in
            let column = table.columns[index]
            let cells = table.rows.prefix(60).compactMap { index < $0.cells.count ? $0.cells[index] : nil }
            let controls = cells.contains { !RecordRules.controls(in: $0).isEmpty }
            let words = [column.label] + cells.flatMap { RecordRules.words(in: $0) }
            let kind = ContentRules.columnKind(column, controls: controls)
            return (place: index, kind: kind, secondary: column.secondary, width: ContentRules.columnWidth(kind, widest: CellType.widest(words)))
        }
        // Until the table is laid out, every column as wide as it asks.
        let fitted = available.map { ContentRules.fittedColumns(asked, available: $0) } ?? asked.map { ($0.place, $0.width.ideal) }
        return fitted.map { place, width in
            let asks = asked.first { $0.place == place }?.width
            let flexible = asks?.max == nil
            return Column(id: place, label: table.columns[place].label, width: flexible ? asks?.min ?? width : width, flexible: flexible)
        }
    }

    struct Cell: View {
        let record: Record
        let index: Int
        var opening: ContentRules.TableOpening = .firstCell

        var body: some View {
            if index < record.cells.count {
                if index == 0, opening == .choosing, record.action != nil {
                    // The row's click opens it: the name's own button (the web
                    // page's link to the same) takes no click of its own.
                    NodeView(record.cells[index]).allowsHitTesting(false)
                } else if index == 0 {
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
