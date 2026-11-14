import SwiftUI

/// Where the app's accent colour goes. Apple keeps it for the one prominent
/// action, what is chosen and links; the items of a bar are drawn without
/// colour on the glass, as the system draws Back and the overflow menu, so
/// the one tinted item stands out. On a Mac the person's own accent colour
/// is the one every control takes.
public enum TintRules {
    /// Whether the app's own accent is the tint of its windows: on iPhone
    /// and iPad, yes; on a Mac, the accent the person chose in System
    /// Settings wins.
    public static func appliesAppAccent(bar: ActionBar) -> Bool {
        bar == .navigation
    }

    /// Whether an item in a bar keeps the accent: only the screen's one main
    /// act and a sheet's confirming one, which the system draws prominent.
    /// Making something new, sorting, filtering, the account, Edit and the
    /// rest are drawn in the bar's own colour.
    public static func keepsAccent(_ slot: Slot) -> Bool {
        switch slot {
        case .primary, .confirm: true
        case .create, .cancel, .overflow, .destructiveSection, .leading, .bar, .window, .menu: false
        }
    }
}

/// An item of a bar drawn in the bar's own colour, unless it is the one the
/// accent marks (see ``TintRules/keepsAccent(_:)``). A Mac's toolbar draws
/// its items itself.
public struct BarItemLook: ViewModifier {
    let slot: Slot

    public init(_ slot: Slot) {
        self.slot = slot
    }

    public func body(content: Content) -> some View {
        #if os(iOS)
        if TintRules.keepsAccent(slot) {
            content
        } else {
            content.tint(Color.primary)
        }
        #else
        content
        #endif
    }
}
