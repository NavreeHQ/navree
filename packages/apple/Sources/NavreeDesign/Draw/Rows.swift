import NavreeProtocol
import SwiftUI

/// One row of a screen's list.
struct RowView: View {
    let item: RowItem
    @Environment(\.navree) private var context
    @Environment(\.navreeChoose) private var choose
    #if os(iOS)
    @Environment(\.editMode) private var editMode
    #endif

    /// Whether the list's selection is the row open beside it, which any row
    /// may be, rather than the rows ticked for an action on many.
    private var choosesOpen: Bool {
        #if os(iOS)
        choose != nil && !(editMode?.wrappedValue.isEditing ?? false)
        #else
        choose != nil
        #endif
    }

    var body: some View {
        switch item {
        case .part(let node):
            NodeView(node)
        case .record(let row):
            RecordRowView(row: row)
                .tag(row.id ?? "")
                .selectionDisabled(!choosesOpen && !row.choosable)
        case .row(let row):
            TreeRowView(row: row)
                .tag(row.id ?? "")
        case .button(let button, let form):
            ButtonRow(button: button)
                .environment(\.navreeForm, form.map(FormRef.init))
        case .fact(let fact):
            FactContent(fact: fact)
        case .labeled(let label, let value, let muted):
            LabeledContent {
                Text(value).monospacedDigit()
            } label: {
                Text(label).foregroundStyle(muted ? Color.secondary : Color.primary)
            }
            .fontWeight(muted ? nil : .semibold)
        case .field(let field, let form):
            FieldControl(field: field)
                .environment(\.navreeForm, FormRef(form))
        case .option(let field, let option, let form):
            OptionRow(field: field, option: option, form: FormRef(form))
        case .pending(let region):
            if context.region(region).failed {
                RetryRow(title: "Couldn't Load This") { context.send(.retry(region: region)) }
            } else {
                ProgressView()
                    .frame(maxWidth: .infinity)
                    .task { context.send(.appear(region: region)) }
            }
        case .more(let target, let href):
            if context.list(target).failed {
                RetryRow(title: "Couldn't Load More") { context.send(.more(target: target, href: href)) }
            } else {
                ProgressView()
                    .frame(maxWidth: .infinity)
                    .task { context.send(.more(target: target, href: href)) }
            }
        }
    }
}

/// A row's way to what it opens: a link that leaves its presentation to the
/// app, or asks for a push, has the system's own disclosure and asks the
/// runtime as it is tapped; anything else is a tap that asks it.
struct Opening<Label: View>: View {
    let action: Navree.Action?
    let place: Place
    @ViewBuilder let label: () -> Label
    @Environment(\.navree) private var context
    @Environment(\.navreeChoose) private var choose

    var body: some View {
        if let action, let choose, NavigationRules.pushes(action, routes: context.routes) {
            // A row of a list beside its detail: a tap chooses what the detail
            // shows. A Mac's list selects a link of its own accord (as a window
            // comes to the front), so these rows are no links.
            Button {
                choose(action, place)
            } label: {
                label().contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        } else if let action, NavigationRules.pushes(action, routes: context.routes) {
            NavigationLink(value: PathEntry.tap(action, place)) {
                label()
            }
        } else if let action {
            Button {
                context.send(.act(action, place))
            } label: {
                label().contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        } else {
            label()
        }
    }
}

/// A table's record as a row: its name and a line under it, its states as
/// words with a symbol, and what stands at its end. Forms and buttons of the
/// record are its swipe actions and the items of its context menu.
struct RecordRowView: View {
    let row: RecordRow
    @State private var formSheet: ActionForm?
    @Environment(\.navree) private var context

    var body: some View {
        Opening(action: row.action, place: Place(form: row.actionForm.map(FormRef.init))) {
            RecordContent(row: row)
        }
        .swipeActions(edge: .trailing) {
            ForEach(Array(actions.enumerated()), id: \.offset) { _, action in
                Button(role: buttonRole(action.button.role)) {
                    perform(action)
                } label: {
                    ButtonLabel(button: action.button, mode: .both)
                }
                .tint(action.button.role == .destructive ? .red : .accentColor)
            }
        }
        .contextMenu {
            ForEach(Array(actions.enumerated()), id: \.offset) { _, action in
                Button(role: buttonRole(action.button.role)) {
                    perform(action)
                } label: {
                    ButtonLabel(button: action.button, mode: .both)
                }
            }
        }
        .sheet(item: $formSheet) { sheet in
            if let form = sheet.form {
                FormSheet(title: sheet.button.label, subtitle: row.title, form: form, confirm: sheet.button)
            }
        }
    }

    /// The record's controls: a form that holds fields opens in a sheet; a
    /// form or button with nothing to fill acts at once.
    private var actions: [ActionForm] {
        row.controls.flatMap { cell -> [ActionForm] in
            var found: [ActionForm] = []
            cell.walk { node in
                if case .form(let form) = node {
                    var submit: Navree.Button?
                    var fields = 0
                    form.walkFields { inner in
                        if case .button(let button) = inner, submit == nil { submit = button }
                        if case .field = inner { fields += 1 }
                    }
                    if let submit { found.append(ActionForm(button: submit, form: form, fields: fields)) }
                }
            }
            if found.isEmpty {
                cell.walk { node in
                    if case .button(let button) = node { found.append(ActionForm(button: button, form: nil, fields: 0)) }
                }
            }
            return found
        }
    }

    private func perform(_ action: ActionForm) {
        if action.fields > 0, action.form != nil {
            formSheet = action
        } else if let act = action.button.action {
            context.send(.act(act, Place(form: action.form.map(FormRef.init), ask: PresentationRules.question(action.button))))
        }
    }
}

/// A control of a record and the form it stands in.
struct ActionForm: Identifiable {
    let button: Navree.Button
    let form: Navree.Form?
    let fields: Int
    var id: String { (form?.action ?? "") + button.label }
}

/// The words of a record row. A large text size stacks what stood side by side.
struct RecordContent: View {
    let row: RecordRow
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        let layout = typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 4))
            : AnyLayout(HStackLayout(alignment: .firstTextBaseline, spacing: 12))
        layout {
            if let leading = row.leading {
                NodeView(leading)
            }
            VStack(alignment: .leading, spacing: 3) {
                Text(row.title)
                    .font(.headline)
                    .foregroundStyle(.primary)
                if !row.subtitle.isEmpty {
                    Text(row.subtitle.joined(separator: " · "))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                if !row.statuses.isEmpty {
                    statuses
                }
            }
            if !typeSize.isAccessibilitySize {
                Spacer(minLength: 0)
            }
            if let trailing = row.trailing {
                Text(trailing)
                    .font(.subheadline)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .accessibilityLabel(row.trailingLabel.map { "\($0), \(trailing)" } ?? trailing)
            }
        }
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder private var statuses: some View {
        let parts = HStack(spacing: 12) {
            ForEach(Array(row.statuses.enumerated()), id: \.offset) { _, badge in StatusLabel(badge: badge) }
        }
        ViewThatFits(in: .horizontal) {
            parts
            VStack(alignment: .leading, spacing: 2) {
                ForEach(Array(row.statuses.enumerated()), id: \.offset) { _, badge in StatusLabel(badge: badge) }
            }
        }
        .font(.subheadline)
        .foregroundStyle(.secondary)
    }
}

/// A row of the tree: what leads it, its name and lines, what stands at its
/// end, its swipe actions and its context menu.
struct TreeRowView: View {
    let row: Navree.Row
    @Environment(\.navree) private var context
    @Environment(\.navreeOverlay) private var overlay

    var body: some View {
        Opening(action: row.action, place: Place(overlay: overlay)) {
            RowContent(row: row)
        }
        .swipeActions(edge: .trailing) {
            ForEach(Array(row.swipe.enumerated()), id: \.offset) { _, button in
                Button(role: buttonRole(button.role)) {
                    if let action = button.action {
                        context.send(.act(action, Place(overlay: overlay, ask: PresentationRules.question(button))))
                    }
                } label: {
                    ButtonLabel(button: button, mode: .both)
                }
            }
        }
        .contextMenu {
            if !row.menu.isEmpty { MenuItems(items: row.menu) }
        }
    }
}

/// The parts of a row of the tree.
struct RowContent: View {
    let row: Navree.Row
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        HStack(alignment: Self.leadsWithPicture(row) ? .center : .firstTextBaseline, spacing: 12) {
            if let leading = row.leading {
                NodeView(leading).font(.body)
            } else if let state = row.state {
                Image(systemName: Self.symbol(state))
                    .foregroundStyle(state == .upcoming ? Color.secondary : Color.accentColor)
            }
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 8) {
                    Text(row.title).foregroundStyle(row.state == .upcoming ? Color.secondary : Color.primary)
                    if let badge = row.badge {
                        StatusLabel(badge: badge).font(.subheadline).foregroundStyle(.secondary)
                    }
                }
                if let subtitle = row.subtitle {
                    Text(subtitle).font(.subheadline).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                ForEach(Array(RowRules.detailLines(row.detail).enumerated()), id: \.offset) { _, line in
                    if line.count > 1 {
                        // States one after another share a line, as Mail's flags do.
                        Flow(spacing: 12, lineSpacing: 4) {
                            ForEach(Array(line.enumerated()), id: \.offset) { _, part in NodeView(part) }
                        }
                        .font(.subheadline).foregroundStyle(.secondary)
                    } else if let part = line.first {
                        NodeView(part).font(.footnote).foregroundStyle(.secondary)
                            .environment(\.navreeInRowDetail, true)
                    }
                }
            }
            if let trailing = row.trailing, !typeSize.isAccessibilitySize {
                Spacer(minLength: 0)
                NodeView(trailing).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }

    /// A row led by a picture with no words in it (a chip, an avatar, an
    /// image) sets it beside the middle of its words, as Mail and Fitness do.
    static func leadsWithPicture(_ row: Navree.Row) -> Bool {
        switch row.leading {
        case .layers?, .avatar?, .image?, .shape?: true
        default: false
        }
    }

    static func symbol(_ state: Navree.RowState) -> String {
        switch state {
        case .done: "checkmark.circle.fill"
        case .current: "circle.inset.filled"
        case .upcoming: "circle"
        }
    }
}

/// A button as a row of a grouped list: its words in the tint, in red when it
/// destroys, a spinner beside them while it waits.
struct ButtonRow: View {
    let button: Navree.Button
    @Environment(\.navree) private var context
    @Environment(\.navreeForm) private var form
    @Environment(\.navreeOverlay) private var overlay

    var body: some View {
        Button(role: buttonRole(button.role)) {
            if let action = button.action {
                context.send(.act(action, Place(form: form, overlay: overlay, ask: PresentationRules.question(button))))
            }
        } label: {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    ButtonLabel(button: button, mode: .title)
                    if let reason = ActionRules.reason(button), !inFootedSection {
                        Text(reason).font(.footnote).foregroundStyle(.secondary)
                    }
                }
                if button.pending {
                    Spacer()
                    ProgressView()
                }
            }
        }
        .disabled(button.disabled || button.pending)
        .modifier(RowButtonLook())
        .modifier(PopoverAnchor(action: button.action))
    }

    /// Set where the section's footer already says why.
    var inFootedSection = false
}

/// A button that is a row of a list: on iPhone and iPad the row itself, its
/// words in the tint (in red when it destroys); on a Mac, the Mac's own push
/// button at its regular size, with the system's insets round its words.
struct RowButtonLook: ViewModifier {
    func body(content: Content) -> some View {
        #if os(macOS)
        content.buttonStyle(.bordered).controlSize(.regular).fixedSize()
        #else
        content
        #endif
    }
}

/// A row that says something did not come, and offers to try again.
struct RetryRow: View {
    let title: LocalizedStringKey
    let retry: () -> Void

    var body: some View {
        #if os(macOS)
        // On a Mac: what did not come, and a push button to try again.
        HStack {
            Label(title, systemImage: "exclamationmark.triangle").foregroundStyle(.secondary)
            Spacer(minLength: 0)
            Button("Try Again", action: retry).modifier(RowButtonLook())
        }
        #else
        Button(action: retry) {
            Label {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).foregroundStyle(.primary)
                    Text("Tap to try again.").font(.subheadline).foregroundStyle(.secondary)
                }
            } icon: {
                Image(systemName: "arrow.clockwise")
            }
        }
        #endif
    }
}

/// A label and its value, as Settings shows one; a fact with lines under its
/// value stacks them under its label, as Contacts shows a contact's details.
struct FactContent: View {
    let fact: Navree.Fact
    @Environment(\.dynamicTypeSize) private var typeSize

    private var value: String {
        fact.value.flatMap { RecordRules.words(in: $0) }.joined(separator: " ")
    }

    var body: some View {
        if fact.details.isEmpty && !typeSize.isAccessibilitySize {
            LabeledContent {
                Text(value).foregroundStyle(.secondary)
            } label: {
                label
            }
        } else {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                if let icon = fact.icon {
                    Glyph(icon: icon).foregroundStyle(.secondary)
                }
                VStack(alignment: .leading, spacing: 3) {
                    Text(fact.label).font(.subheadline).foregroundStyle(.secondary)
                    Text(value)
                    ForEach(fact.details, id: \.self) { line in
                        Text(line).font(.subheadline).foregroundStyle(.secondary)
                    }
                }
            }
            .accessibilityElement(children: .combine)
        }
    }

    @ViewBuilder private var label: some View {
        if let icon = fact.icon {
            Label { Text(fact.label) } icon: { Glyph(icon: icon).foregroundStyle(.secondary) }
        } else {
            Text(fact.label)
        }
    }
}

/// One option of a field that takes any of several, as a row with a checkmark.
struct OptionRow: View {
    let field: Navree.Field
    let option: Navree.ChoiceOption
    let form: FormRef
    @Environment(\.navree) private var context

    var body: some View {
        let values = context.values(form, field.name) ?? field.input.initialValues
        let chosen = values.contains(option.value)
        Button {
            let next = chosen ? values.filter { $0 != option.value } : values + [option.value]
            context.send(.change(form: form, name: field.name, values: next))
        } label: {
            HStack {
                Text(option.label).foregroundStyle(.primary)
                Spacer()
                if let detail = option.detail {
                    Text(detail).foregroundStyle(.secondary).monospacedDigit()
                }
                Image(systemName: "checkmark")
                    .fontWeight(.semibold)
                    .foregroundStyle(Color.accentColor)
                    .opacity(chosen ? 1 : 0)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(option.disabled)
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}
