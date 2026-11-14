import CoreGraphics
import SwiftUI

#if os(macOS)
import AppKit
#endif

/// What the app's choices among the system's window options are in the
/// system's own values.
public enum WindowLookRules {
    /// The size a new main window opens at when the system has kept none of
    /// its own for it: the app's ideal size, never below its minimum. Nil
    /// leaves it to the system.
    public static func firstSize(_ look: WindowLook) -> CGSize? {
        guard let ideal = look.idealSize else { return nil }
        let minimum = look.minimumSize ?? .zero
        return CGSize(width: max(ideal.width, minimum.width), height: max(ideal.height, minimum.height))
    }

    /// Whether main windows may be merged as tabs: as the person's own setting
    /// says, unless the app says they may not.
    public static func allowsTabs(_ look: WindowLook) -> Bool {
        look.tabs
    }

    #if os(macOS)
    /// What the app chose that the system holds for the whole app, said to it
    /// as the app starts.
    @MainActor
    static func applyAtLaunch(_ look: WindowLook) {
        if !allowsTabs(look) { NSWindow.allowsAutomaticWindowTabbing = false }
    }
    #endif
}

extension Scene {
    /// The main windows as the app chose them among the system's options:
    /// the size a new one opens at, and whether they join as tabs. The frame,
    /// the title bar and the materials stay the system's.
    public func navreeWindowLook(_ look: WindowLook) -> some Scene {
        #if os(macOS)
        WindowLookScene(content: self, look: look)
        #else
        self
        #endif
    }
}

#if os(macOS)
/// The main windows' scene: a new window opens at the app's first size (the
/// system's own when it has none), a window the system restores keeps its
/// own; and what the app chose for the whole app is said to the system
/// before the first window opens.
struct WindowLookScene<Content: Scene>: Scene {
    let content: Content
    let look: WindowLook

    init(content: Content, look: WindowLook) {
        self.content = content
        self.look = look
        WindowLookRules.applyAtLaunch(look)
    }

    var body: some Scene {
        content.defaultWindowPlacement { [look] _, _ in
            let size = WindowLookRules.firstSize(look)
            return WindowPlacement(x: nil, y: nil, width: size?.width, height: size?.height)
        }
    }
}
#endif
