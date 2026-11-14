import NavreeShell
import SwiftUI

/// Which of the demo's apps runs: the window toolbar's style is chosen in
/// code, so on a Mac each style is an app of its own, chosen with
/// `-NavreeToolbarStyle unified|compact|expanded|untitled` (by default the
/// system's own).
enum DemoStyles {
    static func run() {
        #if os(macOS)
        switch UserDefaults.standard.string(forKey: "NavreeToolbarStyle") {
        case "unified": StyledDemoApp<Unified>.main()
        case "compact": StyledDemoApp<Compact>.main()
        case "expanded": StyledDemoApp<Expanded>.main()
        case "untitled": StyledDemoApp<Untitled>.main()
        default: DemoApp.main()
        }
        #else
        DemoApp.main()
        #endif
    }
}

#if os(macOS)
/// A window toolbar style of the system's, named in code.
protocol ToolbarChoice {
    associatedtype Style: WindowToolbarStyle
    static var style: Style { get }
}

/// The title and the toolbar in one row.
enum Unified: ToolbarChoice {
    static var style: UnifiedWindowToolbarStyle { .unified }
}

/// The title and the toolbar in one shorter row.
enum Compact: ToolbarChoice {
    static var style: UnifiedCompactWindowToolbarStyle { .unifiedCompact }
}

/// The title above the toolbar.
enum Expanded: ToolbarChoice {
    static var style: ExpandedWindowToolbarStyle { .expanded }
}

/// The toolbar in one row, the title left out.
enum Untitled: ToolbarChoice {
    static var style: UnifiedWindowToolbarStyle { .unified(showsTitle: false) }
}

/// The demo with its windows' toolbar in one of the system's styles.
struct StyledDemoApp<Choice: ToolbarChoice>: App {
    /// What the system tells the app outside its windows, passed on to the shell.
    @NSApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
    private let configuration = DemoConfiguration.make()

    var body: some Scene {
        NavreeScenes(configuration: configuration, toolbarStyle: Choice.style) { $0.modifier(DemoPictures()) }
    }
}
#endif
