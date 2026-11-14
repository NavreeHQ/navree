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

    /// The standard action's symbol, else the icon's, else the role's.
    private var symbol: Image? {
        if let standard = button.standard.flatMap(Symbols.standard) { return Image(systemName: standard) }
        if let image = button.icon.flatMap(Symbols.image(for:)) { return image }
        return Symbols.forRole(button.role).map { Image(systemName: $0) }
    }

    var body: some View {
        if showIcon, let symbol {
            if showTitle {
                Label { Text(button.label) } icon: { symbol }.labelStyle(.titleAndIcon)
            } else {
                Label { Text(button.label) } icon: { symbol }.labelStyle(.iconOnly)
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

    /// Drawn as its words or symbol alone: a low button, never the screen's main one.
    private var borderless: Bool {
        !prominent && !(button.prominence == .high && button.role != .destructive) && button.prominence == .low
    }

    var body: some View {
        let control = Button(role: buttonRole(button.role)) {
            if let action = button.action {
                context.send(.act(action, Place(form: form, overlay: overlay, ask: PresentationRules.question(button))))
            }
        } label: {
            Group {
                if button.pending {
                    HStack(spacing: 6) {
                        ProgressView()
                        ButtonLabel(button: button, mode: .title)
                    }
                } else {
                    ButtonLabel(button: button)
                }
            }
            .modifier(TargetFill(active: borderless))
        }
        .disabled(button.disabled || button.pending)
        .modifier(PopoverAnchor(action: button.action))
        VStack(alignment: .leading, spacing: 4) {
            Group {
                if prominent || (button.prominence == .high && button.role != .destructive) {
                    control.buttonStyle(.borderedProminent).modifier(FingerTarget())
                } else if borderless {
                    TargetRoom { control.buttonStyle(.borderless) }
                } else {
                    control.buttonStyle(.bordered).modifier(FingerTarget())
                }
            }
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

/// A control that draws only its words or its symbol (a borderless button, a
/// link, a menu in the content), given the target a finger needs on iPhone
/// and iPad (``HitRules/target(drawn:bar:)``): the layout round it is told
/// the size it draws, so it looks as before and the line it stands on keeps
/// its spacing, and the control itself is placed as large as its target,
/// centred on what it draws. Its label fills what it is given
/// (``TargetFill``), so the whole target takes the tap and VoiceOver's frame
/// is the target. A Mac's pointer needs only what is drawn.
struct TargetRoom: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard let control = subviews.first else { return .zero }
        // What it draws: its own size, or wrapped to a narrower width offered.
        let ideal = control.sizeThatFits(.unspecified)
        guard let width = proposal.width, width < ideal.width else { return ideal }
        return control.sizeThatFits(ProposedViewSize(width: width, height: nil))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard let control = subviews.first else { return }
        let target = HitRules.target(drawn: (Double(bounds.width), Double(bounds.height)), bar: .current)
        control.place(
            at: CGPoint(x: bounds.midX, y: bounds.midY), anchor: .center,
            proposal: ProposedViewSize(width: target.width, height: target.height)
        )
    }
}

/// The label of a control in a ``TargetRoom``: as large as the target it is
/// given, what it draws centred in it, the whole of it taking the tap.
struct TargetFill: ViewModifier {
    var active = true

    func body(content: Content) -> some View {
        if active, ActionBar.current == .navigation {
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .contentShape(Rectangle())
        } else {
            content
        }
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
            TargetRoom {
                Button {
                    context.send(.act(link.action, Place(overlay: overlay, from: link.id)))
                } label: {
                    Text(link.label).modifier(TargetFill())
                }
                .buttonStyle(.borderless)
            }
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
        TargetRoom {
            Menu {
                MenuItems(items: menu.items)
            } label: {
                Group {
                    switch (ContentRules.menuLabel(menu), menu.icon) {
                    case (.titleAndIcon, let icon?):
                        label(icon).labelStyle(.titleAndIcon)
                    case (.icon, let icon?):
                        label(icon).labelStyle(.iconOnly)
                    default:
                        Text(menu.label)
                    }
                }
                .modifier(TargetFill())
            }
            .accessibilityLabel(Text(menu.label))
        }
        .fixedSize()
    }

    @ViewBuilder private func label(_ icon: Navree.Icon) -> some View {
        if let symbol = Symbols.image(for: icon) {
            Label { Text(menu.label) } icon: { symbol }
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
