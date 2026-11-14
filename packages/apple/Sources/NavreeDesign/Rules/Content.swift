import NavreeProtocol

/// How a list of parts is set out.
public enum ListLayout: Sendable, Equatable {
    /// Rows from edge to edge: records, messages, a feed.
    case plain
    /// Rounded groups inset from the edges: settings, a record's facts, forms.
    case insetGrouped
}

/// How a table is drawn.
public enum TableLayout: Sendable, Equatable {
    /// Each record as a row of a list: its name, a line under it, what stands
    /// at its end, its state.
    case rows
    /// Columns with headings, as wide windows show them.
    case columns
}

/// The system control a field's input becomes.
public enum ControlKind: Sendable, Equatable {
    case textField
    case secureField
    case multilineField
    case toggle
    /// A pop-up menu of choices: the default picker in a form.
    case menuPicker
    /// A segmented control: a few short choices side by side.
    case segmented
    /// Choices as rows with a checkmark on the chosen one: iOS has no radio buttons.
    case inlinePicker
    /// A row that opens a list of the choices on a screen of its own.
    case navigationPicker
    /// Rows with checkmarks, any number chosen, on a screen of their own.
    case multipleChoice
    /// A compact date or time button that opens the system picker.
    case datePicker
    case slider
    case colorPicker
    case filePicker
    /// Nothing this client draws: the field is left out and the web view can
    /// show the page.
    case none
}

/// The keyboard and the autofill a text field asks for.
public struct Keyboard: Sendable, Equatable {
    public enum Layout: Sendable, Equatable {
        case standard, email, url, number, decimal, phone, oneTimeCode
    }

    public enum Content: Sendable, Equatable {
        case none, email, password, newPassword, phone, url, oneTimeCode, name, username
    }

    public var layout: Layout
    public var content: Content
    public var autocapitalizes: Bool
    public var autocorrects: Bool
}

/// A wide table's column width in points.
public struct ColumnWidth: Sendable, Hashable {
    public var min: Double
    public var ideal: Double
    /// None for a column that takes what room is left.
    public var max: Double?

    public init(min: Double, ideal: Double, max: Double?) {
        self.min = min
        self.ideal = ideal
        self.max = max
    }
}

public enum ContentRules {
    /// The width of the column a symbol beside a label stands in, where the
    /// system's lists give their symbols one column (a Mac's, as its
    /// sidebar does): every label then starts at one edge, whatever each
    /// symbol's own width. Grown with the person's text. iPhone and iPad
    /// lists already set a label's symbol in a column of their own (nil).
    public static func symbolColumn(bar: ActionBar) -> Double? {
        bar == .window ? 20 : nil
    }

    /// How a button of low prominence (a borderless button: "Saved replies")
    /// writes its words: on a Mac in the accent colour, as the Mac's own
    /// borderless buttons in content read as controls (Mail's "Show
    /// Details", Finder's "Show More"); the system's borderless button
    /// alone draws grey words there, which read as a label. iPhone and iPad
    /// draw a borderless button in the tint already.
    public enum LowProminenceWords: Sendable, Equatable {
        case accent
        case system
    }

    public static func lowProminenceWords(bar: ActionBar) -> LowProminenceWords {
        bar == .window ? .accent : .system
    }

    /// A screen that is mostly one list of records gets plain rows; anything
    /// else (a record's facts, sections, forms) gets inset groups.
    public static func list(_ body: Navree.Node) -> ListLayout {
        switch body {
        case .table, .list: return .plain
        case .region(let region) where region.children.count == 1:
            return list(region.children[0])
        case .stack(let stack):
            let parts = stack.children.filter { node in
                switch node {
                case .sheet, .dialog, .popover: false
                default: true
                }
            }
            if parts.count == 1 { return list(parts[0]) }
            return .insetGrouped
        default:
            return .insetGrouped
        }
    }

    /// Records as rows on a compact width; columns on a regular one when there
    /// are more than two to show. At an accessibility text size records are
    /// rows at every width, as Apple's apps give up columns there: seven
    /// columns of that text leave a heading two letters and a name a syllable
    /// a line.
    public static func table(_ table: Navree.Table, width: WidthClass, accessibilityText: Bool = false) -> TableLayout {
        guard width == .regular, !accessibilityText else { return .rows }
        return table.columns.count > 2 ? .columns : .rows
    }

    /// What a wide table's column holds, for its width.
    public enum ColumnKind: Sendable, Equatable {
        /// The record's name: the column that takes what room is left.
        case title
        /// A number or an amount, short and set to the trailing edge.
        case number
        /// A state in words beside a dot.
        case status
        /// The record's controls: as wide as its buttons, no wider.
        case controls
        /// Any other words.
        case words
    }

    public static func columnKind(_ column: Navree.Column, controls: Bool) -> ColumnKind {
        // The name stays the name, even when it is a button (an order's number opening its peek).
        if column.role == .title { return .title }
        if controls || column.role == .actions { return .controls }
        if column.numeric { return .number }
        if column.role == .status { return .status }
        return .words
    }

    /// A wide table's column width in points: its least, the width it asks
    /// for, and its most (none for the name, which takes what is left). Each
    /// column asks for room for its widest words (its heading's or a cell's,
    /// measured as drawn) and the cell's margins, within a range for what it
    /// holds, so a name is not wrapped to four lines beside half-empty columns.
    public static func columnWidth(_ kind: ColumnKind, widest: Double) -> ColumnWidth {
        let room = (widest + cellMargins).rounded(.up)
        func clamp(_ value: Double, _ low: Double, _ high: Double) -> Double { Swift.min(Swift.max(value, low), high) }
        switch kind {
        case .title: return ColumnWidth(min: 180, ideal: Swift.max(room, 240), max: nil)
        case .number: return ColumnWidth(min: 64, ideal: clamp(room, 64, 160), max: clamp(room, 64, 160) + 40)
        case .status: return ColumnWidth(min: 90, ideal: clamp(room, 90, 200), max: 260)
        case .controls: return ColumnWidth(min: room, ideal: room, max: room)
        case .words: return ColumnWidth(min: 80, ideal: clamp(room, 80, 260), max: 320)
        }
    }

    /// The columns a wide table shows across `available` points, by their
    /// places, with the width each is given: all of them where they fit;
    /// else, as Apple's apps give up columns as a window narrows, the
    /// secondary columns go first, from the last, then any other column of
    /// words, from the last (the name, the states, the numbers and the
    /// controls stay). Each column is as wide as it asks, and the name takes
    /// what is left, so the columns fill the table's width.
    public static func fittedColumns(
        _ columns: [(place: Int, kind: ColumnKind, secondary: Bool, width: ColumnWidth)], available: Double
    ) -> [(place: Int, width: Double)] {
        var shown = columns
        func asked() -> Double {
            shown.reduce(0) { $0 + $1.width.ideal } + Double(Swift.max(shown.count - 1, 0)) * columnGap + tableMargins
        }
        while asked() > available, let last = shown.lastIndex(where: { $0.secondary && $0.kind != .title }) {
            shown.remove(at: last)
        }
        while asked() > available, let last = shown.lastIndex(where: { $0.kind == .words }) {
            shown.remove(at: last)
        }
        let room = Double(Swift.max(shown.count - 1, 0)) * columnGap + tableMargins
        let others = shown.filter { $0.width.max != nil }.reduce(0) { $0 + $1.width.ideal } + room
        let names = shown.filter { $0.width.max == nil }
        let rest = names.isEmpty ? 0 : Swift.max(0, available - others) / Double(names.count)
        return shown.map { column in
            (column.place, column.width.max == nil ? Swift.max(column.width.min, rest) : column.width.ideal)
        }
    }

    /// The room the system's table keeps between two columns, as iPadOS 27.2
    /// lays one out (measured from the headings: 8 points).
    static let columnGap: Double = 8
    /// The room the system's table keeps at its two sides together, as iPadOS
    /// 27.2 lays one out (measured: 20 points each side).
    static let tableMargins: Double = 40

    /// The room a cell of the system's table keeps round its words: its
    /// leading and trailing margins, and a little air.
    static let cellMargins: Double = 24

    /// The columns a wide table shows, by their places: all but the subtitle
    /// column, whose words a narrow row puts under the name and which the
    /// title cell already shows beside it.
    public static func shownColumns(_ table: Navree.Table) -> [Int] {
        table.columns.indices.filter { table.columns[$0].role != .subtitle }
    }

    /// The columns a window too narrow for them gave up (see
    /// ``fittedColumns(_:available:)``), by their places, in the table's order.
    public static func givenUp(_ table: Navree.Table, fitted: [Int]) -> [Int] {
        shownColumns(table).filter { !fitted.contains($0) }
    }

    /// The line under a record's name that holds the words of the columns a
    /// narrow window gave up, so they are still there to read in the row, as
    /// Files writes a file's date and size under its name once its window is
    /// too narrow for their columns: each given-up cell's words, in the
    /// table's order, joined by " · "; `nil` when they hold none.
    public static func givenUpLine(_ cells: [Navree.Node], columns: [Int]) -> String? {
        let parts = columns.compactMap { index -> String? in
            guard index < cells.count else { return nil }
            let words = RecordRules.words(in: cells[index]).joined(separator: " ")
            return words.isEmpty ? nil : words
        }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    /// How a record of a wide table opens what it opens.
    public enum TableOpening: Sendable, Equatable {
        /// A tap on its first cell, its name (iPad: a table's rows are
        /// chosen only while it is edited).
        case firstCell
        /// A click anywhere on its row chooses the row and opens it, as a
        /// Mac's list does and Mail's messages do; the row stays chosen while
        /// what it opened is up.
        case choosing
    }

    public static func tableOpening(bar: ActionBar) -> TableOpening {
        bar == .window ? .choosing : .firstCell
    }


    /// The words a record's name cell shows in their place, where a click on
    /// its row opens the record: the cell's button or link to it (the web
    /// page's link, which the row's click now is) becomes its words, so it
    /// reads as the record's name in the label's colour and not as a second,
    /// dimmed control. `nil` where the cell's own control is the way in (a tap
    /// on its name), or where it holds none.
    public static func nameWords(_ cell: Navree.Node, opening: TableOpening, opens: Bool) -> [String]? {
        guard opening == .choosing, opens, RecordRules.firstAction(in: cell, form: nil) != nil else { return nil }
        let words = RecordRules.words(in: cell)
        return words.isEmpty ? nil : words
    }

    /// How a control in a wide table's cell is drawn.
    public enum CellControlLook: Sendable, Equatable {
        /// The words alone, in the tint: iPad, where a borderless button is
        /// the tint's words.
        case borderless
        /// The system's push button at its small size: a Mac, where a
        /// borderless button is grey words that read as no control at all,
        /// and a table row's own control is a small push button (as System
        /// Settings' and Mail's lists set theirs).
        case smallPushButton
    }

    public static func cellControlLook(bar: ActionBar) -> CellControlLook {
        bar == .window ? .smallPushButton : .borderless
    }

    /// The control a field's input becomes on iPhone and iPad.
    public static func control(_ input: Navree.Input) -> ControlKind {
        switch input {
        case .text(let text):
            switch text.kind {
            case .password: return .secureField
            case .multiline: return .multilineField
            default: return .textField
            }
        case .toggle: return .toggle  // a switch, whatever the web drew: iOS has no checkbox
        case .choice(let choice):
            switch choice.kind {
            case .segmented: return choice.options.count <= 5 ? .segmented : .menuPicker
            case .radio: return choice.options.count <= 6 ? .inlinePicker : .navigationPicker
            case .list: return .navigationPicker
            case .menu: return choice.options.count > 12 ? .navigationPicker : .menuPicker
            }
        case .choices: return .multipleChoice
        case .date: return .datePicker
        case .slider: return .slider
        case .colour: return .colorPicker
        case .file: return .filePicker
        // A range of two ends has no control drawn for it yet.
        case .range, .unknown: return .none
        }
    }

    /// The keyboard a text field brings up, and what the system may fill it with.
    public static func keyboard(_ input: Navree.TextInput) -> Keyboard {
        var keyboard = Keyboard(layout: .standard, content: .none, autocapitalizes: true, autocorrects: true)
        switch input.kind {
        case .email: keyboard = Keyboard(layout: .email, content: .email, autocapitalizes: false, autocorrects: false)
        case .password:
            let new = input.autofill == "new-password"
            keyboard = Keyboard(layout: .standard, content: new ? .newPassword : .password, autocapitalizes: false, autocorrects: false)
        case .number: keyboard.layout = .number
        case .decimal: keyboard.layout = .decimal
        case .phone: keyboard = Keyboard(layout: .phone, content: .phone, autocapitalizes: false, autocorrects: false)
        case .url: keyboard = Keyboard(layout: .url, content: .url, autocapitalizes: false, autocorrects: false)
        case .oneTimeCode: keyboard = Keyboard(layout: .oneTimeCode, content: .oneTimeCode, autocapitalizes: false, autocorrects: false)
        case .search: keyboard.autocorrects = false
        case .plain, .multiline: break
        }
        switch input.autofill {
        case "name"?: keyboard.content = .name
        case "username"?: keyboard.content = .username
        case "email"?: keyboard.content = .email
        case "tel"?: keyboard.content = .phone
        default: break
        }
        if keyboard.layout == .number || keyboard.layout == .decimal {
            keyboard.autocapitalizes = false
            keyboard.autocorrects = false
        }
        return keyboard
    }

    /// Whether a group of parts is a row of buttons and nothing else: on iOS it
    /// becomes rows of a grouped list, the destructive ones in a group of their
    /// own after the rest.
    public static func isButtonGroup(_ nodes: [Navree.Node]) -> Bool {
        nodes.count > 1 && nodes.allSatisfy { node in
            switch node {
            case .button, .menu, .link: true
            default: false
            }
        }
    }
}
