import NavreeProtocol
import SwiftUI

/// Actions placed by ``PlacementRules``, as toolbar items.
struct PlacedActions: ToolbarContent {
    let actions: [Node]
    let depth: ScreenDepth
    let overlay: String?
    let form: FormRef?

    var body: some ToolbarContent {
        let slots = PlacementRules.place(actions.map { ActionTraits($0) ?? ActionTraits(role: .plain) }, depth: depth)
        let placed = Array(zip(actions, slots))
        ToolbarItemGroup(placement: .cancellationAction) {
            ForEach(Array(placed.filter { $0.1 == .cancel }.enumerated()), id: \.offset) { _, entry in
                ToolbarAction(node: entry.0, slot: .cancel, overlay: overlay, form: form)
            }
        }
        ToolbarItemGroup(placement: .navigation) {
            ForEach(Array(placed.filter { $0.1 == .leading }.enumerated()), id: \.offset) { _, entry in
                ToolbarAction(node: entry.0, slot: .leading, overlay: overlay, form: form)
            }
        }
        ToolbarItemGroup(placement: .secondaryAction) {
            ForEach(Array(placed.filter { $0.1 == .overflow }.enumerated()), id: \.offset) { _, entry in
                ToolbarAction(node: entry.0, slot: .overflow, overlay: overlay, form: form)
            }
        }
        ToolbarItemGroup(placement: .primaryAction) {
            ForEach(Array(placed.filter { $0.1 == .create || $0.1 == .primary }.enumerated()), id: \.offset) { _, entry in
                ToolbarAction(node: entry.0, slot: entry.1, overlay: overlay, form: form)
            }
        }
        ToolbarItemGroup(placement: .confirmationAction) {
            ForEach(Array(placed.filter { $0.1 == .confirm }.enumerated()), id: \.offset) { _, entry in
                ToolbarAction(node: entry.0, slot: .confirm, overlay: overlay, form: form)
            }
        }
    }
}

/// One action in a toolbar: a symbol where the guidelines ask for one, the
/// system's roles for cancel and confirm, one prominent item at most.
struct ToolbarAction: View {
    let node: Node
    let slot: Slot
    let overlay: String?
    let form: FormRef?
    @Environment(\.navree) private var context

    var body: some View {
        switch node {
        case .button(let button):
            let action = {
                if let act = button.action { context.send(.act(act, Place(form: form, overlay: overlay))) }
            }
            switch slot {
            case .cancel:
                Button(role: .cancel, action: action) { ButtonLabel(button: button, mode: .icon) }
            case .confirm:
                Button(role: .confirm, action: action) { ButtonLabel(button: button, mode: .icon) }
                    .disabled(button.disabled || button.pending)
            case .primary:
                Button(action: action) { ButtonLabel(button: button, mode: .icon) }
                    .buttonStyle(.glassProminent)
                    .disabled(button.disabled || button.pending)
            case .create:
                Button(action: action) { ButtonLabel(button: button, mode: .icon) }
                    .disabled(button.disabled)
            default:
                Button(role: buttonRole(button.role), action: action) { ButtonLabel(button: button, mode: .both) }
                    .disabled(button.disabled)
            }
        case .menu(let menu):
            Menu {
                MenuItems(items: menu.items)
            } label: {
                Label(menu.label, systemImage: "ellipsis")
            }
        case .link(let link):
            Button(link.label) { context.send(.act(link.action, Place(overlay: overlay))) }
        default:
            EmptyView()
        }
    }
}
