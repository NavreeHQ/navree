import NavreeProtocol
import SwiftUI

extension EnvironmentValues {
    /// The popovers of the screen being drawn, by id: each is anchored to the
    /// control that opens it.
    @Entry var navreePopovers: [String: Navree.Popover] = [:]
}

/// A popover of the tree, anchored to the control that opens it: a popover
/// with its arrow on a regular width, and (the system's own adaptation) a
/// sheet at half height on a compact one. Its parts are a grouped list.
struct PopoverAnchor: ViewModifier {
    let action: Navree.Action?
    @Environment(\.navree) private var context
    @Environment(\.navreePopovers) private var popovers

    func body(content: Content) -> some View {
        if let id = action.flatMap(PopoverRules.opens), let popover = popovers[id] {
            content.popover(isPresented: Binding(
                get: { context.opened.contains(id) },
                set: { shown in if !shown { context.send(.dismissed(overlay: id)) } }
            )) {
                PopoverContent(popover: popover)
                    .presentationDetents([.medium, .large])
                    .presentationCompactAdaptation(.sheet)
            }
        } else {
            content
        }
    }
}

/// What a popover holds, as a grouped list: a small task keeps its buttons in
/// place, as Apple's own popovers do.
struct PopoverContent: View {
    let popover: Navree.Popover

    var body: some View {
        List {
            ForEach(SectionRules.sections(popover.children)) { section in
                SectionView(section: section)
            }
        }
        .modifier(ListLook(layout: .insetGrouped))
        .frame(minWidth: 320, idealWidth: 360, minHeight: 280, idealHeight: 380)
        .environment(\.navreeOverlay, popover.id)
    }
}
