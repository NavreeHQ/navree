import SwiftUI

#if os(macOS)
/// The app's item in the menu bar, as the configuration and the person's
/// choice say. An app with no status item is never put in the menu bar.
public struct NavreeStatusItem: Scene {
    let configuration: ShellConfiguration

    public init(configuration: ShellConfiguration) {
        self.configuration = configuration
    }

    public var body: some Scene {
        MenuBarExtra(isInserted: .constant(false)) {
            EmptyView()
        } label: {
            EmptyView()
        }
    }
}
#endif
