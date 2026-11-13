import NavreeProtocol
import SwiftUI

/// SF Symbols for what the server names, from the protocol's own table: an
/// icon's `symbol` when the server filled it, else the table's symbol for the
/// icon's name, else none (the icon's own shape is drawn). A standard action
/// always takes the system's own symbol.
public enum Symbols {
    /// The symbol Apple's guidelines give each standard action, as the protocol's table names it.
    public static func standard(_ action: Navree.Standard) -> String? {
        ProtocolSymbols.standard[action]?.apple
    }

    /// The symbol a role implies when the action has no icon of its own.
    public static func forRole(_ role: Navree.Role) -> String? {
        switch role {
        case .create: "plus"
        case .cancel: "xmark"
        case .confirm: "checkmark"
        default: nil
        }
    }

    /// The SF Symbol for an icon of the toolkit's set, by its name.
    public static func system(for name: String) -> String? {
        ProtocolSymbols.names[name]?.apple
    }

    /// The symbol to draw for `icon`: its system symbol, or `nil` when its
    /// shape must be drawn.
    public static func symbol(for icon: Navree.Icon) -> String? {
        if let named = icon.symbol?.apple, !named.isEmpty { return named }
        return icon.name.flatMap(system(for:))
    }

    /// The filled form of `icon`'s symbol, where the table has one: for a
    /// selected destination.
    public static func filled(for icon: Navree.Icon) -> String? {
        if let named = icon.symbol?.appleFilled, !named.isEmpty { return named }
        return icon.name.flatMap { ProtocolSymbols.names[$0]?.filled } ?? symbol(for: icon)
    }
}

/// The system colours and symbols a tone stands for. System colours adapt to
/// dark mode and increased contrast by themselves.
public enum Tones {
    public static func color(_ tone: Navree.Tone?) -> Color {
        switch tone {
        case .success: .green
        case .warning: .orange
        case .danger: .red
        case .info: .blue
        case .accent: .accentColor
        case .neutral, nil: .secondary
        }
    }

    /// The symbol that says the tone's meaning without its colour.
    public static func symbol(_ tone: Navree.Tone?) -> String {
        switch tone {
        case .success: "checkmark.circle.fill"
        case .warning: "exclamationmark.triangle.fill"
        case .danger: "xmark.octagon.fill"
        case .info: "info.circle.fill"
        case .accent, .neutral, nil: "circle.fill"
        }
    }
}
