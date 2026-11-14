import CoreGraphics

/// The app's main windows, as far as the system lets an app choose (Mac).
/// The frame, the title bar and the materials stay the system's.
public struct WindowLook: Sendable, Equatable {
    /// The size a first main window opens at; the content's own when nil.
    /// The system restores each window's own size after that.
    public var idealSize: CGSize?
    /// The smallest a main window may become; the content's own when nil.
    public var minimumSize: CGSize?
    /// Whether toolbar items show their symbols, their words or both, until the
    /// person chooses otherwise from the toolbar's menu.
    public var toolbarLabels: ToolbarLabels
    /// Whether main windows may be merged as tabs; when they may, the person's
    /// own setting decides.
    public var tabs: Bool

    public init(
        idealSize: CGSize? = nil, minimumSize: CGSize? = nil, toolbarLabels: ToolbarLabels = .automatic,
        tabs: Bool = true
    ) {
        self.idealSize = idealSize
        self.minimumSize = minimumSize
        self.toolbarLabels = toolbarLabels
        self.tabs = tabs
    }
}

/// How toolbar items are labelled: the system's `ToolbarLabelStyle`.
public enum ToolbarLabels: String, Sendable, Equatable, CaseIterable {
    /// As the system labels them.
    case automatic
    /// Their symbols.
    case iconOnly
    /// Their symbols with their words under them.
    case titleAndIcon
    /// Their words.
    case titleOnly
}

/// An icon the person may choose for the app (iOS, iPadOS).
public struct AppIcon: Sendable, Equatable, Identifiable {
    /// The alternate icon's name, as the build's alternate app icon sets name
    /// it (`AppIconIndigo`); nil for the primary icon.
    public var name: String?
    /// What the choice is called.
    public var label: String
    /// A picture of the icon in the app's assets, for the choice: the system
    /// draws no other icon than the current one.
    public var preview: String

    public var id: String { name ?? "" }

    public init(name: String?, label: String, preview: String) {
        self.name = name
        self.label = label
        self.preview = preview
    }
}
