import SwiftUI

/// What a main window tells the app about where it stands outside its
/// windows, and what it hears back. A window with no presence of its own to
/// tell is drawn as it is.
struct PresenceHooks: ViewModifier {
    let session: Session
    /// The window's own name.
    let scene: String

    func body(content: Content) -> some View {
        content
    }
}

extension Scene {
    /// The main windows as the app's place in the system needs them: a Dock
    /// app opens one at launch, as the system does for any app.
    public func navreePresence(_ presence: Presence) -> some Scene {
        self
    }
}
