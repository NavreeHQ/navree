import NavreeProtocol
import SwiftUI

/// The SwiftUI role of a button's role.
func buttonRole(_ role: Role) -> ButtonRole? {
    switch role {
    case .destructive: .destructive
    case .cancel: .cancel
    default: nil
    }
}

/// How a button's label shows.
enum LabelMode {
    /// As the page drew it: the icon alone, or the words.
    case automatic
    /// The words only.
    case title
    /// The symbol only; the words stay for VoiceOver.
    case icon
    /// Both, as menus show them.
    case both
}

/// A button's label: the system's symbol for a standard action or the
/// symbol of its icon, and its words.
struct ButtonLabel: View {
    let button: Node.Button
    var mode: LabelMode = .automatic
    @Environment(\.navree) private var context

    private var showIcon: Bool {
        switch mode {
        case .automatic: button.iconOnly
        case .title: false
        case .icon, .both: true
        }
    }

    private var showTitle: Bool {
        switch mode {
        case .automatic: !button.iconOnly
        case .title, .both: true
        case .icon: false
        }
    }

    private var symbol: String? {
        button.standard.flatMap(Symbols.standard) ?? button.icon.flatMap(Symbols.symbol(for:)) ?? Symbols.forRole(button.role)
    }

    var body: some View {
        if showIcon, let symbol {
            if showTitle {
                Label(button.label, systemImage: symbol).labelStyle(.titleAndIcon)
            } else {
                Label(button.label, systemImage: symbol).labelStyle(.iconOnly)
            }
        } else if showIcon, let icon = button.icon {
            if showTitle {
                Label { Text(button.label) } icon: { Glyph(icon: icon, icons: context.icons) }.labelStyle(.titleAndIcon)
            } else {
                Label { Text(button.label) } icon: { Glyph(icon: icon, icons: context.icons) }.labelStyle(.iconOnly)
            }
        } else {
            Text(button.label)
        }
    }
}

/// A button in the content: bordered by how loud it is, with a spinner in it
/// while it waits.
struct ButtonPart: View {
    let button: Node.Button
    var prominent = false
    @Environment(\.navree) private var context
    @Environment(\.navreeForm) private var form
    @Environment(\.navreeOverlay) private var overlay

    var body: some View {
        let control = Button(role: buttonRole(button.role)) {
            if let action = button.action {
                context.send(.act(action, Place(form: form, overlay: overlay)))
            }
        } label: {
            if button.pending {
                HStack(spacing: 6) {
                    ProgressView()
                    ButtonLabel(button: button, mode: .title)
                }
            } else {
                ButtonLabel(button: button)
            }
        }
        .disabled(button.disabled || button.pending)
        Group {
            if prominent || (button.prominence == .high && button.role != .destructive) {
                control.buttonStyle(.borderedProminent)
            } else if button.prominence == .low {
                control.buttonStyle(.borderless)
            } else {
                control.buttonStyle(.bordered)
            }
        }
    }
}

/// A link: words that lead somewhere, in the app's tint.
struct LinkPart: View {
    let link: Node.Link
    @Environment(\.navree) private var context
    @Environment(\.navreeOverlay) private var overlay

    var body: some View {
        Button {
            context.send(.act(link.action, Place(overlay: overlay)))
        } label: {
            if link.children.isEmpty {
                Text(link.label)
            } else {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(link.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
                }
                .accessibilityElement(children: .combine)
                .accessibilityLabel(link.label)
            }
        }
        .buttonStyle(.borderless)
    }
}

/// A pull-down menu of actions.
struct MenuPart: View {
    let menu: Node.Menu
    @Environment(\.navree) private var context

    var body: some View {
        Menu {
            MenuItems(items: menu.items)
        } label: {
            if let icon = menu.icon {
                Label { Text(menu.label) } icon: { Glyph(icon: icon, icons: context.icons) }
            } else {
                Text(menu.label)
            }
        }
    }
}

/// The items of a menu or a context menu: buttons, links, groups and dividers,
/// the destructive ones last, as the guidelines ask.
struct MenuItems: View {
    let items: [Node]
    @Environment(\.navree) private var context
    @Environment(\.navreeOverlay) private var overlay

    var body: some View {
        let plain = items.filter { !isDestructive($0) }
        let destructive = items.filter(isDestructive)
        ForEach(Array(plain.enumerated()), id: \.offset) { _, item in entry(item) }
        if !destructive.isEmpty {
            Divider()
            ForEach(Array(destructive.enumerated()), id: \.offset) { _, item in entry(item) }
        }
    }

    private func isDestructive(_ node: Node) -> Bool {
        if case .button(let button) = node { return button.role == .destructive }
        return false
    }

    @ViewBuilder private func entry(_ item: Node) -> some View {
        switch item {
        case .button(let button):
            Button(role: buttonRole(button.role)) {
                if let action = button.action { context.send(.act(action, Place(overlay: overlay))) }
            } label: {
                ButtonLabel(button: button, mode: .both)
            }
            .disabled(button.disabled)
        case .link(let link):
            Button(link.label) { context.send(.act(link.action, Place(overlay: overlay))) }
        case .section(let section):
            Section(section.title ?? "") { MenuItems(items: section.children) }
        case .divider:
            Divider()
        case .menu(let inner):
            Menu(inner.label) { MenuItems(items: inner.items) }
        default:
            EmptyView()
        }
    }
}
