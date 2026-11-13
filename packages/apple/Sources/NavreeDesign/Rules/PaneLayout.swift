import NavreeProtocol

/// Which of a screen's panes stand side by side, and which is its inspector.
public struct PaneLayout: Sendable, Equatable {
    /// The panes drawn, from the leading edge.
    public var shown: [Navree.Pane]
    /// The pane shown on request beside the rest (a sheet on a compact width).
    public var inspector: Navree.Pane?
}

public enum PaneRules {
    /// The panes a screen's body is made of, through the splits and wrappers
    /// that hold them; none when it holds no pane.
    public static func panes(in body: Navree.Node) -> [Navree.Pane] {
        var found: [Navree.Pane] = []
        func visit(_ node: Navree.Node) {
            switch node {
            case .pane(let pane): found.append(pane)
            case .split(let split):
                visit(split.primary)
                visit(split.detail)
            case .stack(let stack) where !stack.hidden: stack.children.forEach(visit)
            case .region(let region): region.children.forEach(visit)
            default: break
            }
        }
        visit(body)
        return found
    }

    /// The order panes stand in across a wide window.
    static func rank(_ role: Navree.PaneRole) -> Int {
        switch role {
        case .list: 0
        case .content: 1
        case .detail: 2
        case .supporting: 3
        case .inspector: 4
        }
    }

    /// A compact width shows one pane: the one the server says is current,
    /// else the detail, else the first. A regular width shows the list beside
    /// its content and detail, as Mail does. The inspector stands apart in
    /// both, shown when the person asks.
    public static func layout(_ panes: [Navree.Pane], width: WidthClass) -> PaneLayout {
        let inspector = panes.first { $0.role == .inspector }
        let main = panes.filter { $0.role != .inspector }
        switch width {
        case .compact:
            let current = main.first(where: \.current) ?? main.first { $0.role == .detail } ?? main.first
            return PaneLayout(shown: current.map { [$0] } ?? [], inspector: inspector)
        case .regular:
            let ordered = main.enumerated().sorted { a, b in
                (rank(a.element.role), a.offset) < (rank(b.element.role), b.offset)
            }.map(\.element)
            return PaneLayout(shown: ordered, inspector: inspector)
        }
    }
}
