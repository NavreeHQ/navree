import NavreeProtocol

/// A table's record, read as one row of a list: what a narrow window shows
/// instead of columns.
public struct RecordRow: Sendable, Equatable {
    /// What leads the row: a picture, an avatar, an icon.
    public var leading: Navree.Node?
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
    public var statuses: [Navree.Badge]
    /// What a tap on the row does, as the server says.
    public var action: Navree.Action?
    public var actionForm: Navree.Form?
    /// The record's actions and its inline edits (a form in a cell): they
    /// become the row's swipe actions and the items of its context menu.
    public var controls: [Navree.Node]

    public init(
        leading: Navree.Node? = nil, title: String, subtitle: [String], trailing: String? = nil,
        trailingLabel: String? = nil, statuses: [Navree.Badge] = [], action: Navree.Action? = nil,
        actionForm: Navree.Form? = nil, controls: [Navree.Node] = []
    ) {
        self.leading = leading
        self.title = title
        self.subtitle = subtitle
        self.trailing = trailing
        self.trailingLabel = trailingLabel
        self.statuses = statuses
        self.action = action
        self.actionForm = actionForm
        self.controls = controls
    }
}

public enum RecordRules {
    /// Reads a record of `columns` as a row, by what the server says each
    /// column is, never by what a cell looks like. The `title` column (else
    /// the first) names it; the `subtitle` column is the line under the name
    /// (in its place, the title cell's other words); `leading` leads it;
    /// `status` and any badge are its states; `trailing` stands at its end;
    /// `actions`, the record's own actions and a form in any cell (the
    /// record's inline edit) are what it can do. A column with no role adds
    /// its words to the line under the name, unless a narrow window may leave
    /// it out (`secondary`). A tap on the row does what the record says.
    public static func row(columns: [Navree.Column], record: Navree.TableRow) -> RecordRow {
        let cells = record.cells
        let titleIndex = columns.firstIndex { $0.role == .title } ?? 0
        let hasSubtitle = columns.contains { $0.role == .subtitle }
        var row = RecordRow(title: "", subtitle: [], action: record.action, controls: record.actions)
        var subtitle: [String] = []
        var others: [String] = []
        for (index, cell) in cells.enumerated() {
            let column = index < columns.count ? columns[index] : nil
            let role = index == titleIndex ? .title : column?.role
            if role == .actions || (role != .title && holdsForm(cell)) {
                row.controls.append(cell)
                continue
            }
            let texts = words(in: cell)
            switch role {
            case .title?:
                row.title = texts.first ?? ""
                if !hasSubtitle { subtitle.append(contentsOf: texts.dropFirst()) }
            case .subtitle?:
                subtitle.append(contentsOf: texts)
            case .leading?:
                row.leading = cell
            case .status?:
                let found = badges(in: cell)
                row.statuses.append(contentsOf: found.isEmpty ? texts.map { Navree.Badge(text: $0) } : found)
            case .trailing?:
                row.trailing = texts.first
                row.trailingLabel = column?.label
                others.append(contentsOf: texts.dropFirst())
            case .actions?:
                break
            case nil:
                if column?.secondary == true { continue }
                // A badge is a state wherever it stands.
                let found = badges(in: cell)
                row.statuses.append(contentsOf: found)
                let rest = texts.filter { text in !found.contains { $0.text == text } }
                if column?.numeric == true, let value = rest.first, let label = column?.label, !label.isEmpty {
                    // A number away from the row's end keeps its column's name: `Qty 1`.
                    others.append("\(label) \(value)")
                } else {
                    others.append(contentsOf: rest.prefix(1))
                }
            }
        }
        row.subtitle = subtitle + others
        return row
    }

    /// Whether a cell holds a form that posts: the protocol's word for the
    /// record's inline edit, which a client offers as an action that opens
    /// the form as a task.
    static func holdsForm(_ node: Navree.Node) -> Bool {
        node.first { part in
            if case .form(let form) = part { return form.method == .post }
            return false
        } != nil
    }

    /// The words a cell shows, one entry per text, a flag kept with its place.
    public static func words(in node: Navree.Node) -> [String] {
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

    static func badges(in node: Navree.Node) -> [Navree.Badge] {
        if case .badge(let badge) = node { return [badge] }
        return node.children.flatMap(badges)
    }

    /// The first button or link in `node`, and the form it stands in.
    static func firstAction(in node: Navree.Node, form: Navree.Form?) -> (Navree.Action, Navree.Form?)? {
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
