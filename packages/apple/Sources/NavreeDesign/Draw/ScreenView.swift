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

    public init(screen: Navree.Screen, depth: ScreenDepth, appName: String = "", notice: Notice? = nil, failure: Notice? = nil) {
        self.screen = PagerRules.lifted(screen)
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
        let sections = SectionRules.sections(screen.body).map { lifted.confirms.isEmpty ? $0 : OverlayContent.withoutSubmits($0) }
        let width = WidthClass(sizeClass)
        let table = sections.compactMap(\.table).first
        let query = QueryForm(screen: screen, table: table, region: sections.first { $0.table != nil }?.region)
        content(sections: sections, width: width, table: table)
            .navigationTitle(NavigationRules.title(screen.title, appName: appName))
            .modifier(Subtitle(text: screen.subtitle))
            .modifier(TitleDisplay(mode: PagerRules.titleMode(depth, pager: screen.pager)))
            .modifier(PagerInBar(pager: screen.pager))
            .toolbar {
                PlacedActions(actions: actions.filter { !Self.isDestructive($0) }, depth: depth, overlay: nil, form: nil)
                if !lifted.confirms.isEmpty {
                    PlacedActions(
                        actions: lifted.confirms.map(Navree.Node.button), depth: depth, overlay: nil,
                        form: lifted.form.map(FormRef.init)
                    )
                }
                let foot = ActionRules.footToolbars(screen.body)
                if !foot.isEmpty { FootToolbar(toolbars: foot) }
                if let query, query.hasFilters || !query.sorts.isEmpty {
                    ToolbarItem(placement: .primaryAction) {
                        QueryButtons(query: query)
                    }
                }
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

    @ViewBuilder private func content(sections: [ListSection], width: WidthClass, table: Navree.Table?) -> some View {
        let panes = PaneRules.panes(in: screen.body)
        if let empty = Self.wholeEmpty(screen.body) {
            EmptyPart(empty: empty)
        } else if !panes.isEmpty {
            PaneScreenBody(panes: panes)
        } else if CompositionRules.isComposed(screen.body) {
            ComposedScreenBody(body: screen.body)
        } else if let table, sections.count == 1, ContentRules.table(table, width: width) == .columns {
            TableColumnsView(table: table, region: sections[0].region)
        } else {
            List(selection: selection(table)) {
                if let notice {
                    Section { NoticeRow(notice: notice) }
                }
                ForEach(sections) { section in
                    SectionView(section: section)
                }
                let destructive = actions.filter(Self.isDestructive)
                if !destructive.isEmpty {
                    let buttons = destructive.compactMap { node -> Navree.Button? in
                        if case .button(let button) = node { return button }
                        return nil
                    }
                    Section {
                        ForEach(Array(buttons.enumerated()), id: \.offset) { _, button in
                            ButtonRow(button: button, inFootedSection: true)
                        }
                    } footer: {
                        if let footer = ActionRules.footer(buttons) { Text(footer) }
                    }
                }
            }
            .modifier(ListLook(layout: ContentRules.list(screen.body)))
            .modifier(Editing(table: table, region: sections.first { $0.table != nil }?.region, tree: screen.body, reorders: sections.contains { $0.list?.reorder != nil }))
        }
    }

    /// The rows chosen of a table whose rows can be chosen: what the list
    /// shows ticked in edit mode, each change told to the core.
    private func selection(_ table: Navree.Table?) -> Binding<Set<String>>? {
        guard let table, table.select != nil else { return nil }
        let key = table.id ?? sectionsRegion ?? ""
        return Binding(
            get: { EditRules.chosen(table) },
            set: { next in
                for change in EditRules.changes(table, to: next) {
                    context.send(.choose(table: key, row: change.row, on: change.on, across: false))
                }
            }
        )
    }

    private var sectionsRegion: String? {
        SectionRules.sections(screen.body).first { $0.table != nil }?.region
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

extension OverlayPresenter {
    static func isOverlay(_ node: Navree.Node) -> Bool {
        switch node {
        case .sheet, .dialog, .popover: true
        default: false
        }
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
                    Button(role: .cancel) { dismiss() } label: { Label("Cancel", systemImage: "xmark") }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm) {
                        context.send(.act(.submit(Navree.Action.Submit(form: query.form.key)), Place(form: query.form)))
                        dismiss()
                    } label: {
                        Label("Apply", systemImage: "checkmark")
                    }
                }
            }
            .environment(\.navreeForm, query.form)
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }
}

/// The search field, where the width puts it, sending the words as the reader
/// types (after a short pause) and when they submit.
struct SearchModifier: ViewModifier {
    let query: QueryForm?
    let placement: SearchPlacement
    @State private var text: String?
    @Environment(\.navree) private var context

    func body(content: Content) -> some View {
        if let query, let name = query.searchName {
            let binding = Binding(get: { text ?? query.searchValue }, set: { text = $0 })
            content
                .searchable(text: binding, placement: Self.placement(placement), prompt: query.searchPrompt ?? "Search")
                .onSubmit(of: .search) { send(query, name) }
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

struct Subtitle: ViewModifier {
    let text: String?

    func body(content: Content) -> some View {
        if let text {
            content.navigationSubtitle(text)
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
        switch layout {
        case .plain: content.listStyle(.plain)
        case .insetGrouped: content.listStyle(.inset)
        }
        #endif
    }
}

