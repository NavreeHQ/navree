import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import SwiftUI

/// What a main window tells the app's place in the system (its name, its
/// destinations, its person, whether it is in front), and what it hears back
/// (an address opened from outside the windows). It draws nothing.
struct PresenceHooks: ViewModifier {
    let session: Session
    /// The window's own name.
    let scene: String
    @Environment(\.scenePhase) private var phase

    func body(content: Content) -> some View {
        content
            .onChange(of: scene, initial: true) { _, scene in
                guard !scene.isEmpty else { return }
                PresenceModel.shared.attach(scene: scene, session: session)
                PresenceModel.shared.heard(scene: scene, destinations: mirror.destinations, user: mirror.user)
            }
            .onChange(of: mirror) { _, mirror in
                guard !scene.isEmpty else { return }
                PresenceModel.shared.heard(scene: scene, destinations: mirror.destinations, user: mirror.user)
            }
            .onChange(of: phase) { _, phase in
                if phase == .active, !scene.isEmpty { PresenceModel.shared.show(scene) }
            }
            .onDisappear {
                if !scene.isEmpty { PresenceModel.shared.detach(scene: scene) }
            }
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
}

extension Scene {
    /// The main windows as the app's place in the system needs them: a Dock
    /// app opens one at launch, as the system does for any app.
    public func navreePresence(_ presence: Presence) -> some Scene {
        PresenceModel.shared.configure(presence)
        return self
    }
}
