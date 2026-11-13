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
/// has none.
public struct SettingsShell: View {
    @State private var session: Session

    public init(configuration: ShellConfiguration) {
        var configuration = configuration
        configuration.launch = Launch()
        _session = State(initialValue: Session(configuration: configuration))
    }

    public var body: some View {
        Group {
            if let client = session.client {
                SettingsContent(client: client, configuration: session.configuration)
            } else {
                ContentUnavailableView("No Settings", systemImage: "gearshape")
            }
        }
        .frame(minWidth: 480, idealWidth: 560, minHeight: 360, idealHeight: 520)
        .task {
            session.begin(restoring: false)
            session.window(.medium)
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
                StackView(stack: opened == href ? stack : [], client: client, configuration: configuration)
                    .modifier(WindowWidth(width: .medium))
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
