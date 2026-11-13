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
        self.screen = screen
        self.depth = depth
        self.appName = appName
        self.notice = notice
        self.failure = failure
    }

    public var body: some View {
        let sections = SectionRules.sections(screen.body)
        let width = WidthClass(sizeClass)
        let table = sections.compactMap(\.table).first
        let query = QueryForm(screen: screen, table: table, region: sections.first { $0.table != nil }?.region)
        content(sections: sections, width: width, table: table)
            .navigationTitle(NavigationRules.title(screen.title, appName: appName))
            .modifier(Subtitle(text: screen.subtitle))
            .modifier(TitleDisplay(mode: NavigationRules.titleMode(depth)))
            .toolbar {
                PlacedActions(actions: screen.actions.filter { !Self.isDestructive($0) }, depth: depth, overlay: nil, form: nil)
                if let query, query.hasFilters || !query.sorts.isEmpty {
                    ToolbarItem(placement: .primaryAction) {
                        QueryButtons(query: query)
                    }
                }
            }
            .modifier(SearchModifier(query: query, placement: PresentationRules.search(width: width)))
            .modifier(Refresh(enabled: screen.refresh))
            .modifier(OverlayPresenter(tree: screen.body))
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
        if let empty = Self.wholeEmpty(screen.body) {
            EmptyPart(empty: empty)
        } else if let table, sections.count == 1, ContentRules.table(table, width: width) == .columns {
            TableColumnsView(table: table, region: sections[0].region)
        } else {
            List {
                if let notice {
                    Section { NoticeRow(notice: notice) }
                }
                ForEach(sections) { section in
                    SectionView(section: section)
                }
                let destructive = screen.actions.filter(Self.isDestructive)
                if !destructive.isEmpty {
                    Section {
                        ForEach(Array(destructive.enumerated()), id: \.offset) { _, node in
                            if case .button(let button) = node { ButtonRow(button: button) }
                        }
                    }
                }
            }
            .modifier(ListLook(layout: ContentRules.list(screen.body)))
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

    var body: some View {
        Section {
            ForEach(Array(section.rows.enumerated()), id: \.offset) { _, row in
                RowView(item: row)
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
struct NoticeRow: View {
    let notice: Notice
    @Environment(\.navree) private var context

    var body: some View {
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
            var filters: [Navree.Field] = []
            var name: String?
            var value = ""
            var prompt: String?
            query.walkFields { node in
                guard case .field(let field) = node else { return }
                if case .text(let text) = field.input, text.kind == .search {
                    name = field.name
                    value = text.value
                    prompt = text.placeholder
                } else if !Self.isPageSize(field) {
                    filters.append(field)
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

    /// A choice of how many rows a page holds: lists load by cursor here, so
    /// it means nothing natively. Read from its shape: a single choice whose
    /// options are all numbers.
    static func isPageSize(_ field: Navree.Field) -> Bool {
        guard case .choice(let choice) = field.input, !choice.options.isEmpty else { return false }
        return choice.options.allSatisfy { Int($0.value) != nil }
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

