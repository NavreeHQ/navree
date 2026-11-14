import SwiftUI

#if os(macOS)
/// The app's item in the menu bar, as the configuration and the person's
/// choice say: a menu of the app's destinations, Settings… and Quit, opening
/// what is chosen in the main window. An app with no status item, or one in
/// the Dock only, has no item at all; the person may take the item out of the
/// menu bar (⌘-drag), and an app in both places then stays in the Dock while
/// the system ends an app in the menu bar only.
///
/// The item is the system's own `NSStatusItem`, made only while the app
/// stands in the menu bar: a scene cannot be declared or left out by a value
/// known at run time, and a `MenuBarExtra` left out of the menu bar still
/// makes an item. So the scene itself declares nothing.
///
/// An item showing a small window with one destination's screen
/// (`StatusContent.screen`) is not built yet: it needs one session shared by
/// the app's cores. Such an item stands nowhere and the app stays in the Dock.
public struct NavreeStatusItem: Scene {
    let configuration: ShellConfiguration

    public init(configuration: ShellConfiguration) {
        self.configuration = configuration
        PresenceModel.shared.configure(configuration.presence)
    }

    public var body: some Scene {}
}
#endif
