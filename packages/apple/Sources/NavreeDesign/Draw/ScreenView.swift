import Foundation
import NavreeProtocol
import SwiftUI

/// A message the runtime holds for a screen: what the web would have shown as
/// a toast.
public struct Notice: Sendable, Equatable, Identifiable {
    public var id: String
    public var tone: Navree.Tone
    public var title: String?
    public var text: String
    /// What the message offers to do next: a link, or a button that posts.
    public var action: Navree.Action?
    public var actionLabel: String?

    public init(id: String, tone: Navree.Tone, title: String?, text: String, action: Navree.Action? = nil, actionLabel: String? = nil) {
        self.id = id
        self.tone = tone
        self.title = title
        self.text = text
        self.action = action
        self.actionLabel = actionLabel
    }
}

/// A whole screen, drawn as an iOS screen: its title in the navigation bar
/// (large at a destination's root), its content as a list of sections, its
/// actions in the toolbar by role, its search under the title or in the
/// toolbar by width, and its sheets and dialogs presented when they open.
public struct ScreenView: View {
    let screen: Navree.Screen
    let depth: ScreenDepth
    let appName: String
    let notice: Notice?
    let failure: Notice?

    @Environment(\.navree) private var context
    @Environment(\.horizontalSizeClass) private var sizeClass
    @Environment(\.navreeBesideList) private var besideList
    @Environment(\.dynamicTypeSize) private var typeSize

    public init(screen: Navree.Screen, depth: ScreenDepth, appName: String = "", notice: Notice? = nil, failure: Notice? = nil) {
        // The head row's buttons go to the bar before the day control is
        // placed, which on iOS can put it back at the head of the content.
        let lifted = ActionRules.lifted(PagerRules.lifted(screen), depth: depth)
        #if os(iOS)
        self.screen = PagerRules.placed(lifted, PagerRules.place(depth))
        #else
        self.screen = lifted
        #endif
        self.depth = depth
        self.appName = appName
        self.notice = notice
        self.failure = failure
    }

    /// The screen's own actions, a toolbar's items among them.
    private var actions: [Navree.Node] { ActionRules.expand(screen.actions) }

    /// A presented screen is a task: its form's submit leaves the content for
    /// the bar, as Done at the top trailing corner.
    private var lifted: LiftedSubmits {
        depth == .sheet || depth == .cover ? SheetRules.lift([screen.body]) : LiftedSubmits(form: nil, confirms: [])
    }

    public var body: some View {
        let lifted = lifted
        // A conversation's reply rests at the foot of the screen, not at the
        // end of what scrolls.
        let composer = depth == .sheet || depth == .cover ? nil : ComposerRules.composer(in: screen.body)
        let drawn = composer?.rest ?? screen.body
        let sections = SectionRules.sections(drawn).map { lifted.confirms.isEmpty ? $0 : OverlayContent.withoutSubmits($0) }
        let width = WidthClass(sizeClass)
        let table = sections.compactMap(\.table).first
        let query = QueryForm(screen: screen, table: table, region: sections.first { $0.table != nil }?.region)
        content(drawn, sections: sections, width: width, table: table)
            .modifier(ComposerDock(found: composer))
            .navigationTitle(NavigationRules.title(screen.title, appName: appName))
            .modifier(Subtitle(text: screen.subtitle, underLargeTitle: screen.pager == nil))
            .modifier(TitleDisplay(mode: PagerRules.titleMode(depth, pager: screen.pager)))
            .modifier(PagerInBar(pager: screen.pager, place: PagerRules.place(depth)))
            .toolbar {
                // The list's sort and filters, then what it makes new at the
                // trailing end, as Mail's filter and compose stand.
                if let query, query.hasFilters || !query.sorts.isEmpty {
                    ToolbarItem(placement: .primaryAction) {
                        QueryButtons(query: query).modifier(BarItemLook(.bar))
                    }
                }
                // A screen with no list of its own (a designed one) has no
                // section at its end: its destructive actions end its menu.
                let listed = !CompositionRules.isComposed(screen.body) && PaneRules.panes(in: screen.body).isEmpty
                PlacedActions(
                    actions: listed ? actions.filter { !Self.isDestructive($0) } : actions, depth: depth, overlay: nil, form: nil,
                    destructiveInMenu: !listed
                )
                if !lifted.confirms.isEmpty {
                    PlacedActions(
                        actions: lifted.confirms.map(Navree.Node.button), depth: depth, overlay: nil,
                        form: lifted.form.map(FormRef.init)
                    )
                }
                let foot = ActionRules.footToolbars(screen.body)
                if !foot.isEmpty { FootToolbar(toolbars: foot) }
            }
            .modifier(SearchModifier(query: query, placement: PresentationRules.search(width: width)))
            .modifier(Refresh(enabled: screen.refresh))
            .modifier(OverlayPresenter(tree: screen.body, actions: actions))
            .environment(\.navreePopovers, PopoverRules.popovers(in: screen.body))
            .alert(
                failure?.title ?? "Something went wrong",
                isPresented: Binding(get: { failure != nil }, set: { if !$0 { context.send(.noticeDismissed) } }),
                presenting: failure
            ) { _ in
                Button("OK", role: .cancel) {}
            } message: { failure in
                Text(failure.text)
            }
    }

    @ViewBuilder private func content(_ body: Navree.Node, sections: [ListSection], width: WidthClass, table: Navree.Table?) -> some View {
        let panes = PaneRules.panes(in: body)
        if let empty = Self.wholeEmpty(body) {
            EmptyPart(empty: empty)
        } else if !panes.isEmpty {
            PaneScreenBody(panes: panes)
        } else if CompositionRules.isComposed(body) {
            ComposedScreenBody(body: body, readable: CompositionRules.readableWidth(bar: .current, depth: depth))
        } else if let table, sections.count == 1,
            ContentRules.table(table, width: width, accessibilityText: typeSize.isAccessibilitySize) == .columns
        {
            TableColumnsView(
                table: table, region: sections[0].region,
                presenting: SectionRules.overlays(body).contains(where: OverlayPresenter.isOpen)
            )
        } else {
            SectionsList(
                tree: body, sections: sections, notice: notice, table: table,
                heading: NavigationRules.titleInContent(bar: .current, besideList: besideList) ? screen.title : nil,
                key: table.flatMap { $0.id } ?? sections.first { $0.table != nil }?.region ?? "",
                // On a Mac, what destroys is a command of the menu bar only.
                destructive: ActionBar.current == .window ? [] : actions.filter(Self.isDestructive).compactMap { node -> Navree.Button? in
                    if case .button(let button) = node { return button }
                    return nil
                }
            )
            .modifier(ListLook(layout: ContentRules.list(body)))
            .modifier(Editing(table: table, region: sections.first { $0.table != nil }?.region, tree: body, reorders: sections.contains { $0.list?.reorder != nil }))
        }
    }

    static func isDestructive(_ node: Navree.Node) -> Bool {
        if case .button(let button) = node { return button.role == .destructive || button.standard == .delete }
        return false
    }

    /// The empty state, when it is all the screen holds.
    static func wholeEmpty(_ body: Navree.Node) -> Navree.Empty? {
        switch body {
        case .empty(let empty): return empty
        case .stack(let stack):
            let parts = stack.children.filter { !OverlayPresenter.isOverlay($0) }
            return parts.count == 1 ? wholeEmpty(parts[0]) : nil
        case .region(let region): return region.children.count == 1 ? wholeEmpty(region.children[0]) : nil
        default: return nil
        }
    }
}

extension EnvironmentValues {
    /// Set on a screen open beside its list (and on what is pushed over it)
    /// in a window of columns: where a Mac window's one title is the list's.
    @Entry public var navreeBesideList: Bool = false
}

extension OverlayPresenter {
    static func isOverlay(_ node: Navree.Node) -> Bool {
        switch node {
        case .sheet, .dialog, .popover: true
        default: false
        }
    }
}

/// A screen's sections as the system's list. Its selection is the row open
/// beside it when it is the list of a list beside its detail (as Mail shows
/// the message open, and no other), and the rows ticked for an action on many
/// while it is being edited.
struct SectionsList: View {
    let tree: Navree.Node
    let sections: [ListSection]
    let notice: Notice?
    let table: Navree.Table?
    /// The screen's title, where its content says it at its head.
    let heading: String?
    let key: String
    let destructive: [Navree.Button]
    @Environment(\.navree) private var context
    @Environment(\.navreeChoose) private var choose
    @Environment(\.navreeOpen) private var open
    #if os(iOS)
    @Environment(\.editMode) private var editMode
    #endif

    var body: some View {
        List(selection: selection) {
            if let heading {
                Text(heading)
                    .font(.title2.weight(.bold))
                    .accessibilityAddTraits(.isHeader)
                    .listRowSeparator(.hidden)
            }
            if let notice {
                Section { NoticeRow(notice: notice) }
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
    }

    private var editing: Bool {
        #if os(iOS)
        editMode?.wrappedValue.isEditing ?? false
        #else
        false
        #endif
    }

    /// The row open beside the list, which only a choice in it changes; or
    /// the rows of a table ticked while it is edited, each change told to the
    /// core.
    private var selection: Binding<Set<String>>? {
        if choose != nil, !editing {
            let shown = open.flatMap { SplitRules.openRow(in: tree, opened: $0) }
            return Binding(get: { shown.map { [$0] } ?? [] }, set: { _ in })
        }
        guard let table, table.select != nil else { return nil }
        return Binding(
            get: { EditRules.chosen(table) },
            set: { next in
                for change in EditRules.changes(table, to: next) {
                    context.send(.choose(table: key, row: change.row, on: change.on, across: false))
                }
            }
        )
    }
}

/// A section of the list, with its heading in title case (never upper case)
/// and its footer.
struct SectionView: View {
    let section: ListSection
    @Environment(\.navree) private var context

    var body: some View {
        Section {
            if let list = section.list, let id = list.id, let ids = EditRules.movable(list) {
                // The list's rows, which the person drags into a new order.
                ForEach(Array(section.rows.prefix(ids.count).enumerated()), id: \.offset) { _, row in
                    RowView(item: row)
                }
                .onMove { offsets, destination in
                    if let move = EditRules.move(ids: ids, from: Array(offsets), to: destination) {
                        context.send(.move(item: move.item, from: id, to: id, before: move.before))
                    }
                }
                ForEach(Array(section.rows.dropFirst(ids.count).enumerated()), id: \.offset) { _, row in
                    RowView(item: row)
                }
            } else {
                ForEach(Array(section.rows.enumerated()), id: \.offset) { _, row in
                    RowView(item: row)
                }
            }
        } header: {
            if section.header != nil || section.subheader != nil {
                VStack(alignment: .leading, spacing: 2) {
                    if let header = section.header {
                        Text(header).textCase(nil)
                    }
                    if let subheader = section.subheader {
                        Text(subheader).font(.footnote).textCase(nil).foregroundStyle(.secondary)
                    }
                }
            }
        } footer: {
            if let footer = section.footer { Text(footer) }
        }
    }
}

/// A line of status at the top of the content, where Apple's apps show one: a
/// symbol in the tone's colour, the words, and a button to put it away.
public struct NoticeRow: View {
    let notice: Notice
    @Environment(\.navree) private var context

    public init(notice: Notice) {
        self.notice = notice
    }

    public var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: Tones.symbol(notice.tone))
                .foregroundStyle(Tones.color(notice.tone))
            VStack(alignment: .leading, spacing: 2) {
                if let title = notice.title { Text(title).font(.headline) }
                Text(notice.text).font(.subheadline).foregroundStyle(.secondary)
                if let action = notice.action, let label = notice.actionLabel {
                    Button(label) { context.send(.act(action, Place())) }
                        .buttonStyle(.borderless)
                        .font(.subheadline)
                        .padding(.top, 2)
                }
            }
            Spacer(minLength: 0)
            Button {
                context.send(.noticeDismissed)
            } label: {
                Label("Dismiss", systemImage: "xmark")
                    .labelStyle(.iconOnly)
            }
            .buttonStyle(.borderless)
            .tint(.secondary)
        }
        .accessibilityElement(children: .combine)
    }
}

/// What a table's query form offers: a search field, filters, sorts.
struct QueryForm {
    var form: FormRef
    var searchName: String?
    var searchValue: String
    var searchPrompt: String?
    var filters: [Navree.Field]
    var source: Navree.Form?
    var sorts: [Navree.Column]
    var region: String?

    var hasFilters: Bool { !filters.isEmpty }

    /// The query form holding only its filters: what the filters sheet draws.
    var filtersForm: Navree.Form {
        guard var form = source else { return Navree.Form() }
        form.children = filters.map(Navree.Node.field)
        return form
    }

    /// From the screen's own search, or the query form of its table.
    init?(screen: Navree.Screen, table: Navree.Table?, region: String?) {
        self.region = region
        sorts = table?.columns.filter { $0.sort != nil } ?? []
        if let query = table?.query {
            form = FormRef(query)
            source = query
            let fields = QueryRules.fields(query)
            let filters = fields.filters
            var name: String?
            var value = ""
            var prompt: String?
            if let field = fields.search {
                name = field.name
                if case .text(let text) = field.input {
                    value = text.value
                    prompt = text.placeholder
                }
            }
            self.filters = filters
            searchName = name
            searchValue = value
            searchPrompt = prompt
        } else if let search = screen.search {
            form = FormRef(key: "search", method: .get, action: search.action, hidden: search.hidden)
            source = nil
            filters = []
            searchName = search.name
            searchValue = search.value
            searchPrompt = search.placeholder
        } else if !sorts.isEmpty {
            form = .loose
            source = nil
            filters = []
            searchValue = ""
        } else {
            return nil
        }
    }
}

/// The table's sort as a menu and its filters as a sheet, in the toolbar.
struct QueryButtons: View {
    let query: QueryForm
    @State private var filtering = false
    @Environment(\.navree) private var context

    var body: some View {
        HStack {
            if !query.sorts.isEmpty {
                Menu {
                    ForEach(Array(query.sorts.enumerated()), id: \.offset) { _, column in
                        Button {
                            if let href = column.sort?.href {
                                context.send(.act(.navigate(Navree.Action.Navigate(href: href, present: .replace)), Place()))
                            }
                        } label: {
                            if let dir = column.sort?.dir {
                                Label(column.label, systemImage: dir == .asc ? "chevron.up" : "chevron.down")
                            } else {
                                Text(column.label)
                            }
                        }
                    }
                } label: {
                    Label("Sort", systemImage: "arrow.up.arrow.down")
                }
            }
            if query.hasFilters {
                Button {
                    filtering = true
                } label: {
                    Label("Filter", systemImage: "line.3.horizontal.decrease")
                }
            }
        }
        .sheet(isPresented: $filtering) {
            FiltersSheet(query: query)
        }
    }
}

/// The filters of a table, in a sheet: each field a section, applied with Done.
struct FiltersSheet: View {
    let query: QueryForm
    @Environment(\.dismiss) private var dismiss
    @Environment(\.navree) private var context

    var body: some View {
        NavigationStack {
            List {
                ForEach(SectionRules.sections(.form(query.filtersForm)).map(OverlayContent.withoutSubmits)) { section in
                    SectionView(section: section)
                }
            }
            .modifier(ListLook(layout: .insetGrouped))
            .navigationTitle("Filters")
            .navigationBarTitleDisplayModeInline()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    SheetDismissButton(.cancel) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm) {
                        context.send(.act(.submit(Navree.Action.Submit(form: query.form.key)), Place(form: query.form)))
                        dismiss()
                    } label: {
                        SheetButtonLabel("Apply", systemImage: "checkmark")
                    }
                    .sheetKey(.confirm)
                }
            }
            .environment(\.navreeForm, query.form)
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .modifier(SheetSizing())
    }
}

/// The search field, where the width puts it, sending the words as the reader
/// types (after a short pause) and when they submit.
struct SearchModifier: ViewModifier {
    let query: QueryForm?
    let placement: SearchPlacement
    @State private var text: String?
    /// Whether the screen has appeared, for a field that waits for it.
    @State private var appeared = false
    @Environment(\.navree) private var context

    func body(content: Content) -> some View {
        if query?.searchName != nil, PresentationRules.searchWaitsForFirstAppearance(placement), !appeared {
            content.onAppear {
                // After the bar's first layout, not within it.
                DispatchQueue.main.async { appeared = true }
            }
        } else if let query, let name = query.searchName {
            let binding = Binding(get: { text ?? query.searchValue }, set: { text = $0 })
            content
                .searchable(text: binding, placement: Self.placement(placement), prompt: query.searchPrompt ?? "Search")
                .onSubmit(of: .search) { send(query, name) }
                .modifier(FindFocus())
                .task(id: text) {
                    guard text != nil else { return }
                    try? await Task.sleep(for: .milliseconds(400))
                    if !Task.isCancelled { send(query, name) }
                }
        } else {
            content
        }
    }

    private func send(_ query: QueryForm, _ name: String) {
        context.send(.search(form: query.form, name: name, text: text ?? query.searchValue))
    }

    static func placement(_ placement: SearchPlacement) -> SearchFieldPlacement {
        switch placement {
        #if os(iOS)
        case .underTitle: .navigationBarDrawer(displayMode: .always)
        #else
        case .underTitle: .automatic
        #endif
        case .toolbar: .toolbar
        }
    }
}

/// The screen's subtitle: the system's, beside an inline title. Under a
/// large title on iPhone and iPad it is written in the system's own place for
/// it, but on as many lines as its words need: the system keeps it to one,
/// and at the largest text sizes cuts a date line short ("Saturday 14 Novem…").
struct Subtitle: ViewModifier {
    let text: String?
    /// Whether the place under a large title is the subtitle's (a day
    /// control may hold it).
    var underLargeTitle = false

    /// The type of the system's own subtitle under a large title, as iOS 27.2
    /// draws it (measured: 11 points, medium, the secondary colour); the place
    /// gives a view of its own no type.
    static let type = Font.caption2.weight(.medium)

    @Environment(\.dynamicTypeSize) private var typeSize

    func body(content: Content) -> some View {
        if let text {
            #if os(iOS)
            if underLargeTitle {
                content
                    .navigationSubtitle(text)
                    .toolbar {
                        ToolbarItem(placement: .largeSubtitle) {
                            Text(text)
                                .font(Self.type)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                                // As large as the person's text, as the system's
                                // subtitle grows: a bar holds its items' text smaller.
                                .dynamicTypeSize(typeSize)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
            } else {
                content.navigationSubtitle(text)
            }
            #else
            content.navigationSubtitle(text)
            #endif
        } else {
            content
        }
    }
}

struct TitleDisplay: ViewModifier {
    let mode: TitleMode

    func body(content: Content) -> some View {
        #if os(iOS)
        content.navigationBarTitleDisplayMode(mode == .large ? .large : .inline)
        #else
        content
        #endif
    }
}

struct Refresh: ViewModifier {
    let enabled: Bool
    @Environment(\.navree) private var context

    func body(content: Content) -> some View {
        if enabled {
            content.refreshable { context.send(.refresh) }
        } else {
            content
        }
    }
}

struct ListLook: ViewModifier {
    let layout: ListLayout

    func body(content: Content) -> some View {
        #if os(iOS)
        switch layout {
        case .plain: content.listStyle(.plain)
        case .insetGrouped: content.listStyle(.insetGrouped)
        }
        #else
        // A Mac's lists are inset, as Mail's messages and the sidebar are:
        // one leading edge for the list and its title, and the chosen row a
        // rounded shape inset from the column's edges.
        content.listStyle(.inset)
        #endif
    }
}

