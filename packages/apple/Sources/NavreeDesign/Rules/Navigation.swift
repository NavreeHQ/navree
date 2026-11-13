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

/// How a window draws its destinations and the destination it shows.
public enum WindowLayout: Sendable, Equatable {
    /// The destinations as the system's tab view (a tab bar, or the sidebar it
    /// turns into), each destination's stack inside its tab.
    case tabs
    /// One split view of two columns: the destinations as its sidebar and the
    /// destination's stack as its detail.
    case sidebarStack
    /// One split view of three columns, as Mail's mailboxes, messages and
    /// message: the destinations as its sidebar, the destination's list as
    /// its content and what was chosen in it as its detail.
    case sidebarListDetail
}

public enum NavigationRules {
    /// Where the destinations are a sidebar that never turns into a tab bar
    /// (a window the system gives no size class: a Mac's), they are the
    /// sidebar of one split view whatever the destination shown, as Mail keeps
    /// its mailboxes: with the destination's list and its detail as two more
    /// columns when it is a list beside its detail (a split view inside the
    /// tab view's sidebar would inset the detail twice), else with its stack
    /// as the one other column. The sidebar is the same in both, so it keeps
    /// its width as the person moves between destinations. Where the
    /// destinations are a tab bar, or turn into one, they stay the system's
    /// tab view.
    public static func window(destinations: Int, sidebarOnly: Bool, stack: StackLayout) -> WindowLayout {
        guard sidebarOnly, destinations > 1 else { return .tabs }
        switch stack {
        case .stack: return .sidebarStack
        case .panes, .listBesideDetail: return .sidebarListDetail
        }
    }

    /// The tab bar on a compact width, the sidebar-adaptable tab view on a
    /// regular one; never chosen from the kind of device.
    /// Whether a wide window shows the shell's groups as sections of the
    /// sidebar: only when there is more than one, since a lone section would
    /// stand in the tab bar in place of its destinations.
    public static func groupsAsSections(_ groups: Int) -> Bool {
        groups > 1
    }

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

    /// Whether a link pushes a screen, and so shows the system's disclosure
    /// chevron: one that says `push`, or one that leaves it to the app when no
    /// route of the shell presents its address otherwise. A sheet, a task, a
    /// cover, a replacement or another app is no push.
    public static func pushes(_ action: Navree.Action?, routes: [Navree.Route]) -> Bool {
        guard case .navigate(let link)? = action else { return false }
        switch link.present {
        case .push: return true
        case .auto:
            let route = routes.first { matches($0.match, link.href) }
            return route?.present == nil || route?.present == .auto || route?.present == .push
        default: return false
        }
    }

    /// Whether a route's pattern names an address: segment by segment, `*`
    /// for any one, the query left out.
    public static func matches(_ pattern: String, _ address: String) -> Bool {
        var path = address
        if let scheme = path.range(of: "://") {
            let rest = path[scheme.upperBound...]
            path = rest.firstIndex(of: "/").map { String(rest[$0...]) } ?? "/"
        }
        path = String(path.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false).first ?? "")
        path = String(path.split(separator: "#", maxSplits: 1, omittingEmptySubsequences: false).first ?? "")
        let wanted = pattern.split(separator: "/")
        let given = path.split(separator: "/")
        return wanted.count == given.count && zip(wanted, given).allSatisfy { $0 == "*" || $0 == $1 }
    }
}
