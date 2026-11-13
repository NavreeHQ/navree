import NavreeProtocol
import SwiftUI

/// A screen made of panes: on a compact width the current one alone, on a
/// regular width the list beside the rest; the inspector on request, from a
/// button in the bar (the system shows it as a sheet on a compact width).
struct PaneScreenBody: View {
    let panes: [Navree.Pane]
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var inspecting = false

    var body: some View {
        let layout = PaneRules.layout(panes, width: WidthClass(sizeClass))
        HStack(spacing: 0) {
            ForEach(Array(layout.shown.enumerated()), id: \.offset) { index, pane in
                if index > 0 { Divider().ignoresSafeArea() }
                PaneList(pane: pane)
                    .frame(maxWidth: Self.width(pane, of: layout.shown.count))
            }
        }
        .modifier(Inspector(pane: layout.inspector, shown: $inspecting))
    }

    /// The list stands at a sidebar's width beside its detail; anything else takes the room left.
    static func width(_ pane: Navree.Pane, of count: Int) -> CGFloat? {
        count > 1 && pane.role == .list ? 360 : .infinity
    }
}

/// One pane as a list of its parts.
struct PaneList: View {
    let pane: Navree.Pane

    var body: some View {
        let sections = SectionRules.sections(pane.children)
        if sections.count == 1, sections[0].rows.count == 1, case .part(.empty(let empty)) = sections[0].rows[0] {
            EmptyPart(empty: empty)
        } else {
            List {
                ForEach(sections) { section in
                    SectionView(section: section)
                }
            }
            .modifier(ListLook(layout: ContentRules.list(.stack(Navree.Stack(children: pane.children)))))
        }
    }
}

/// A pane drawn in place, outside a screen of panes: its parts.
struct PanePart: View {
    let pane: Navree.Pane

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(pane.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
        }
    }
}

/// The inspector pane, shown on request, with the button in the bar that asks for it.
struct Inspector: ViewModifier {
    let pane: Navree.Pane?
    @Binding var shown: Bool

    func body(content: Content) -> some View {
        if let pane {
            content
                .inspector(isPresented: $shown) {
                    PaneList(pane: pane)
                }
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        Button {
                            shown.toggle()
                        } label: {
                            Label(pane.title ?? "Details", systemImage: "info.circle")
                        }
                    }
                }
        } else {
            content
        }
    }
}
