import NavreeProtocol

/// One row of a screen's list, as the design layer reads the tree.
public enum RowItem: Sendable, Equatable {
    /// A part drawn as it is, in a row of its own.
    case part(Navree.Node)
    /// A table's record, or a row of the tree, read as a row of a list.
    case record(RecordRow)
    /// A row of the tree.
    case row(Navree.Row)
    /// A button as a row of a grouped list.
    case button(Navree.Button, form: Navree.Form?)
    /// A label and its value.
    case fact(Navree.Fact)
    /// Two words side by side, the first naming the second (`Subtotal ₹7,848`).
    case labeled(label: String, value: String, muted: Bool)
    /// A field of a form.
    case field(Navree.Field, form: Navree.Form)
    /// One option of a field that takes any of several: a row with a checkmark.
    case option(Navree.Field, Navree.ChoiceOption, form: Navree.Form)
    /// A region that came empty: fetched when it shows.
    case pending(region: String)
    /// The end of a list or table, which fetches what comes after when it shows.
    case more(target: String, href: String)
}

/// A group of rows with what stands above and under it.
public struct ListSection: Sendable, Equatable, Identifiable {
    public var id: String
    public var header: String?
    public var subheader: String?
    public var footer: String?
    public var rows: [RowItem]
    /// The table the rows come from, when they are its records: what a wide
    /// window draws as columns and what search and filters act on.
    public var table: Navree.Table?
    /// The region the section stands in, when it is one: what its table's
    /// query and more answer into.
    public var region: String?
}

/// Reads a screen's body as the sections of a list, as iOS sets content out:
/// a section of the tree is a section, a table is its records, a row of
/// buttons is rows of buttons (the destructive ones in a section of their
/// own), facts are labelled values, a form is its fields. Sheets and dialogs
/// are left out: they are presented, not drawn in place.
public enum SectionRules {
    public static func sections(_ body: Navree.Node) -> [ListSection] {
        var builder = Builder()
        builder.read(body, region: nil)
        builder.flushLater()
        var sections = builder.sections.filter { !$0.rows.isEmpty || $0.header != nil }
        for index in sections.indices {
            sections[index].id = "s\(index)"
        }
        return sections
    }

    /// The sections of several parts read one after another.
    public static func sections(_ nodes: [Navree.Node]) -> [ListSection] {
        var builder = Builder()
        for node in nodes { builder.read(node, region: nil) }
        builder.flushLater()
        var sections = builder.sections.filter { !$0.rows.isEmpty || $0.header != nil }
        for index in sections.indices {
            sections[index].id = "s\(index)"
        }
        return sections
    }

    /// The sheets, dialogs and popovers anywhere in the tree.
    public static func overlays(_ body: Navree.Node) -> [Navree.Node] {
        var found: [Navree.Node] = []
        body.walk { node in
            switch node {
            case .sheet, .dialog, .popover: found.append(node)
            default: break
            }
        }
        return found
    }

    struct Builder {
        var sections: [ListSection] = []
        var current = ListSection(id: "", rows: [])
        var later: [ListSection] = []

        mutating func close() {
            if !current.rows.isEmpty || current.header != nil {
                sections.append(current)
            }
            current = ListSection(id: "", rows: [])
        }

        mutating func flushLater() {
            close()
            sections.append(contentsOf: later)
            later = []
        }

        mutating func row(_ item: RowItem) {
            current.rows.append(item)
        }

        mutating func read(_ node: Navree.Node, region: String?) {
            switch node {
            case .sheet, .dialog, .popover:
                return
            case .region(let part):
                if part.children.isEmpty {
                    row(.pending(region: part.id))
                } else {
                    for child in part.children { read(child, region: part.id) }
                }
            case .stack(let stack) where stack.hidden:
                return
            case .stack(let stack) where stack.axis == .vertical && stack.align == .center:
                // Parts set in the middle together (a ring and its words) stay one row.
                row(.part(node))
            case .stack(let stack) where stack.axis == .vertical:
                readVertical(stack.children, region: region)
            case .stack(let stack):
                readHorizontal(stack, region: region)
            case .section(let section):
                close()
                current.header = section.title
                current.subheader = section.subtitle
                for child in section.actions + section.children { read(child, region: region) }
                let footer = section.footer.flatMap(RecordRules.words).joined(separator: " ")
                current.footer = footer.isEmpty ? nil : footer
                close()
            case .table(let table):
                if !current.rows.isEmpty { close() }
                current.table = table
                current.region = region
                for record in table.rows {
                    row(.record(RecordRules.row(columns: table.columns, record: record)))
                }
                if table.rows.isEmpty, let empty = table.empty {
                    row(.part(empty))
                }
                if let more = table.more {
                    row(.more(target: region ?? table.id ?? "", href: more))
                }
                current.footer = current.footer ?? table.summary
                close()
            case .list(let list):
                if !current.rows.isEmpty { close() }
                for child in list.children { readListChild(child, region: region) }
                if let more = list.more {
                    row(.more(target: list.id ?? region ?? "", href: more))
                }
                close()
            case .row(let part):
                row(.row(part))
            case .facts(let facts):
                for fact in facts.items { row(.fact(fact)) }
            case .form(let form):
                readForm(form, region: region)
            case .divider, .spacer:
                // A list draws its own separators and room.
                return
            case .button(let button):
                if button.role == .destructive {
                    later.append(ListSection(id: "", rows: [.button(button, form: nil)]))
                } else {
                    row(.button(button, form: nil))
                }
            case .unknown(let part):
                if let alt = part.alt { read(alt, region: region) }
            case .toolbar, .feedRow, .message, .pane:
                // Kinds this client does not draw yet: no row stands empty for them.
                return
            default:
                row(.part(node))
            }
        }

        mutating func readListChild(_ node: Navree.Node, region: String?) {
            switch node {
            case .row(let part): row(.row(part))
            default: read(node, region: region)
            }
        }

        /// A vertical stack: its parts one after another. A row of buttons with
        /// a line of text after it reads as the buttons and the text as their
        /// footer.
        mutating func readVertical(_ children: [Navree.Node], region: String?) {
            var index = 0
            while index < children.count {
                let child = children[index]
                if case .stack(let inner) = child, inner.axis == .horizontal, let buttons = Self.buttons(in: inner) {
                    var note: String?
                    if index + 1 < children.count, case .text(let text) = children[index + 1] {
                        note = text.value
                        index += 1
                    }
                    readButtons(buttons, note: note)
                } else {
                    read(child, region: region)
                }
                index += 1
            }
        }

        /// The buttons of a horizontal stack that holds only buttons, however
        /// deep in horizontal stacks they are.
        static func buttons(in stack: Navree.Stack) -> [Navree.Button]? {
            var found: [Navree.Button] = []
            for child in stack.children {
                switch child {
                case .button(let button): found.append(button)
                case .stack(let inner) where inner.axis == .horizontal:
                    guard let more = buttons(in: inner) else { return nil }
                    found.append(contentsOf: more)
                default: return nil
                }
            }
            return found.isEmpty ? nil : found
        }

        mutating func readButtons(_ buttons: [Navree.Button], note: String?) {
            let plain = buttons.filter { $0.role != .destructive }
            let destructive = buttons.filter { $0.role == .destructive }
            if !plain.isEmpty {
                close()
                for button in plain { row(.button(button, form: nil)) }
                if destructive.isEmpty { current.footer = note }
                close()
            }
            if !destructive.isEmpty {
                close()
                for button in destructive { row(.button(button, form: nil)) }
                current.footer = note
                close()
            }
        }

        /// A horizontal stack: two words spread apart are a label and its
        /// value; one holding a link or a button and words is a row that
        /// leads somewhere; anything else is drawn as it is.
        mutating func readHorizontal(_ stack: Navree.Stack, region: String?) {
            if stack.justify == .between, stack.children.count == 2,
                case .text(let label) = stack.children[0], case .text(let value) = stack.children[1]
            {
                row(.labeled(label: label.value, value: value.value, muted: label.muted && value.muted))
                return
            }
            if let buttons = Self.buttons(in: stack) {
                readButtons(buttons, note: nil)
                return
            }
            if stack.justify == .between, let found = RecordRules.firstAction(in: .stack(stack), form: nil) {
                let words = RecordRules.words(in: .stack(stack))
                let badges = RecordRules.badges(in: .stack(stack))
                let texts = words.filter { word in !badges.contains { $0.text == word } }
                row(.record(RecordRow(
                    title: texts.first ?? "", subtitle: Array(texts.dropFirst().dropLast()),
                    trailing: texts.count > 2 ? texts.last : nil, trailingLabel: nil, statuses: badges,
                    action: found.0, actionForm: found.1, controls: []
                )))
                return
            }
            row(.part(.stack(stack)))
        }

        mutating func readForm(_ form: Navree.Form, region: String?) {
            var buttons: [Navree.Button] = []
            form.walkFields { node in
                switch node {
                case .field(let field):
                    if case .choices(let choices) = field.input {
                        close()
                        current.header = field.label ?? field.name
                        for option in choices.options { row(.option(field, option, form: form)) }
                        close()
                    } else {
                        row(.field(field, form: form))
                    }
                case .button(let button): buttons.append(button)
                default: row(.part(node))
                }
            }
            for button in buttons { row(.button(button, form: form)) }
        }
    }
}

extension Navree.Form {
    /// The form's fields and buttons, through the stacks that lay them out on
    /// the web, and any other part as it is.
    func walkFields(_ visit: (Navree.Node) -> Void) {
        func each(_ nodes: [Navree.Node]) {
            for node in nodes {
                switch node {
                case .stack(let stack) where !stack.hidden: each(stack.children)
                case .field, .button: visit(node)
                case .stack: break
                default: visit(node)
                }
            }
        }
        each(children)
    }
}
