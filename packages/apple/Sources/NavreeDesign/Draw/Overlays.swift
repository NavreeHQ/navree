import NavreeProtocol
import SwiftUI

/// A sheet or a dialog of the tree that is open now.
struct OpenOverlay: Identifiable, Equatable {
    let node: Navree.Node
    var id: String { node.id ?? "" }
}

/// Presents the sheets and dialogs of a screen's tree that are open, each the
/// way Apple's guidelines present that kind: an alert, an action sheet from
/// its source, or a sheet; one at a time.
struct OverlayPresenter: ViewModifier {
    let tree: Navree.Node
    @Environment(\.navree) private var context

    func body(content: Content) -> some View {
        let open = SectionRules.overlays(tree).filter(Self.isOpen)
        let sheet = open.first { node in
            switch node {
            case .sheet: true
            case .dialog(let dialog): PresentationRules.dialog(dialog) == .sheet
            default: false
            }
        }
        let alert = open.first { node in
            if case .dialog(let dialog) = node { return PresentationRules.dialog(dialog) == .alert }
            return false
        }
        let question = open.first { node in
            if case .dialog(let dialog) = node { return PresentationRules.dialog(dialog) == .confirmationDialog }
            return false
        }
        content
            .sheet(item: binding(sheet.map(OpenOverlay.init))) { overlay in
                OverlaySheet(node: overlay.node)
            }
            .alert(
                title(alert),
                isPresented: presented(alert),
                presenting: dialog(alert)
            ) { dialog in
                DialogButtons(dialog: dialog)
            } message: { dialog in
                if let text = dialog.text { Text(text) }
            }
            .confirmationDialog(
                title(question),
                isPresented: presented(question),
                titleVisibility: .visible,
                presenting: dialog(question)
            ) { dialog in
                DialogButtons(dialog: dialog)
            } message: { dialog in
                if let text = dialog.text { Text(text) }
            }
    }

    static func isOpen(_ node: Navree.Node) -> Bool {
        switch node {
        case .sheet(let sheet): sheet.open
        case .dialog(let dialog): dialog.open
        default: false
        }
    }

    private func dialog(_ node: Navree.Node?) -> Navree.Dialog? {
        if case .dialog(let dialog)? = node { return dialog }
        return nil
    }

    private func title(_ node: Navree.Node?) -> String {
        dialog(node)?.title ?? ""
    }

    private func presented(_ node: Navree.Node?) -> Binding<Bool> {
        Binding(get: { node != nil }, set: { shown in
            if !shown, let id = node?.id { context.send(.dismissed(overlay: id)) }
        })
    }

    private func binding(_ overlay: OpenOverlay?) -> Binding<OpenOverlay?> {
        Binding(get: { overlay }, set: { next in
            if next == nil, let id = overlay?.id { context.send(.dismissed(overlay: id)) }
        })
    }
}

/// A dialog's answers as an alert's or an action sheet's buttons: the
/// destructive one first, Cancel last, as the system orders them.
struct DialogButtons: View {
    let dialog: Navree.Dialog
    @Environment(\.navree) private var context

    var body: some View {
        ForEach(Array(dialog.actions.enumerated()), id: \.offset) { _, node in
            if case .button(let button) = node {
                Button(button.label, role: buttonRole(button.role)) {
                    if let action = button.action { context.send(.act(action, Place(overlay: dialog.id))) }
                }
            }
        }
    }
}

/// A sheet of the tree, or a dialog that holds a form, presented as a sheet:
/// its title in the bar, Cancel at the top leading corner, the form's own
/// submit (or the confirming action) at the top trailing corner, and its
/// parts as a grouped list. A medium sheet rests at half height.
struct OverlaySheet: View {
    let node: Navree.Node
    @Environment(\.navree) private var context
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        let parts = Self.parts(of: node)
        NavigationStack {
            OverlayContent(parts: parts)
                .navigationTitle(parts.title)
                .modifier(Subtitle(text: parts.subtitle))
                .navigationBarTitleDisplayModeInline()
                .toolbar {
                    PlacedActions(actions: parts.actions, depth: .sheet, overlay: parts.id, form: parts.form.map(FormRef.init))
                    if !parts.hasCancel {
                        ToolbarItem(placement: .cancellationAction) {
                            Button(role: .close) {
                                context.send(.dismissed(overlay: parts.id))
                            } label: {
                                Label("Close", systemImage: "xmark")
                            }
                        }
                    }
                }
                .environment(\.navreeOverlay, parts.id)
        }
        .presentationDetents(typeSize.isAccessibilitySize ? [.large] : parts.detents)
        .presentationDragIndicator(parts.detents.count > 1 && !typeSize.isAccessibilitySize ? .visible : .automatic)
    }

    /// What a sheet is made of, read once.
    struct Parts {
        var id: String
        var title: String
        var subtitle: String?
        var text: String?
        var body: [Navree.Node]
        var actions: [Navree.Node]
        var form: Navree.Form?
        var detents: Set<PresentationDetent>
        var hasCancel: Bool
    }

    /// The first form in `node` that is not inside another sheet or dialog.
    static func ownForm(_ node: Navree.Node) -> Navree.Form? {
        switch node {
        case .form(let form): return form
        case .sheet, .dialog, .popover: return nil
        default: return node.children.lazy.compactMap(ownForm).first
        }
    }

    /// The parts of a sheet or a form dialog. A form's submit button leaves
    /// the content for the toolbar: there is no submit button at the foot of a
    /// form on iOS.
    static func parts(of node: Navree.Node) -> Parts {
        var parts = Parts(id: node.id ?? "", title: "", body: [], actions: [], detents: [.large], hasCancel: false)
        switch node {
        case .sheet(let sheet):
            parts.title = sheet.title ?? ""
            parts.subtitle = sheet.subtitle
            parts.body = sheet.children
            parts.actions = sheet.actions
            parts.detents = PresentationRules.sheet(sheet.size) == .resizable ? [.medium, .large] : [.large]
        case .dialog(let dialog):
            parts.title = dialog.title ?? ""
            parts.text = dialog.text
            parts.body = dialog.children
            parts.actions = dialog.actions
            parts.detents = [.medium, .large]
        default:
            break
        }
        // The sheet's own form gives its submit buttons to the toolbar, as the
        // confirming action. A dialog or sheet inside it keeps its own.
        if let form = parts.body.lazy.compactMap(Self.ownForm).first {
            parts.form = form
            form.walkFields { field in
                if case .button(var button) = field, case .submit? = button.action {
                    if button.role == Navree.Role.none || button.role == .primary { button.role = .confirm }
                    parts.actions.append(.button(button))
                }
            }
        }
        parts.hasCancel = parts.actions.contains { node in
            if case .button(let button) = node { return button.role == .cancel }
            return false
        }
        return parts
    }
}

/// The content of a sheet: the dialog's words, then its parts as sections of
/// a grouped list, without the submit buttons the toolbar now holds.
struct OverlayContent: View {
    let parts: OverlaySheet.Parts

    var body: some View {
        let sections = SectionRules.sections(parts.body).map(Self.withoutSubmits)
        List {
            if let text = parts.text {
                Section {
                    Text(text).foregroundStyle(.secondary)
                }
            }
            ForEach(sections) { section in
                SectionView(section: section)
            }
        }
        .modifier(ListLook(layout: .insetGrouped))
    }

    static func withoutSubmits(_ section: ListSection) -> ListSection {
        var section = section
        section.rows.removeAll { row in
            if case .button(let button, _) = row, case .submit? = button.action { return true }
            return false
        }
        return section
    }
}

/// A form presented on its own in a sheet: a record's restock form, opened
/// from the record's swipe action or context menu.
struct FormSheet: View {
    let title: String
    let subtitle: String?
    let form: Navree.Form
    let confirm: Navree.Button
    @Environment(\.dismiss) private var dismiss
    @Environment(\.navree) private var context
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        let ref = FormRef(form)
        NavigationStack {
            List {
                ForEach(SectionRules.sections(.form(form)).map(OverlayContent.withoutSubmits)) { section in
                    SectionView(section: section)
                }
            }
            .modifier(ListLook(layout: .insetGrouped))
            .navigationTitle(title)
            .modifier(Subtitle(text: subtitle))
            .navigationBarTitleDisplayModeInline()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .cancel) { dismiss() } label: { Label("Cancel", systemImage: "xmark") }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm) {
                        if let action = confirm.action { context.send(.act(action, Place(form: ref))) }
                        dismiss()
                    } label: {
                        Label(confirm.label, systemImage: "checkmark")
                    }
                }
            }
            .environment(\.navreeForm, ref)
        }
        .presentationDetents(typeSize.isAccessibilitySize ? [.large] : [.medium, .large])
        .presentationDragIndicator(.visible)
    }
}

extension View {
    /// An inline title, where the system has a choice of title modes.
    @ViewBuilder func navigationBarTitleDisplayModeInline() -> some View {
        #if os(iOS)
        self.navigationBarTitleDisplayMode(.inline)
        #else
        self
        #endif
    }
}
