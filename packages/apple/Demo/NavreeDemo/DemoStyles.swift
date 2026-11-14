import SwiftUI

/// Which of the demo's apps runs: the window toolbar's style is chosen in
/// code, so each style is an app of its own.
enum DemoStyles {
    static func run() {
        DemoApp.main()
    }
}
