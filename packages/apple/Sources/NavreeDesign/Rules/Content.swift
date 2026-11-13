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

public enum ContentRules {
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
    /// are more than two to show.
    public static func table(_ table: Navree.Table, width: WidthClass) -> TableLayout {
        guard width == .regular else { return .rows }
        return table.columns.count > 2 ? .columns : .rows
    }

    /// The columns a wide table shows, by their places: all but the subtitle
    /// column, whose words a narrow row puts under the name and which the
    /// title cell already shows beside it.
    public static func shownColumns(_ table: Navree.Table) -> [Int] {
        table.columns.indices.filter { table.columns[$0].role != .subtitle }
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
