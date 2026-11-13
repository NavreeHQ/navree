import NavreeProtocol
import SwiftUI

extension EnvironmentValues {
    /// How wide the window is, as the runtime core was told: what the drawing
    /// lays a screen out by. Set once at the top of the app.
    @Entry public var navreeWindow: WindowClass = .compact
    /// How many times Find (⌘F) was chosen in the window: each change asks
    /// the search field of the screen in front to take the focus.
    @Entry public var navreeFind: Int = 0
    /// Set in the list of a list beside its detail: what a row that would push
    /// does instead, choosing the screen the detail shows.
    @Entry public var navreeChoose: (@MainActor @Sendable (Navree.Action, Place) -> Void)? = nil
}

/// The search field takes the focus when Find is chosen from the menu bar.
struct FindFocus: ViewModifier {
    @Environment(\.navreeFind) private var find
    @FocusState private var focused: Bool

    func body(content: Content) -> some View {
        content
            .searchFocused($focused)
            .onChange(of: find) { _, _ in focused = true }
    }
}

/// A list beside what was chosen in it, as the system's split view draws it:
/// the list in the leading column, the detail with its own navigation stack in
/// the other, and the inspector beside the detail where the window is wide
/// enough (a sheet on a compact width, on request elsewhere). On a compact
/// width the system folds the columns into one stack, showing first the
/// column `compactColumn` names.
///
/// A choice in the list is not a push: every link the list's rows follow is
/// handed to `choose`, which asks the runtime for the detail; the detail
/// column then shows what it answers.
public struct SplitColumns<ListColumn: View, DetailRoot: View, Inspector: View, Pushed: View>: View {
    let path: Binding<[PathEntry]>
    let compactColumn: CompactColumn
    let inspectorTitle: String?
    let choose: @MainActor @Sendable (Navree.Action, Place) -> Void
    let list: ListColumn
    let detail: DetailRoot
    let inspector: Inspector?
    let pushed: (PathEntry) -> Pushed

    @Environment(\.navreeWindow) private var window
    @State private var shown: NavigationSplitViewColumn
    @State private var inspecting: Bool?

    public init(
        path: Binding<[PathEntry]>, compactColumn: CompactColumn, inspectorTitle: String? = nil,
        choose: @escaping @MainActor @Sendable (Navree.Action, Place) -> Void,
        @ViewBuilder list: () -> ListColumn, @ViewBuilder detail: () -> DetailRoot,
        inspector: (() -> Inspector)? = nil, @ViewBuilder pushed: @escaping (PathEntry) -> Pushed
    ) {
        self.path = path
        self.compactColumn = compactColumn
        self.inspectorTitle = inspectorTitle
        self.choose = choose
        self.list = list()
        self.detail = detail()
        self.inspector = inspector?()
        self.pushed = pushed
        _shown = State(initialValue: compactColumn == .detail ? .detail : .sidebar)
    }

    public var body: some View {
        NavigationSplitView(preferredCompactColumn: $shown) {
            list
                // A row the list would push is a choice, asked of the runtime
                // for the detail column.
                .environment(\.navreeChoose, { action, place in
                    shown = .detail
                    choose(action, place)
                })
                // A list narrower than the window draws as a phone's would:
                // rows, never columns, its search under its title.
                .environment(\.horizontalSizeClass, .compact)
                .navigationSplitViewColumnWidth(min: 300, ideal: 360, max: 440)
        } detail: {
            NavigationStack(path: path) {
                detail
                    .navigationDestination(for: PathEntry.self, destination: pushed)
                    .toolbar {
                        if inspector != nil {
                            ToolbarItem(placement: .automatic) {
                                Button {
                                    inspecting = !inspectorShown
                                } label: {
                                    Label(inspectorTitle ?? String(localized: "Inspector"), systemImage: "sidebar.trailing")
                                }
                                .help(inspectorShown ? "Hide \(inspectorTitle ?? "Inspector")" : "Show \(inspectorTitle ?? "Inspector")")
                            }
                        }
                    }
            }
            .modifier(InspectorColumn(content: inspector, shown: inspectorBinding))
            .modifier(DetailMinimum())
        }
        .navigationSplitViewStyle(.balanced)
        .onChange(of: compactColumn) { _, column in shown = column == .detail ? .detail : .sidebar }
    }

    /// Shown beside the detail as the window's width says, until the person
    /// says otherwise.
    private var inspectorShown: Bool {
        inspecting ?? SplitRules.showsInspector(width: window)
    }

    private var inspectorBinding: Binding<Bool> {
        Binding(get: { inspectorShown }, set: { inspecting = $0 })
    }
}

extension SplitColumns where Inspector == EmptyView {
    /// A list beside its detail, with no inspector.
    public init(
        path: Binding<[PathEntry]>, compactColumn: CompactColumn,
        choose: @escaping @MainActor @Sendable (Navree.Action, Place) -> Void,
        @ViewBuilder list: () -> ListColumn, @ViewBuilder detail: () -> DetailRoot,
        @ViewBuilder pushed: @escaping (PathEntry) -> Pushed
    ) {
        self.init(
            path: path, compactColumn: compactColumn, inspectorTitle: nil, choose: choose, list: list, detail: detail,
            inspector: nil, pushed: pushed
        )
    }
}

/// The inspector beside the detail, with its own bar for its title.
struct InspectorColumn<Inspector: View>: ViewModifier {
    let content: Inspector?
    let shown: Binding<Bool>

    func body(content view: Content) -> some View {
        if let content {
            view.inspector(isPresented: shown) {
                NavigationStack {
                    content
                }
                .inspectorColumnWidth(min: 280, ideal: 320, max: 400)
            }
        } else {
            view
        }
    }
}

/// What the detail column shows while nothing is chosen in the list.
public struct NothingChosen: View {
    public init() {}

    public var body: some View {
        ContentUnavailableView("No Selection", systemImage: "sidebar.left")
    }
}

/// A record's controls in a wide table's cell: one button each, never a form
/// spread across the cell. A form with fields opens them in a sheet, with the
/// form's submit as Done, as a narrow window's swipe action does.
struct CellControls: View {
    let cell: Navree.Node
    let title: String
    @State private var filling: Filling?
    @Environment(\.navree) private var context

    struct Filling: Identifiable {
        let control: CellControl
        var id: String { (control.form?.action ?? "") + control.button.label }
    }

    var body: some View {
        let controls = RecordRules.controls(in: cell)
        HStack(spacing: 8) {
            ForEach(Array(controls.enumerated()), id: \.offset) { _, control in
                Button(role: buttonRole(control.button.role)) {
                    if control.opensSheet {
                        filling = Filling(control: control)
                    } else if let action = control.button.action {
                        context.send(.act(action, Place(form: control.form.map(FormRef.init), ask: PresentationRules.question(control.button))))
                    }
                } label: {
                    Text(CommandRules.title(control.button.label, more: control.opensSheet))
                }
                .buttonStyle(.borderless)
                .disabled(control.button.disabled || control.button.pending)
                .modifier(PointerHover())
            }
        }
        .sheet(item: $filling) { filling in
            if let form = filling.control.form {
                FormSheet(title: filling.control.button.label, subtitle: title, form: form, confirm: filling.control.button)
            }
        }
    }
}

/// The system's highlight under the pointer, on a control the system does
/// not already give one (an iPad's pointer; a Mac's lists highlight what is
/// chosen, not what is under the pointer, as Mail's and Notes' do).
struct PointerHover: ViewModifier {
    /// The corner of the part's own rounded shape, which the highlight takes.
    var corner: Double?

    func body(content: Content) -> some View {
        #if os(iOS)
        if let corner {
            content
                .contentShape(.hoverEffect, RoundedRectangle(cornerRadius: corner, style: .continuous))
                .hoverEffect(.highlight)
        } else {
            content.hoverEffect(.highlight)
        }
        #else
        content
        #endif
    }
}

/// A Mac window keeps room for the detail beside its list: the window stops
/// narrowing before the detail is squeezed (an iPad's split view folds instead).
struct DetailMinimum: ViewModifier {
    func body(content: Content) -> some View {
        #if os(macOS)
        content.frame(minWidth: 360)
        #else
        content
        #endif
    }
}

/// A split that stands among a screen's other parts (content beside what
/// supports it): its panes side by side where there is room, one over the
/// other where there is not, the one the server marks current first.
struct InPlaceSplit: View {
    let split: Navree.Split
    @Environment(\.navreeWindow) private var window

    var body: some View {
        let parts = SplitRules.inPlace(split, width: window)
        if parts.sideBySide {
            HStack(alignment: .top, spacing: 20) {
                ForEach(Array(parts.panes.enumerated()), id: \.offset) { _, pane in
                    NodeView(pane).frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        } else {
            VStack(alignment: .leading, spacing: 16) {
                ForEach(Array(parts.panes.enumerated()), id: \.offset) { _, pane in
                    NodeView(pane)
                }
            }
        }
    }
}
