import CoreGraphics
import Foundation
import NavreeShell

/// The demo's windows and the icons it offers, as its launch arguments say:
/// `-NavreeWindowSize 1180x760` (a first window's size), `-NavreeMinimumSize
/// 760x480`, `-NavreeToolbarLabels iconOnly|titleAndIcon|titleOnly`,
/// `-NavreeTabs NO`. By default the system's own windows.
enum DemoWindows {
    static func make(_ defaults: UserDefaults) -> WindowLook {
        WindowLook(
            idealSize: size(defaults.string(forKey: "NavreeWindowSize")),
            minimumSize: size(defaults.string(forKey: "NavreeMinimumSize")),
            toolbarLabels: defaults.string(forKey: "NavreeToolbarLabels").flatMap(ToolbarLabels.init(rawValue:))
                ?? .automatic,
            tabs: defaults.object(forKey: "NavreeTabs") == nil || defaults.bool(forKey: "NavreeTabs")
        )
    }

    static func icons(_ defaults: UserDefaults) -> [AppIcon] {
        []
    }

    /// `1180x760` as a size in points.
    static func size(_ text: String?) -> CGSize? {
        let numbers = text?.split(separator: "x").compactMap { Double($0) } ?? []
        guard numbers.count == 2 else { return nil }
        return CGSize(width: numbers[0], height: numbers[1])
    }
}
