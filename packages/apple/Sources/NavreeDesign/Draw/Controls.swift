import NavreeProtocol
import SwiftUI

/// The SwiftUI role of a button's role.
func buttonRole(_ role: Navree.Role) -> ButtonRole? {
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
    let button: Navree.Button
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
    let button: Navree.Button
    var prominent = false
    @Environment(\.navree) private var context
    @Environment(\.navreeForm) private var form
    @Environment(\.navreeOverlay) private var overlay
    @Environment(\.navreeRowSpeech) private var row

    var body: some View {
        let control = Button(role: buttonRole(button.role)) {
            if let action = button.action {
                context.send(.act(action, Place(form: form, overlay: overlay, ask: PresentationRules.question(button))))
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
        .modifier(PopoverAnchor(action: button.action))
        VStack(alignment: .leading, spacing: 4) {
            Group {
                if prominent || (button.prominence == .high && button.role != .destructive) {
                    control.buttonStyle(.borderedProminent)
                } else if button.prominence == .low {
                    control.buttonStyle(.borderless)
                } else {
                    control.buttonStyle(.bordered)
                }
            }
            .modifier(FingerTarget())
            if let reason = ActionRules.reason(button) {
                Text(reason).font(.footnote).foregroundStyle(.secondary)
            }
        }
        // A button that says its row's title again is the row's own action.
        .accessibilityHidden(row.map { SpeechRules.repeatsTitle(button, of: $0.title) } ?? false)
    }
}

/// A button in the content as large as a finger needs on iPhone and iPad:
/// the system's large size, whose capsule is at least the 44 points Apple
/// asks of every target (the regular one is 34.5 points tall). A Mac's
/// pointer keeps the regular size.
struct FingerTarget: ViewModifier {
    func body(content: Content) -> some View {
        #if os(iOS)
        content.controlSize(.large)
        #else
        content
        #endif
    }
}

/// A link: words that lead somewhere, in the app's tint.
struct LinkPart: View {
    let link: Navree.Link
    @Environment(\.navree) private var context
    @Environment(\.navreeOverlay) private var overlay
    @Environment(\.navreeInControl) private var inControl

    var body: some View {
        if link.children.isEmpty {
            Button {
                context.send(.act(link.action, Place(overlay: overlay, from: link.id)))
            } label: {
                Text(link.label)
            }
            .buttonStyle(.borderless)
        } else if inControl {
            drawn
        } else {
            // What is drawn is the children, as one target with the system's
            // pressed state; the link's label is its name for VoiceOver.
            Button {
                context.send(.act(link.action, Place(overlay: overlay, from: link.id)))
            } label: {
                drawn.contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .environment(\.navreeInControl, true)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(link.label)
            .accessibilityAddTraits(.isButton)
        }
    }

    private var drawn: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(Array(link.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
        }
        .frame(maxWidth: .infinity)
    }
}

/// A pull-down menu of actions in the content. Its words always show beside
/// its symbol: a menu in a row of controls often names what it sets ("Open",
/// "High"), as Reminders writes a reminder's priority and Calendar an event's
/// availability, and a symbol alone would leave the person to guess.
struct MenuPart: View {
    let menu: Navree.Menu
    @Environment(\.navree) private var context

    var body: some View {
        Menu {
            MenuItems(items: menu.items)
        } label: {
            switch (ContentRules.menuLabel(menu), menu.icon) {
            case (.titleAndIcon, let icon?):
                label(icon).labelStyle(.titleAndIcon)
            case (.icon, let icon?):
                label(icon).labelStyle(.iconOnly)
            default:
                Text(menu.label)
            }
        }
        .fixedSize()
        .accessibilityLabel(Text(menu.label))
    }

    @ViewBuilder private func label(_ icon: Navree.Icon) -> some View {
        if let symbol = Symbols.symbol(for: icon) {
            Label(menu.label, systemImage: symbol)
        } else {
            Label { Text(menu.label) } icon: { Glyph(icon: icon, icons: context.icons) }
        }
    }
}

/// The items of a menu or a context menu: buttons, links, groups and dividers,
/// the destructive ones last, as the guidelines ask.
struct MenuItems: View {
    let items: [Navree.Node]
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

    private func isDestructive(_ node: Navree.Node) -> Bool {
        if case .button(let button) = node { return button.role == .destructive }
        return false
    }

    @ViewBuilder private func entry(_ item: Navree.Node) -> some View {
        switch item {
        case .button(let button):
            Button(role: buttonRole(button.role)) {
                if let action = button.action {
                    context.send(.act(action, Place(overlay: overlay, ask: PresentationRules.question(button))))
                }
            } label: {
                ReasonedLabel(button: button)
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
