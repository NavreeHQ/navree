import NavreeProtocol

/// How a screen written for native clients is set out: the rules of the
/// composition kinds (`card`, `layers`, `shape`, `carousel`, `adaptive`, and
/// the newer words of `stack` and `grid`) that the health screens need.
public enum CompositionRules {
    /// Whether a screen is composed rather than a list of sections: it holds
    /// a card or one of the other composition kinds, or a stack that turns,
    /// grows or pads. Such a screen scrolls on the grouped ground with its
    /// cards as surfaces, as the Health app's summary does; any other screen
    /// is the system's grouped list.
    public static func isComposed(_ body: Navree.Node) -> Bool {
        body.first { node in
            switch node {
            case .card, .layers, .shape, .carousel, .adaptive: true
            case .stack(let stack): stack.wide != nil || stack.grow || stack.pad > 0
            default: false
            }
        } != nil
    }

    /// The axis a stack is set along: its `wide` axis in a wide room (a
    /// regular width), its own axis otherwise.
    public static func axis(_ stack: Navree.Stack, width: WidthClass) -> Navree.Axis {
        width == .regular ? stack.wide ?? stack.axis : stack.axis
    }

    /// Which of an adaptive part's rooms is drawn.
    public static func room(_ adaptive: Navree.Adaptive, width: WidthClass) -> [Navree.Node] {
        width == .regular ? adaptive.wide : adaptive.compact
    }

    /// How a grid's columns are made: a fixed count when it gives one (the
    /// five pillars are always one row of five), else as many as fit at its
    /// least width.
    public enum Columns: Equatable, Sendable {
        case fixed(Int)
        case fitting(minimum: Double)
    }

    public static func columns(_ grid: Navree.Grid) -> Columns {
        if let cols = grid.cols, cols > 0 { return .fixed(cols) }
        return .fitting(minimum: Double(grid.min ?? 160))
    }

    /// A section that holds exactly one card is a group: its heading stands
    /// on the page's ground as a section's header does, and the card under it
    /// is the surface. No second surface is drawn for the section.
    public static func isGroup(_ section: Navree.Section) -> Bool {
        guard section.children.count == 1, case .card = section.children[0] else { return false }
        return true
    }

    /// Points from a number of the system's spacing steps.
    public static func points(steps: Int) -> Double {
        Double(steps) * 4
    }

    /// The padding inside a card: its own, or the system's 16 points.
    public static func padding(_ card: Navree.Card) -> Double {
        card.pad.map { points(steps: $0) } ?? 16
    }
}
