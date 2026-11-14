import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import SwiftUI

#if os(macOS)
import AppKit
#endif

/// What a main window tells the app's place in the system (its name, its
/// destinations, its person, whether it is in front, on a Mac its system
/// window), and what it hears back: an address opened from outside the
/// windows while it had none. It draws nothing.
struct PresenceHooks: ViewModifier {
    let session: Session
    /// The window's scene's name, which a shell outside a scene (the one an
    /// item in the menu bar holds) does not have.
    let scene: String
    @Environment(\.scenePhase) private var phase
    @Environment(\.openWindow) private var openWindow
    @State private var box = Box()
    /// The shell's own name in the presence, wherever it stands.
    @State private var name = UUID().uuidString

    func body(content: Content) -> some View {
        let scene = name
        return content
            .onChange(of: scene, initial: true) { _, scene in
                guard !scene.isEmpty else { return }
                let model = PresenceModel.shared
                model.keep(openWindow)
                model.attach(scene: scene, session: session)
                model.heard(scene: scene, destinations: mirror.destinations, user: mirror.user)
                box.scene = scene
                #if os(macOS)
                if let window = box.window { model.found(window, scene: scene) }
                #endif
            }
            .onChange(of: mirror) { _, mirror in
                guard !scene.isEmpty else { return }
                PresenceModel.shared.heard(scene: scene, destinations: mirror.destinations, user: mirror.user)
            }
            .onChange(of: session.client?.nav.stage, initial: true) { _, stage in
                // An address opened while no window was open, taken by the
                // first window ready for it.
                guard stage == .ready, let address = PresenceModel.shared.takePending() else { return }
                session.client?.send(.openLink(url: address))
            }
            .onChange(of: phase) { _, phase in
                if phase == .active, !scene.isEmpty { PresenceModel.shared.show(scene) }
            }
            .onAppear {
                // Back in a window (the shell an item in the menu bar holds moves
                // between its panel and a full window).
                guard !scene.isEmpty else { return }
                PresenceModel.shared.attach(scene: scene, session: session)
                PresenceModel.shared.heard(scene: scene, destinations: mirror.destinations, user: mirror.user)
            }
            .onDisappear {
                if !scene.isEmpty { PresenceModel.shared.detach(scene: scene) }
            }
            #if os(macOS)
            .background(WindowReader { window in
                box.window = window
                if !box.scene.isEmpty { PresenceModel.shared.found(window, scene: box.scene) }
            })
            #endif
    }

    /// What the window's core holds that the presence mirrors.
    private var mirror: Mirror {
        guard let client = session.client else { return Mirror() }
        return Mirror(
            destinations: client.nav.destinations.map { destination in
                PresenceDestination(
                    label: destination.label, href: destination.href,
                    symbol: destination.icon.flatMap(Symbols.symbol(for:)), badge: destination.badge
                )
            },
            user: client.app?.user
        )
    }

    struct Mirror: Equatable {
        var destinations: [PresenceDestination] = []
        var user: Navree.User?
    }

    /// The window's name and system window, as each becomes known.
    @MainActor
    final class Box {
        var scene = ""
        #if os(macOS)
        weak var window: NSWindow?
        #endif
    }
}

#if os(macOS)
/// Hands over the system window a view stands in, once it is in one.
struct WindowReader: NSViewRepresentable {
    let found: @MainActor (NSWindow) -> Void

    func makeNSView(context: Context) -> Reader {
        let view = Reader()
        view.found = found
        return view
    }

    func updateNSView(_ view: Reader, context: Context) {
        view.found = found
    }

    final class Reader: NSView {
        var found: (@MainActor (NSWindow) -> Void)?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window { found?(window) }
        }
    }
}
#endif

extension EnvironmentValues {
    /// Whether the shell stands in the window under an item in the menu bar
    /// (a Mac's): a compact window, laid out by its width as one.
    @Entry var navreeCompactWindow = false
}

extension Scene {
    /// The main windows as the app's place in the system needs them: an app
    /// in the menu bar only opens none at launch; any other opens one, as the
    /// system does for any app.
    public func navreePresence(_ presence: Presence) -> some Scene {
        PresenceModel.shared.configure(presence)
        #if os(macOS)
        let place = PresenceRules.place(presence, chosen: PresenceModel.storedPlace)
        let launch = PresenceRules.launch(place: place)
        // An app held by its item in the menu bar has one session, there: no
        // main window of another is brought back from before.
        let restores = !(place == .menuBar && StatusRules.holdsApp(presence.statusItem))
        return defaultLaunchBehavior(launch == .suppressed ? .suppressed : .automatic)
            .restorationBehavior(restores ? .automatic : .disabled)
        #else
        return self
        #endif
    }
}
