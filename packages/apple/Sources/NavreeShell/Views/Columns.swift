import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import SwiftUI

/// A destination's stack drawn as a list beside its detail, in the system's
/// split view, bound to the core's stack as a navigation stack is.
///
/// From the server's panes: the first screen's list in the leading column,
/// its detail (and what is pushed from it) in the other, its inspector beside
/// that; a row chosen in the list asks the core for the screen of panes for
/// that row, in place of this one.
///
/// From rows that open screens: the first screen in the leading column, the
/// screen a row opened in the other, and what is pushed from that over it; a
/// row chosen in the list takes the place of the one open.
///
/// With a `sidebar` (the app's destinations, where the window's layout says
/// so), the sidebar leads as a third column.
struct ColumnsStack<Sidebar: View>: View {
    let layout: StackLayout
    let stack: [ScreenID]
    let client: NativeClient
    let configuration: ShellConfiguration
    var sidebar: Sidebar?

    var body: some View {
        if let root = stack.first {
            switch layout {
            case .panes(let panes):
                SplitColumns(
                    path: path(StackPath.path(stack), stack: stack),
                    compactColumn: SplitRules.compactColumn(panes),
                    inspectorTitle: panes.inspector?.title,
                    choose: { action, place in client.perform(.act(SplitRules.choice(action), place), on: root) },
                    sidebar: sidebar,
                    list: {
                        ColumnPage(id: root, part: .list, client: client, configuration: configuration)
                            .environment(\.navreeOpen, client.screen(root)?.url)
                    },
                    detail: { PageView(id: root, depth: 1, modal: nil, client: client, configuration: configuration, part: .detail) },
                    inspector: panes.inspector == nil ? nil : {
                        ColumnPage(id: root, part: .inspector, client: client, configuration: configuration)
                    },
                    pushed: pushed
                )
            case .listBesideDetail:
                let detail = Array(stack.dropFirst())
                SplitColumns(
                    path: path(StackPath.path(detail), stack: detail),
                    compactColumn: detail.isEmpty ? .list : .detail,
                    choose: { action, place in choose(action, place, root: root, open: detail.first) },
                    sidebar: sidebar,
                    list: {
                        PageView(id: root, depth: 0, modal: nil, client: client, configuration: configuration)
                            .environment(\.navreeOpen, detail.first.flatMap { client.screen($0)?.url })
                    },
                    detail: {
                        if let first = detail.first {
                            PageView(id: first, depth: 1, modal: nil, client: client, configuration: configuration)
                        } else {
                            // The window keeps the list's name while nothing is open beside it.
                            NothingChosen()
                                .navigationTitle(client.screen(root)?.screen?.title ?? client.screen(root)?.title ?? "")
                        }
                    },
                    inspector: nil as (() -> EmptyView)?,
                    pushed: pushed
                )
            case .stack:
                EmptyView()
            }
        }
    }

    /// A screen pushed in the detail column.
    @ViewBuilder private func pushed(_ entry: PathEntry) -> some View {
        if case .screen(let id) = entry {
            PageView(
                id: id, depth: stack.firstIndex(of: id) ?? 1, modal: nil, client: client, configuration: configuration
            )
        }
    }

    /// The detail column's path over its root, as the core's stack has it;
    /// a link tapped in the detail asks the core, the system's back pops.
    private func path(_ shown: [PathEntry], stack: [ScreenID]) -> Binding<[PathEntry]> {
        Binding(
            get: { shown },
            set: { next in
                switch StackPath.change(stack, to: next) {
                case .tap(let action, let place, let screen)?:
                    client.perform(.act(action, place), on: screen)
                case .popTo(let screen)?:
                    client.send(.popTo(screen: screen))
                case nil:
                    break
                }
            }
        )
    }

    /// A row chosen in the list: the screen open beside it goes, and the one
    /// the row opens takes its place. Choosing the row already open does nothing.
    private func choose(_ action: Navree.Action, _ place: Place, root: ScreenID, open: ScreenID?) {
        if let open, case .navigate(let link) = action, let shown = client.screen(open),
            ColumnRules.same(link.href, shown.url)
        {
            return
        }
        if open != nil { client.send(.popTo(screen: root)) }
        client.perform(.act(action, place), on: root)
    }
}

extension ColumnsStack where Sidebar == EmptyView {
    /// A list beside its detail inside its destination's tab.
    init(layout: StackLayout, stack: [ScreenID], client: NativeClient, configuration: ShellConfiguration) {
        self.init(layout: layout, stack: stack, client: client, configuration: configuration, sidebar: nil)
    }
}

/// The app's destinations as a split view's sidebar, as Mail lists its
/// mailboxes: each with its symbol and count, the shell's groups as sections
/// where there is more than one, the one shown chosen, and the person's
/// account at its foot.
struct DestinationSidebar: View {
    let client: NativeClient

    var body: some View {
        let items = client.nav.destinations
        List(selection: selection) {
            if NavigationRules.groupsAsSections(Set(items.compactMap(\.group)).count) {
                ForEach(DestinationGroup.of(items)) { group in
                    if group.name.isEmpty {
                        Section { rows(group.members) }
                    } else {
                        Section(group.name) { rows(group.members) }
                    }
                }
            } else {
                rows(items.enumerated().map { DestinationGroup.Member(index: $0.offset, item: $0.element) })
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            #if os(macOS)
            if let user = client.app?.user {
                AccountMenu(user: user, client: client)
                    .menuStyle(.button)
                    .buttonStyle(.borderless)
                    .labelStyle(.titleAndIcon)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 10)
            }
            #endif
        }
    }

    private func rows(_ members: [DestinationGroup.Member]) -> some View {
        ForEach(members) { member in
            DestinationLabel(label: member.item.label, icon: member.item.icon)
                .badge(member.item.badge.flatMap { Int($0) } ?? 0)
                .tag(member.index)
        }
    }

    private var selection: Binding<Int?> {
        Binding(get: { client.nav.selected }, set: { index in
            if let index, index != client.nav.selected { client.send(.select(destination: index)) }
        })
    }
}

/// Whether two addresses name the same screen: the same path, whatever the
/// origin written before it.
public enum ColumnRules {
    public static func same(_ href: String, _ url: String) -> Bool {
        path(href) == path(url)
    }

    static func path(_ address: String) -> String {
        var rest = Substring(address)
        if let scheme = rest.range(of: "://") {
            let after = rest[scheme.upperBound...]
            rest = after.firstIndex(of: "/").map { after[$0...] } ?? "/"
        }
        return String(rest.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false).first ?? "/")
    }
}

/// The list or the inspector of a screen of panes: the pane's parts as a
/// screen of their own, reaching the core as the screen does. The screen's
/// questions and messages stand over its detail, never twice.
struct ColumnPage: View {
    let id: ScreenID
    let part: SplitPart
    let client: NativeClient
    let configuration: ShellConfiguration

    var body: some View {
        if let state = client.screens[id]?.state, let screen = state.screen,
            let panes = SplitRules.panes(screen.body)
        {
            let drawn = Drawable.screen(screen, states: state.states)
            ScreenView(
                screen: SplitRules.screen(part, of: drawn, panes: panes), depth: part == .list ? .root : .pushed,
                appName: client.app?.name ?? ""
            )
            .environment(\.navree, client.context(for: id, depth: part == .list ? 0 : 1, base: configuration.base))
            .toolbar {
                #if !os(macOS)
                if part == .list, state.chrome, let user = client.app?.user {
                    ToolbarItem(placement: .primaryAction) {
                        AccountMenu(user: user, client: client)
                    }
                }
                #endif
            }
        } else {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}
