/// The app's chrome as data, fetched once from ``Wire/shellPath`` and again
/// when its stamp changes.
public struct ShellDocument: Sendable, Equatable, Decodable {
    public var navree: Int
    public var build: String
    /// The app's name.
    public var name: String
    /// A line under the name: the workspace, the environment.
    public var context: String?
    /// The address of the app's first screen.
    public var home: String
    public var theme: Theme?
    public var nav: Nav
    public var user: User?
    /// How the screens under an address are presented, the first match first.
    public var routes: [Route]
    public var device: Device?

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        navree = try c.field("navree", or: Wire.major)
        build = try c.field("build", or: "")
        name = try c.field("name", or: "")
        context = try c.optional("context")
        home = try c.field("home", or: "/")
        theme = try c.optional("theme")
        nav = try c.field("nav", or: Nav(groups: []))
        user = try c.optional("user")
        routes = try c.field("routes", or: [])
        device = try c.optional("device")
    }

    /// Every destination, in order, groups flattened.
    public var destinations: [NavItem] {
        nav.groups.flatMap(\.items)
    }
}

/// The app's destinations.
public struct Nav: Sendable, Equatable, Decodable {
    public var groups: [NavGroup]

    public init(groups: [NavGroup]) {
        self.groups = groups
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        groups = try c.field("groups", or: [])
    }
}

/// A group of destinations, with or without a label.
public struct NavGroup: Sendable, Equatable, Decodable {
    public var label: String?
    public var items: [NavItem]

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        label = try c.optional("label")
        items = try c.field("items", or: [])
    }
}

/// One destination of the app.
public struct NavItem: Sendable, Equatable, Decodable {
    public var label: String
    public var icon: Node.Icon?
    public var href: String
    /// A count beside it, in the page's words.
    public var badge: String?
    /// The destinations under it.
    public var items: [NavItem]

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        label = try c.field("label", or: "")
        icon = try c.optional("icon")
        href = try c.required("href")
        badge = try c.optional("badge")
        items = try c.field("items", or: [])
    }
}

/// The person signed in.
public struct User: Sendable, Equatable, Decodable {
    public var name: String
    public var email: String?
    /// Links and buttons.
    public var menu: [Node]

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        name = try c.field("name", or: "")
        email = try c.optional("email")
        menu = try c.field("menu", or: [])
    }
}

/// How the screens under an address are presented.
public struct Route: Sendable, Equatable, Decodable {
    /// The address, where `*` stands for one segment: `/orders/*/edit`.
    public var match: String
    public var present: Present?
    /// Whether the app's navigation stays around it. A sign-in screen has none.
    public var chrome: Bool

    public init(match: String, present: Present?, chrome: Bool = true) {
        self.match = match
        self.present = present
        self.chrome = chrome
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        match = try c.required("match")
        present = try c.optional("present")
        chrome = try c.field("chrome", or: true)
    }
}

/// The brand's part in the look: it tints the system's theme, never replaces it.
public struct Theme: Sendable, Equatable, Decodable {
    public var name: String?
    /// The accent, as `#rrggbb`, on a light ground.
    public var accent: String?
    /// The accent on a dark ground.
    public var accentDark: String?

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        name = try c.optional("name")
        accent = try c.optional("accent")
        accentDark = try c.optional("accent_dark")
    }
}

/// The device features the app uses.
public struct Device: Sendable, Equatable, Decodable {
    public var caps: [String]
    /// Where a device's push token is posted.
    public var pushRegister: String?

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        caps = try c.field("caps", or: [])
        if let push = try? c.nestedContainer(keyedBy: WireKey.self, forKey: WireKey("push")) {
            pushRegister = try push.optional("register")
        } else {
            pushRegister = nil
        }
    }
}
