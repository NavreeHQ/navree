// The closed sets of names the protocol uses. Each reads a name it does not
// know as its fallback, so a newer server never breaks an older app.

/// What a control means, wherever a system puts it.
public enum Role: String, Sendable, Hashable, Decodable, CaseIterable {
    /// No role of its own (`none` on the wire).
    case plain = "none"
    /// The one thing this screen or form is for.
    case primary
    /// Something else a reader may do here.
    case secondary
    /// It deletes or can't be undone.
    case destructive
    /// It leaves without doing anything.
    case cancel
    /// It agrees to what a dialog asks.
    case confirm
    /// It starts a new thing.
    case create
    /// It goes somewhere.
    case navigation

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .plain)
    }
}

/// What a colour means.
public enum Tone: String, Sendable, Hashable, Decodable, CaseIterable {
    case neutral, accent, success, warning, danger, info

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .neutral)
    }
}

/// How loud a control is among its neighbours.
public enum ButtonProminence: String, Sendable, Hashable, Decodable {
    case high, medium, low

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .medium)
    }
}

/// The direction a stack runs.
public enum StackAxis: String, Sendable, Hashable, Decodable {
    case vertical, horizontal

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .vertical)
    }
}

/// Where parts sit across a stack's axis, or text in a column.
public enum Align: String, Sendable, Hashable, Decodable {
    case start, center, end, stretch, baseline

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .start)
    }
}

/// How a stack shares the room left along its axis.
public enum Justify: String, Sendable, Hashable, Decodable {
    case start, center, end, between

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .start)
    }
}

/// How a section is set off from what is around it.
public enum SectionStyle: String, Sendable, Hashable, Decodable {
    case plain, card, inset

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .plain)
    }
}

/// Where a row stands in a sequence.
public enum RowState: String, Sendable, Hashable, Decodable {
    case done, current, upcoming

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .upcoming)
    }
}

/// The style of a piece of text, by what it is for.
public enum TextStyle: String, Sendable, Hashable, Decodable, CaseIterable {
    case largeTitle = "large_title"
    case title, headline, body, callout, caption, mono

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .body)
    }
}

/// How a picture fills its box.
public enum Fit: String, Sendable, Hashable, Decodable {
    case contain, cover

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .contain)
    }
}

/// What a column is to its row.
public enum ColumnRole: String, Sendable, Hashable, Decodable {
    case title, subtitle, status, trailing, actions
    /// A role added after this client was built.
    case unrecognized

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .unrecognized)
    }
}

/// The way a column is sorted now.
public enum SortDirection: String, Sendable, Hashable, Decodable {
    case asc, desc

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .asc)
    }
}

/// The kind of a chart.
public enum ChartKind: String, Sendable, Hashable, Decodable {
    case line, area, bar, donut, sparkline, funnel
    /// A kind added after this client was built: drawn as its numbers.
    case unrecognized

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .unrecognized)
    }
}

/// How a form is sent.
public enum Method: String, Sendable, Hashable, Decodable {
    case get, post

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .get)
    }
}

/// How much of the screen a sheet takes.
public enum SheetSize: String, Sendable, Hashable, Decodable {
    case medium, large, full

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .large)
    }
}

/// How a screen a link leads to is presented.
public enum Present: String, Sendable, Hashable, Decodable {
    case push, replace, root, sheet, full, external

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .push)
    }
}

/// How a change of address enters the client's history.
public enum URLMode: String, Sendable, Hashable, Decodable {
    case push, replace

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .push)
    }
}

/// A state a command sets on a node.
public enum NodeState: String, Sendable, Hashable, Decodable {
    case open, hidden, checked, selected, disabled, active, invalid, pending
    /// A state added after this client was built: the command is passed over.
    case unrecognized

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .unrecognized)
    }
}

/// What typed text is.
public enum TextKind: String, Sendable, Hashable, Decodable, CaseIterable {
    case plain, email, password, number, decimal, phone, url, search, multiline
    case oneTimeCode = "one_time_code"

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .plain)
    }
}

/// How an on-or-off control is drawn on the web.
public enum ToggleKind: String, Sendable, Hashable, Decodable {
    case `switch`, checkbox

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .switch)
    }
}

/// How a choice of one is drawn on the web.
public enum ChoiceKind: String, Sendable, Hashable, Decodable {
    case menu, segmented, radio, list

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .menu)
    }
}

/// What a date control picks.
public enum DateKind: String, Sendable, Hashable, Decodable {
    case date, time, datetime

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .date)
    }
}

/// An action every app has, which each system draws with its own symbol and
/// words, whatever icon the app gave it.
public enum StandardAction: String, Sendable, Hashable, Decodable, CaseIterable {
    case share, delete, add, edit, search, done, cancel, more
    /// One added after this client was built: the button's own icon is used.
    case unrecognized

    public init(from decoder: any Decoder) throws {
        self = try decoder.closedSet(or: .unrecognized)
    }
}
