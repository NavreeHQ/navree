import NavreeProtocol

/// A table's record, read as one row of a list: what a narrow window shows
/// instead of columns.
public struct RecordRow: Sendable, Equatable {
    /// The record's name: the title column's first words.
    public var title: String
    /// Lines of words under the name, from the title cell and from the other
    /// columns a narrow window keeps.
    public var subtitle: [String]
    /// What stands at the row's end: an amount, a count.
    public var trailing: String?
    /// The column the trailing value comes from, for assistive technology.
    public var trailingLabel: String?
    /// Its states, as the table's badges.
    public var statuses: [Node.Badge]
    /// What a tap on the row does, and the form it sends when it is a submit.
    public var action: Action?
    public var actionForm: Node.Form?
    /// Forms and buttons in the record's cells: they become the row's swipe
    /// and context menu actions.
    public var controls: [Node]
}

public enum RecordRules {
    /// Reads a record of `columns` as a row. The title column (by role, or the
    /// first) names it; a column with the trailing role, or the first column of
    /// numbers, stands at the end; badges are its states; forms and buttons in
    /// the other cells are its actions; the words of the other columns a narrow
    /// window keeps go under the name. Columns marked secondary are left out.
    public static func row(columns: [Node.Column], record: Node.TableRow) -> RecordRow {
        let cells = record.cells
        let titleIndex = columns.firstIndex { $0.role == .title } ?? 0
        let trailingIndex = columns.firstIndex { $0.role == .trailing }
            ?? columns.indices.last { $0 != titleIndex && columns[$0].numeric && !columns[$0].secondary }

        var row = RecordRow(
            title: "", subtitle: [], trailing: nil, trailingLabel: nil, statuses: [], action: record.action,
            actionForm: nil, controls: []
        )
        if titleIndex < cells.count {
            let texts = words(in: cells[titleIndex])
            row.title = texts.first ?? ""
            row.subtitle.append(contentsOf: texts.dropFirst())
            if row.action == nil, let found = firstAction(in: cells[titleIndex], form: nil) {
                row.action = found.0
                row.actionForm = found.1
            }
        }
        for (index, cell) in cells.enumerated() where index != titleIndex {
            let column = index < columns.count ? columns[index] : nil
            if column?.secondary == true && column?.role == nil { continue }
            if column?.role == .actions || holdsControls(cell) {
                row.controls.append(cell)
                continue
            }
            let badges = badges(in: cell)
            if column?.role == .status || (!badges.isEmpty && words(in: cell).count == badges.count) {
                row.statuses.append(contentsOf: badges)
                continue
            }
            let texts = words(in: cell)
            if index == trailingIndex {
                row.trailing = texts.first
                row.trailingLabel = column?.label
                row.subtitle.append(contentsOf: texts.dropFirst())
            } else if column?.numeric == true, let value = texts.first, let label = column?.label, !label.isEmpty {
                // A number away from the row's end keeps its column's name: `Qty 1`.
                row.subtitle.append("\(label) \(value)")
            } else {
                row.subtitle.append(contentsOf: texts.prefix(1))
            }
        }
        return row
    }

    /// The words a cell shows, one entry per text, a flag kept with its place.
    public static func words(in node: Node) -> [String] {
        switch node {
        case .text(let text): return text.value.isEmpty ? [] : [text.value]
        case .badge(let badge): return [badge.text]
        case .button(let button): return [button.label]
        case .link(let link): return link.children.isEmpty ? [link.label] : link.children.flatMap(words)
        case .stack(let stack) where stack.axis == .horizontal:
            let parts = stack.children.flatMap(words)
            if parts.count == 2, parts[0].unicodeScalars.allSatisfy({ (0x1F1E6...0x1F1FF).contains($0.value) }) {
                return [parts.joined(separator: " ")]
            }
            return parts
        default:
            return node.children.flatMap(words)
        }
    }

    static func badges(in node: Node) -> [Node.Badge] {
        if case .badge(let badge) = node { return [badge] }
        return node.children.flatMap(badges)
    }

    /// Whether a cell holds something to do rather than something to read: a
    /// form that posts, or a button.
    static func holdsControls(_ node: Node) -> Bool {
        node.first { part in
            switch part {
            case .form(let form): form.method == .post
            case .button, .menu: true
            default: false
            }
        } != nil
    }

    /// The first button or link in `node`, and the form it stands in.
    static func firstAction(in node: Node, form: Node.Form?) -> (Action, Node.Form?)? {
        switch node {
        case .button(let button):
            return button.action.map { ($0, form) }
        case .link(let link):
            return (link.action, form)
        case .form(let inner):
            for child in inner.children {
                if let found = firstAction(in: child, form: inner) { return found }
            }
            return nil
        default:
            for child in node.children {
                if let found = firstAction(in: child, form: form) { return found }
            }
            return nil
        }
    }
}
