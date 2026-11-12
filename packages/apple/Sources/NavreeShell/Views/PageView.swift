import NavreeDesign
import NavreeProtocol
import SwiftUI

/// One screen of a stack: drawn natively when the server sent parts, in the
/// web view inside the native bars when it sent a page, and the system's own
/// layouts while it loads, when it fails, and when the app is too old.
struct PageView: View {
    let ref: PageRef
    let client: NativeClient

    var body: some View {
        let page = client.state.page(ref)
        content(page)
            .environment(\.navree, client.context(for: ref))
            .toolbar {
                if ref.index == 0, case .destination = ref.container, let user = client.state.shell?.user {
                    ToolbarItem(placement: .primaryAction) {
                        AccountMenu(user: user)
                            .environment(\.navree, client.context(for: ref))
                    }
                }
                if ref.index == 0, ref.container == .sheet || ref.container == .cover, !Self.hasCancel(page) {
                    ToolbarItem(placement: .cancellationAction) {
                        Button(role: .close) {
                            client.send(.dismissed(ref.container))
                        } label: {
                            Label("Close", systemImage: "xmark")
                        }
                    }
                }
            }
    }

    @ViewBuilder private func content(_ page: PageState?) -> some View {
        switch page?.content {
        case .screen(let screen)?:
            ScreenView(
                screen: screen, depth: depth, appName: client.state.shell?.name ?? "",
                notice: page?.notice, failure: page?.failure
            )
        case .web(let url, _)?:
            WebPageView(url: url, ref: ref, client: client)
                .ignoresSafeArea(.container, edges: .bottom)
                .navigationTitle(page?.title ?? "")
                .modifier(InlineTitle(depth: depth))
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        ShareLink(item: url) {
                            Label("Share", systemImage: Symbols.standard(.share) ?? "square.and.arrow.up")
                        }
                    }
                }
        case .failed(let message)?:
            ContentUnavailableView {
                Label("Couldn't Load This Page", systemImage: "wifi.exclamationmark")
            } description: {
                Text(message)
            } actions: {
                Button("Try Again") { client.send(.intent(.refresh, at: ref)) }
                    .buttonStyle(.bordered)
            }
        case .upgrade(let upgrade)?:
            ContentUnavailableView {
                Label("Update Required", systemImage: "arrow.down.app")
            } description: {
                Text("This version of the app is too old for the server. Version \(upgrade.minApp) or later is needed.")
            } actions: {
                if let url = URL(string: upgrade.url) {
                    Link("Update", destination: url).buttonStyle(.borderedProminent)
                }
            }
        case .loading?, nil:
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .navigationTitle(page?.title ?? "")
                .modifier(InlineTitle(depth: depth))
        }
    }

    private var depth: ScreenDepth {
        switch ref.container {
        case .destination: ref.index == 0 ? .root : .pushed
        case .sheet: ref.index == 0 ? .sheet : .pushed
        case .cover: ref.index == 0 ? .cover : .pushed
        }
    }

    static func hasCancel(_ page: PageState?) -> Bool {
        guard case .screen(let screen)? = page?.content else { return false }
        return screen.actions.contains { node in
            if case .button(let button) = node { return button.role == .cancel }
            return false
        }
    }
}

struct InlineTitle: ViewModifier {
    let depth: ScreenDepth

    func body(content: Content) -> some View {
        #if os(iOS)
        content.navigationBarTitleDisplayMode(NavigationRules.titleMode(depth) == .large ? .large : .inline)
        #else
        content
        #endif
    }
}

/// The person signed in, at the trailing end of a destination's first
/// screen: their name and what their menu offers (profile, sign out).
struct AccountMenu: View {
    let user: User
    @Environment(\.navree) private var context

    var body: some View {
        Menu {
            Section(user.email.map { "\(user.name)\n\($0)" } ?? user.name) {
                ForEach(Array(user.menu.enumerated()), id: \.offset) { _, item in
                    switch item {
                    case .link(let link):
                        Button(link.label) { context.send(.act(link.action, Place())) }
                    case .button(let button):
                        Button(role: button.role == .destructive ? .destructive : nil) {
                            if let action = button.action { context.send(.act(action, Place())) }
                        } label: {
                            Text(button.label)
                        }
                    default:
                        EmptyView()
                    }
                }
            }
        } label: {
            Label(user.name, systemImage: "person.crop.circle")
        }
    }
}
