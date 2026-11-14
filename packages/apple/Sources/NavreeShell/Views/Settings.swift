import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import SwiftUI

/// The person's settings in a window of their own: what a Mac's Settings
/// window (App menu > Settings…, ⌘,) shows, drawn from the screen the shell's
/// user menu leads to for settings, never a page in the main window.
///
///     Settings { SettingsShell(configuration: configuration) }
///
/// It runs on a runtime core of its own, which opens that screen as the first
/// of its destination; an app whose user menu has no settings shows that it
/// has none. Where the app offers settings of its own on this device (where
/// it stands, opening at login), they are a General pane before the server's,
/// each pane a button of the window's toolbar, the last one shown shown again.
public struct SettingsShell: View {
    @State private var session: Session
    @AppStorage("navree.settings.pane") private var pane: SettingsPane = .general

    public init(configuration: ShellConfiguration) {
        var configuration = configuration
        configuration.launch = Launch()
        _session = State(initialValue: Session(configuration: configuration))
    }

    public var body: some View {
        let panes = SettingsRules.panes(presence: session.configuration.presence, hasServerSettings: hasServerSettings)
        Group {
            if panes.count > 1 {
                TabView(selection: shown(panes)) {
                    ForEach(panes) { pane in
                        Tab(title(pane), systemImage: symbol(pane), value: pane) {
                            content(pane)
                        }
                    }
                }
            } else {
                content(panes.first ?? .server)
            }
        }
        .frame(minWidth: 480, idealWidth: 560, minHeight: 360, idealHeight: 520)
        .task {
            session.begin(restoring: false, scene: "settings")
            session.window(.medium)
        }
    }

    @ViewBuilder private func content(_ pane: SettingsPane) -> some View {
        switch pane {
        case .general:
            GeneralSettingsPane(configuration: session.configuration)
        case .server:
            if let client = session.client {
                SettingsContent(client: client, configuration: session.configuration)
            } else {
                ContentUnavailableView("No Settings", systemImage: "gearshape")
            }
        }
    }

    /// The pane shown: the one last shown, while the window still has it.
    private func shown(_ panes: [SettingsPane]) -> Binding<SettingsPane> {
        Binding(get: { panes.contains(pane) ? pane : panes.first ?? .server }, set: { pane = $0 })
    }

    /// Whether the person's menu leads to a settings screen of the server's.
    private var hasServerSettings: Bool {
        guard let user = session.client?.app?.user else { return false }
        return SettingsRules.destination(in: user.menu) != nil
    }

    /// A pane's name, which the window takes as its title: the server's pane
    /// is named as the person's menu names it.
    private func title(_ pane: SettingsPane) -> String {
        switch pane {
        case .general:
            return String(localized: "General")
        case .server:
            guard let user = session.client?.app?.user, let found = SettingsRules.destination(in: user.menu) else {
                return String(localized: "Account")
            }
            switch user.menu[found.index] {
            case .link(let link): return link.label
            case .button(let button): return button.label
            default: return String(localized: "Account")
            }
        }
    }

    private func symbol(_ pane: SettingsPane) -> String {
        switch pane {
        case .general: "gearshape"
        case .server: "person.crop.circle"
        }
    }
}

/// The settings screen once the app is ready, in a stack of its own.
struct SettingsContent: View {
    let client: NativeClient
    let configuration: ShellConfiguration
    @State private var opened: String?

    var body: some View {
        switch client.nav.stage {
        case .ready:
            if let href = SettingsRules.destination(in: client.app?.user?.menu ?? [])?.href {
                let nav = client.nav
                let stack = nav.destinations.indices.contains(nav.selected) ? nav.destinations[nav.selected].stack : []
                ZStack {
                    if opened == href, !stack.isEmpty {
                        StackView(stack: stack, client: client, configuration: configuration)
                            .modifier(WindowWidth(width: .medium))
                    } else {
                        ProgressView()
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .task(id: href) {
                    guard opened != href else { return }
                    opened = href
                    client.send(.act(.init(screen: nil, action: .navigate(.init(href: href, present: .root)))))
                }
            } else {
                ContentUnavailableView(
                    "No Settings", systemImage: "gearshape",
                    description: Text("This app has no settings of its own.")
                )
            }
        case .failed:
            let words = Words.failure(client.nav.failure)
            ContentUnavailableView {
                Label(words.title, systemImage: "wifi.exclamationmark")
            } description: {
                Text(words.text)
            } actions: {
                Button("Try Again") { client.send(.start) }
            }
        default:
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}
