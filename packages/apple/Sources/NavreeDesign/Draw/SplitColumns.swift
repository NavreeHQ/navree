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
    @Entry public var navreeChoose: Choice? = nil
    /// Set in the list of a list beside its detail: the address of the screen
    /// the detail shows, whose row the list shows chosen.
    @Entry public var navreeOpen: String? = nil
}

/// What a row of a list beside its detail does in place of a push: asks the
/// runtime for the screen the detail shows. Two are the same when they serve
/// the same list, whatever their code: the system cannot compare code, and a
/// choice handed down anew each time the split view is drawn would have every
/// row drawn again with it, and the bars the rows' screens fill, and the split
/// view again, without end on a compact width.
public struct Choice: Equatable, Sendable {
    /// What the choice serves: the same key, the same choice.
    public let key: String
    let run: @MainActor @Sendable (Navree.Action, Place) -> Void

    public init(key: String, run: @escaping @MainActor @Sendable (Navree.Action, Place) -> Void) {
        self.key = key
        self.run = run
    }

    @MainActor public func callAsFunction(_ action: Navree.Action, _ place: Place) {
        run(action, place)
    }

    public static func == (a: Choice, b: Choice) -> Bool { a.key == b.key }
}

/// A navigation stack's path as the runtime holds it, and what a change the
/// system makes to it asks of the runtime. Two are the same when they show the
/// same entries for the same stack: a binding made of code is never the same
/// as the last, and would have the split view drawn again on every update.
public struct ColumnPath: Equatable, Sendable {
    public let entries: [PathEntry]
    /// The stack the path is of: the same key, the same follow.
    public let key: String
    let follow: @MainActor @Sendable ([PathEntry]) -> Void

    public init(entries: [PathEntry], key: String, follow: @escaping @MainActor @Sendable ([PathEntry]) -> Void) {
        self.entries = entries
        self.key = key
        self.follow = follow
    }

    public static func == (a: ColumnPath, b: ColumnPath) -> Bool { a.entries == b.entries && a.key == b.key }
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
/// With a `sidebar` (the app's destinations, on a Mac), the split view has
/// three columns, as Mail's mailboxes, messages and message: the sidebar,
/// the list as the content column, and the detail.
///
/// A choice in the list is not a push: every link the list's rows follow is
/// handed to `choose`, which asks the runtime for the detail; the detail
/// column then shows what it answers.
public struct SplitColumns<Sidebar: View, ListColumn: View, DetailRoot: View, Inspector: View, Pushed: View>: View {
    let path: ColumnPath
    let compactColumn: CompactColumn
    let inspectorTitle: String?
    let choose: Choice
    let sidebar: Sidebar?
    let list: ListColumn
    let detail: DetailRoot
    let inspector: Inspector?
    let pushed: (PathEntry) -> Pushed

    @Environment(\.navreeWindow) private var window
    @State private var shown: NavigationSplitViewColumn
    @State private var inspecting: Bool?
    /// The room the detail's column has, its inspector included.
    @State private var detailRoom: Double?

    public init(
        path: ColumnPath, compactColumn: CompactColumn, inspectorTitle: String? = nil,
        choose: Choice, sidebar: Sidebar?,
        @ViewBuilder list: () -> ListColumn, @ViewBuilder detail: () -> DetailRoot,
        inspector: (() -> Inspector)? = nil, @ViewBuilder pushed: @escaping (PathEntry) -> Pushed
    ) {
        self.path = path
        self.compactColumn = compactColumn
        self.inspectorTitle = inspectorTitle
        self.choose = choose
        self.sidebar = sidebar
        self.list = list()
        self.detail = detail()
        self.inspector = inspector?()
        self.pushed = pushed
        _shown = State(initialValue: Self.column(compactColumn, sidebar: sidebar != nil))
    }

    public var body: some View {
        Group {
            if let sidebar {
                NavigationSplitView(preferredCompactColumn: $shown) {
                    sidebar
                        .navigationSplitViewColumnWidth(min: 180, ideal: 220, max: 320)
                } content: {
                    listColumn
                } detail: {
                    detailColumn
                }
            } else {
                NavigationSplitView(preferredCompactColumn: $shown) {
                    listColumn
                } detail: {
                    detailColumn
                }
            }
        }
        .navigationSplitViewStyle(.balanced)
        // The room the detail's column has: the window's, less the sidebar's
        // and the list's columns at their ideal widths.
        .onGeometryChange(for: Double.self) { Double($0.size.width) } action: { [hasSidebar = sidebar != nil] width in
            detailRoom = SplitRules.detailRoom(window: width, sidebar: hasSidebar)
        }
        .onChange(of: compactColumn) { _, column in shown = Self.column(column, sidebar: sidebar != nil) }
    }

    /// The split view's column that holds the list or the detail.
    static func column(_ column: CompactColumn, sidebar: Bool) -> NavigationSplitViewColumn {
        switch column {
        case .detail: .detail
        case .list: sidebar ? .content : .sidebar
        }
    }

    private var listColumn: some View {
        list
            // A row the list would push is a choice, asked of the runtime
            // for the detail column.
            .environment(\.navreeChoose, Choice(key: choose.key) { [choose, shown = $shown] action, place in
                shown.wrappedValue = .detail
                choose(action, place)
            })
            // A list narrower than the window draws as a phone's would:
            // rows, never columns, its search under its title.
            .environment(\.horizontalSizeClass, .compact)
            .navigationSplitViewColumnWidth(min: 300, ideal: 360, max: 440)
    }

    private var detailColumn: some View {
        detailStack
            .navigationSplitViewColumnWidth(min: NavigationRules.detailMinimumWidth, ideal: 760)
    }

    private var detailStack: some View {
        NavigationStack(path: Binding(get: { path.entries }, set: { [path] next in path.follow(next) })) {
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

    /// Shown beside the detail as the window's width says, until the person
    /// says otherwise.
    private var inspectorShown: Bool {
        inspecting ?? (SplitRules.showsInspector(width: window) && SplitRules.inspectorFits(detailRoom: detailRoom))
    }

    private var inspectorBinding: Binding<Bool> {
        Binding(get: { inspectorShown }, set: { inspecting = $0 })
    }
}

extension SplitColumns where Sidebar == EmptyView {
    /// A list beside its detail, in two columns.
    public init(
        path: ColumnPath, compactColumn: CompactColumn, inspectorTitle: String? = nil,
        choose: Choice,
        @ViewBuilder list: () -> ListColumn, @ViewBuilder detail: () -> DetailRoot,
        inspector: (() -> Inspector)? = nil, @ViewBuilder pushed: @escaping (PathEntry) -> Pushed
    ) {
        self.init(
            path: path, compactColumn: compactColumn, inspectorTitle: inspectorTitle, choose: choose, sidebar: nil,
            list: list, detail: detail, inspector: inspector, pushed: pushed
        )
    }
}

extension SplitColumns where Sidebar == EmptyView, Inspector == EmptyView {
    /// A list beside its detail, in two columns, with no inspector.
    public init(
        path: ColumnPath, compactColumn: CompactColumn,
        choose: Choice,
        @ViewBuilder list: () -> ListColumn, @ViewBuilder detail: () -> DetailRoot,
        @ViewBuilder pushed: @escaping (PathEntry) -> Pushed
    ) {
        self.init(
            path: path, compactColumn: compactColumn, inspectorTitle: nil, choose: choose, sidebar: nil, list: list,
            detail: detail, inspector: nil, pushed: pushed
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
                .inspectorColumnWidth(min: CGFloat(SplitRules.inspectorLeast), ideal: 320, max: 400)
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
                .modifier(CellControlStyle())
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

/// A control in a wide table's cell, as ``ContentRules/cellControlLook(bar:)`` draws it.
struct CellControlStyle: ViewModifier {
    func body(content: Content) -> some View {
        switch ContentRules.cellControlLook(bar: .current) {
        case .borderless: content.buttonStyle(.borderless)
        case .smallPushButton: content.buttonStyle(.bordered).controlSize(.small)
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
        content.frame(minWidth: CGFloat(NavigationRules.detailMinimumWidth), maxWidth: .infinity, maxHeight: .infinity)
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
