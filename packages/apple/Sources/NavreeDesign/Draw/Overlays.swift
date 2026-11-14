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
    var actions: [Navree.Node] = []
    @Environment(\.navree) private var context

    func body(content: Content) -> some View {
        let open = SectionRules.overlays(tree).filter { Self.isOpen($0) || Self.isOpenPopover($0, context.opened) }
        let sheet = open.first { node in
            switch node {
            case .sheet: true
            case .dialog(let dialog): PresentationRules.dialog(dialog) == .sheet
            // A popover nothing on the screen opens has no control to stand by: a sheet.
            case .popover(let popover): !PopoverRules.anchored(popover.id, in: tree, actions: actions)
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

    static func isOpenPopover(_ node: Navree.Node, _ opened: Set<String>) -> Bool {
        if case .popover(let popover) = node { return opened.contains(popover.id) }
        return false
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
/// its title in the bar, Cancel (a task that holds input) or Close at the top
/// leading corner, the form's own submit (or the confirming action) at the top
/// trailing corner, its parts as a grouped list and its destructive actions in
/// a section of their own at the end. A glance rests at half height and is
/// swiped away; a task takes the full height.
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
                    PlacedActions(
                        actions: parts.actions.filter { !ScreenView.isDestructive($0) }, depth: .sheet, overlay: parts.id,
                        form: parts.form.map(FormRef.init)
                    )
                    if !parts.hasCancel {
                        ToolbarItem(placement: .cancellationAction) {
                            SheetDismissButton(parts.dismissal) { context.send(.dismissed(overlay: parts.id)) }
                        }
                    }
                }
                .environment(\.navreeOverlay, parts.id)
        }
        .presentationDetents(typeSize.isAccessibilitySize ? [.large] : parts.detents)
        .presentationDragIndicator(parts.detents.count > 1 && !typeSize.isAccessibilitySize ? .visible : .automatic)
        .modifier(SheetSizing())
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
        var dismissal: SheetDismissal
    }

    /// The parts of a sheet or a form dialog. A form's submit button leaves
    /// the content for the toolbar: there is no submit button at the foot of a
    /// form on iOS.
    static func parts(of node: Navree.Node) -> Parts {
        var parts = Parts(
            id: node.id ?? "", title: "", body: [], actions: [], detents: [.large], hasCancel: false, dismissal: .close
        )
        var task = false
        switch node {
        case .sheet(let sheet):
            parts.title = sheet.title ?? ""
            parts.subtitle = sheet.subtitle
            parts.body = sheet.children
            parts.actions = ActionRules.expand(sheet.actions)
            task = sheet.task
            parts.detents = SheetRules.height(task: task) == .half && PresentationRules.sheet(sheet.size) != .large
                ? [.medium, .large] : [.large]
        case .dialog(let dialog):
            parts.title = dialog.title ?? ""
            parts.text = dialog.text
            parts.body = dialog.children
            parts.actions = ActionRules.expand(dialog.actions)
            // A dialog that holds a form is a small task.
            task = true
            parts.detents = [.medium, .large]
        case .popover(let popover):
            parts.body = popover.children
            parts.detents = [.medium, .large]
        default:
            break
        }
        // The sheet's own form gives its submit buttons to the toolbar, as the
        // confirming action. A dialog or sheet inside it keeps its own.
        let lifted = SheetRules.lift(parts.body)
        parts.form = lifted.form
        parts.actions.append(contentsOf: lifted.confirms.map(Navree.Node.button))
        parts.hasCancel = parts.actions.contains { node in
            if case .button(let button) = node { return button.role == .cancel }
            return false
        }
        parts.dismissal = SheetRules.dismissal(task: task, holdsInput: SheetRules.holdsInput(parts.body))
        return parts
    }
}

/// The content of a sheet: the dialog's words, then its parts as sections of
/// a grouped list, without the submit buttons the toolbar now holds, and its
/// destructive actions in a section of their own at the end.
struct OverlayContent: View {
    let parts: OverlaySheet.Parts

    var body: some View {
        let sections = SectionRules.sections(parts.body).map(Self.withoutSubmits)
        let destructive = parts.actions.compactMap { node -> Navree.Button? in
            if case .button(let button) = node, ScreenView.isDestructive(node) { return button }
            return nil
        }
        List {
            if let text = parts.text {
                Section {
                    Text(text).foregroundStyle(.secondary)
                }
            }
            ForEach(sections) { section in
                SectionView(section: section)
            }
            if !destructive.isEmpty {
                Section {
                    ForEach(Array(destructive.enumerated()), id: \.offset) { _, button in
                        ButtonRow(button: button, inFootedSection: true)
                    }
                } footer: {
                    if let footer = ActionRules.footer(destructive) { Text(footer) }
                }
            }
        }
        .modifier(ListLook(layout: .insetGrouped))
        .sheetListFits()
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
            .sheetListFits()
            .navigationTitle(title)
            .modifier(Subtitle(text: subtitle))
            .navigationBarTitleDisplayModeInline()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    SheetDismissButton(.cancel) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm) {
                        if let action = confirm.action { context.send(.act(action, Place(form: ref))) }
                        dismiss()
                    } label: {
                        SheetButtonLabel(verbatim: confirm.label, systemImage: "checkmark")
                    }
                    .sheetKey(.confirm)
                }
            }
            .environment(\.navreeForm, ref)
        }
        .presentationDetents(typeSize.isAccessibilitySize ? [.large] : [.medium, .large])
        .presentationDragIndicator(.visible)
        .modifier(SheetSizing())
    }
}

/// A sheet's size where the window gives it none of its own: on a Mac, the
/// system's size for a form, so a sheet whose content is a list (which has no
/// height of its own) shows it rather than shrinking to its bar, within the
/// room the window it stands on has, so it never hangs over that window's
/// edges. iPhone and iPad size a sheet by its detents.
public struct SheetSizing: ViewModifier {
    @Environment(\.navreeSheetRoom) private var room
    /// How much taller than its room the sheet's list is (shorter when
    /// negative), once the list has been laid out.
    @State private var overflow: CGFloat?

    public init() {}

    public func body(content: Content) -> some View {
        switch PresentationRules.sheetSizing(bar: .current) {
        case .detents:
            content
        case .form:
            #if os(macOS)
            content
                .presentationSizing(RoomFormSizing(room: room))
                .onPreferenceChange(SheetOverflow.self) { value in overflow = value }
                .background(SheetWindowFit(overflow: overflow, room: room))
            #else
            content.presentationSizing(.form)
            #endif
        }
    }
}

/// How much taller than the room it is shown in a sheet's list is, said by
/// the list itself once laid out: a list has no height of its own to offer
/// the sheet, so the sheet learns it this way.
struct SheetOverflow: PreferenceKey {
    static let defaultValue: CGFloat? = nil
    static func reduce(value: inout CGFloat?, nextValue: () -> CGFloat?) {
        value = value ?? nextValue()
    }
}

extension View {
    /// A list that is a Mac sheet's content says how tall it is, so the sheet
    /// fits it (``PresentationRules/sheetSize(form:fitting:room:)``).
    @ViewBuilder func sheetListFits(_ inSheet: Bool = true) -> some View {
        #if os(macOS)
        if inSheet, PresentationRules.sheetSizing(bar: .current) == .form {
            modifier(SheetListFits())
        } else {
            self
        }
        #else
        self
        #endif
    }
}

#if os(macOS)
struct SheetListFits: ViewModifier {
    @State private var overflow: CGFloat?

    func body(content: Content) -> some View {
        content
            .onScrollGeometryChange(for: CGFloat.self) { geometry in
                (geometry.contentSize.height + geometry.contentInsets.top + geometry.contentInsets.bottom - geometry.containerSize.height)
                    .rounded()
            } action: { _, value in
                overflow = value
            }
            .preference(key: SheetOverflow.self, value: overflow)
    }
}

/// Keeps a Mac sheet at the size ``PresentationRules/sheetSize(form:fitting:room:)``
/// gives it: a form's width within the window it stands on, as tall as its
/// content up to a form's height. It follows the sheet's content as it is
/// laid out, the window under it as that window changes size, and a new
/// window when the app's shell moves into another (the window under an item
/// in the menu bar into a full window): the sheet takes the room it now has.
/// The sheet stays centred on its window, its top where it stands.
struct SheetWindowFit: NSViewRepresentable {
    let overflow: CGFloat?
    let room: CGSize?

    func makeNSView(context: Context) -> Fitter { Fitter() }

    func updateNSView(_ view: Fitter, context: Context) {
        view.overflow = overflow
        view.fit()
    }

    static func dismantleNSView(_ view: Fitter, coordinator: ()) { view.stop() }

    final class Fitter: NSView {
        var overflow: CGFloat?
        private var watching: [any NSObjectProtocol] = []

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            stop()
            guard let window else { return }
            let center = NotificationCenter.default
            // The window under it changing size, or the sheet coming into a new window.
            watching = [
                center.addObserver(forName: NSWindow.didResizeNotification, object: nil, queue: .main) { [weak self] note in
                    let resized = note.object as? NSWindow
                    MainActor.assumeIsolated {
                        guard let self, let resized, resized === self.window?.sheetParent else { return }
                        self.fit()
                    }
                },
                center.addObserver(forName: NSWindow.didBecomeKeyNotification, object: window, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.fit() }
                },
            ]
            DispatchQueue.main.async { [weak self] in MainActor.assumeIsolated { self?.fit() } }
        }

        func stop() {
            for token in watching { NotificationCenter.default.removeObserver(token) }
            watching = []
        }

        func fit() {
            guard let sheet = window, let parent = sheet.sheetParent else { return }
            let content = sheet.contentRect(forFrameRect: sheet.frame)
            let fitting = overflow.map { content.height + $0 }
            let size = PresentationRules.sheetSize(
                form: PresentationRules.formSize, fitting: fitting, room: parent.contentLayoutRect.size
            )
            let frame = sheet.frameRect(forContentRect: CGRect(origin: .zero, size: size))
            let target = CGRect(
                x: (parent.frame.midX - frame.width / 2).rounded(), y: sheet.frame.maxY - frame.height,
                width: frame.width, height: frame.height
            )
            guard abs(target.width - sheet.frame.width) > 0.5 || abs(target.height - sheet.frame.height) > 0.5
                || abs(target.minX - sheet.frame.minX) > 0.5
            else { return }
            sheet.setFrame(target, display: true)
        }
    }
}
#endif

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
