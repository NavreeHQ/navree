import NavreeProtocol
import SwiftUI

/// Actions placed by ``PlacementRules``, as toolbar items.
struct PlacedActions: ToolbarContent {
    let actions: [Navree.Node]
    let depth: ScreenDepth
    let overlay: String?
    let form: FormRef?
    /// Destructive actions end the overflow menu, in red, where the screen
    /// has no section at its end for them.
    var destructiveInMenu = false

    var body: some ToolbarContent {
        let slots = PlacementRules.place(
            actions.map { ActionTraits($0) ?? ActionTraits(role: Navree.Role.none) }, depth: depth, bar: .current
        )
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
            if destructiveInMenu {
                ForEach(Array(placed.filter { $0.1 == .destructiveSection }.enumerated()), id: \.offset) { _, entry in
                    ToolbarAction(node: entry.0, slot: .overflow, overlay: overlay, form: form)
                }
            }
        }
        // A Mac window's toolbar: each action an item of its own, as its
        // symbol or its words, which the system moves into its overflow menu
        // when the window is too narrow for them.
        ToolbarItemGroup(placement: .automatic) {
            ForEach(Array(placed.filter { $0.1 == .window }.enumerated()), id: \.offset) { _, entry in
                ToolbarAction(node: entry.0, slot: .window, overlay: overlay, form: form)
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
    let node: Navree.Node
    let slot: Slot
    let overlay: String?
    let form: FormRef?
    @Environment(\.navree) private var context

    var body: some View {
        content
            .modifier(BarItemLook(slot))
            .modifier(PopoverAnchor(action: Self.action(of: node)))
    }

    static func action(of node: Navree.Node) -> Navree.Action? {
        if case .button(let button) = node { return button.action }
        return nil
    }

    @ViewBuilder private var content: some View {
        switch node {
        case .button(let button):
            let action = {
                if let act = button.action {
                    context.send(.act(act, Place(form: form, overlay: overlay, ask: PresentationRules.question(button))))
                }
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
            case .bar, .window:
                // Its symbol, or its words where it has none; why it can't be
                // used now, where the pointer rests on it.
                Button(role: buttonRole(button.role), action: action) { ButtonLabel(button: button, mode: .icon) }
                    .disabled(button.disabled || button.pending)
                    .help(ActionRules.reason(button) ?? "")
            default:
                Button(role: buttonRole(button.role), action: action) { ReasonedLabel(button: button) }
                    .disabled(button.disabled)
                    .help(ActionRules.reason(button) ?? "")
            }
        case .menu(let menu):
            Menu {
                MenuItems(items: menu.items)
            } label: {
                if let icon = menu.icon, let symbol = Symbols.symbol(for: icon) {
                    Label(menu.label, systemImage: symbol)
                } else {
                    Label(menu.label, systemImage: "ellipsis")
                }
            }
        case .link(let link):
            Button(link.label) { context.send(.act(link.action, Place(overlay: overlay))) }
        default:
            EmptyView()
        }
    }
}

/// A button's words and symbol, with why it can't be used now under them
/// when the server says why: what a menu item shows.
struct ReasonedLabel: View {
    let button: Navree.Button

    var body: some View {
        if let reason = ActionRules.reason(button) {
            let symbol = button.standard.flatMap(Symbols.standard) ?? button.icon.flatMap(Symbols.symbol(for:))
            if let symbol {
                Label {
                    Text(button.label)
                    Text(reason)
                } icon: {
                    Image(systemName: symbol)
                }
            } else {
                Text(button.label)
                Text(reason)
            }
        } else {
            ButtonLabel(button: button, mode: .both)
        }
    }
}

/// The toolbars that stand in a screen's body, as the bottom bar: each item
/// a symbol, in the order the guidelines give roles.
struct FootToolbar: ToolbarContent {
    let toolbars: [Navree.Toolbar]

    /// The bottom bar where a system has one; the window's toolbar on the Mac.
    static var placement: ToolbarItemPlacement {
        #if os(iOS)
        .bottomBar
        #else
        .automatic
        #endif
    }

    var body: some ToolbarContent {
        let items = toolbars.flatMap(\.items).map(ActionRules.node(for:))
        ToolbarItemGroup(placement: Self.placement) {
            ForEach(Array(items.enumerated()), id: \.offset) { index, node in
                if index > 0 { Spacer() }
                ToolbarAction(node: node, slot: .bar, overlay: nil, form: nil)
            }
        }
    }
}
