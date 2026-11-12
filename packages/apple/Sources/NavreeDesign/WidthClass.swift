import SwiftUI

/// How much room a window gives across: the only thing, besides what a part
/// is, that decides where it goes. Never the kind of device: an iPhone app
/// can run in a window on an iPad or a Mac, and a folding phone has both.
public enum WidthClass: Sendable, Hashable {
    /// A phone held upright, a narrow window, a slide-over panel.
    case compact
    /// A tablet, a wide window, a phone's inner display.
    case regular

    /// The width class of a horizontal size class; none is read as compact.
    public init(_ sizeClass: UserInterfaceSizeClass?) {
        self = sizeClass == .regular ? .regular : .compact
    }
}
