import Foundation
import NavreeShell

/// The demo's windows and the icons it offers, as its launch arguments say:
/// by default the system's own windows and no choice of icon.
enum DemoWindows {
    static func make(_ defaults: UserDefaults) -> WindowLook {
        WindowLook()
    }

    static func icons(_ defaults: UserDefaults) -> [AppIcon] {
        []
    }
}
