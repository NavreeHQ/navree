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
    /// The window's own name.
    let scene: String
    @Environment(\.scenePhase) private var phase
    @Environment(\.openWindow) private var openWindow
    @State private var box = Box()

    func body(content: Content) -> some View {
        content
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

extension Scene {
    /// The main windows as the app's place in the system needs them: an app
    /// in the menu bar only opens none at launch; any other opens one, as the
    /// system does for any app.
    public func navreePresence(_ presence: Presence) -> some Scene {
        PresenceModel.shared.configure(presence)
        #if os(macOS)
        let launch = PresenceRules.launch(place: PresenceRules.place(presence, chosen: PresenceModel.storedPlace))
        return defaultLaunchBehavior(launch == .suppressed ? .suppressed : .automatic)
        #else
        return self
        #endif
    }
}
