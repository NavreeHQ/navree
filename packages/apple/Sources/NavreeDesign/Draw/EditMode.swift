import NavreeProtocol
import SwiftUI

/// The system's edit mode for a screen whose rows can be chosen or dragged:
/// Edit in the bar; while editing, the rows' boxes, the actions on the rows
/// chosen in the bottom bar, and every record that matches offered once all
/// the rows held are chosen, as Mail and Files offer them.
struct Editing: ViewModifier {
    let table: Navree.Table?
    let region: String?
    let tree: Navree.Node
    let reorders: Bool

    func body(content: Content) -> some View {
        #if os(iOS)
        EditingBars(content: content, table: table?.select == nil ? nil : table, region: region, tree: tree, reorders: reorders)
        #else
        content
        #endif
    }
}

extension EnvironmentValues {
    /// Whether a screen whose rows can be chosen or moved opens in edit mode:
    /// for a demo's launch and its pictures.
    @Entry public var navreeStartsEditing: Bool = false
}

#if os(iOS)
private struct EditingBars<Wrapped: View>: View {
    let content: Wrapped
    let table: Navree.Table?
    let region: String?
    let tree: Navree.Node
    let reorders: Bool
    @State private var mode: EditMode = .inactive
    @Environment(\.navree) private var context
    @Environment(\.navreeStartsEditing) private var startsEditing

    var body: some View {
        if table == nil && !reorders {
            content
        } else {
            content
                .environment(\.editMode, $mode)
                .toolbar {
                    ToolbarItem(placement: .topBarTrailing) {
                        EditButton().environment(\.editMode, $mode)
                    }
                    if mode.isEditing, let table {
                        ToolbarItemGroup(placement: .bottomBar) { chosenBar(table) }
                    }
                }
                .task { if startsEditing { mode = .active } }
        }
    }

    @ViewBuilder private func chosenBar(_ table: Navree.Table) -> some View {
        let key = table.id ?? region ?? ""
        let across = context.list(key).across
        if EditRules.offersAcross(table, taken: across), let all = table.select?.across {
            Button {
                context.send(.choose(table: key, row: nil, on: true, across: true))
            } label: {
                Text(all.total.map { "Choose All \($0.formatted())" } ?? "Choose All")
            }
        } else if across, let total = table.select?.across?.total {
            Text("All \(total.formatted()) Chosen").font(.subheadline).foregroundStyle(.secondary)
        }
        Spacer()
        if let bulk = EditRules.bulkActions(table, in: tree) {
            ForEach(Array(bulk.buttons.enumerated()), id: \.offset) { _, button in
                Button(role: buttonRole(button.role)) {
                    if let action = button.action {
                        context.send(.act(action, Place(form: FormRef(bulk.form), ask: PresentationRules.question(button))))
                    }
                } label: {
                    ButtonLabel(button: button, mode: .title)
                }
                .disabled(button.disabled || EditRules.chosen(table).isEmpty && !across)
            }
        }
    }
}
#endif
