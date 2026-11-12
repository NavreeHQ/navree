import NavreeProtocol

/// How the app's top-level destinations are drawn.
public enum NavigationStructure: Sendable, Equatable {
    /// One stack and no bar: an app with one destination or none.
    case single
    /// A tab bar: the destinations as tabs, at most `visible` of them in the
    /// bar (the system folds the rest into its own More tab).
    case tabBar(visible: Int)
    /// A tab bar that can turn into a sidebar, with the shell's groups as its
    /// sections: what a wide window gets.
    case sidebarAdaptable
}

/// Where a screen stands, which decides how its title is shown.
public enum ScreenDepth: Sendable, Equatable {
    /// The first screen of a destination.
    case root
    /// A screen pushed on top of another.
    case pushed
    /// The first screen of a sheet.
    case sheet
    /// A screen over everything, such as sign-in.
    case cover
}

/// How a screen's title is shown.
public enum TitleMode: Sendable, Equatable {
    case large
    case inline
}

public enum NavigationRules {
    /// The tab bar on a compact width, the sidebar-adaptable tab view on a
    /// regular one; never chosen from the kind of device.
    public static func structure(destinations: Int, width: WidthClass) -> NavigationStructure {
        guard destinations > 1 else { return .single }
        switch width {
        case .compact: return .tabBar(visible: min(destinations, 5))
        case .regular: return .sidebarAdaptable
        }
    }

    /// A large title at the root of each destination; an inline one on pushed
    /// screens and in sheets, where the title names the task.
    public static func titleMode(_ depth: ScreenDepth) -> TitleMode {
        depth == .root ? .large : .inline
    }

    /// A page in the web view has an inline title wherever it stands: the web
    /// view's own scrolling cannot fold a large title away, which would keep
    /// its room for ever.
    public static func webTitleMode(_ depth: ScreenDepth) -> TitleMode {
        .inline
    }

    /// The title a bar shows. The app's own name never stands as a screen's
    /// title; an empty title stays empty rather than naming the app.
    public static func title(_ title: String, appName: String) -> String {
        title == appName ? "" : title
    }
}
