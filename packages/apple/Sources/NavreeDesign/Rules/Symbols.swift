import NavreeProtocol
import SwiftUI

/// SF Symbols for what the server names: an icon's `symbol` when the server
/// filled it, else the protocol's table's symbol for the icon's name, else
/// this client's own table's (``AppleSymbols``), else none (the icon's own
/// shape is drawn, as large as a symbol would be). A standard action always
/// takes the system's own symbol.
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

    /// The SF Symbol for an icon of the toolkit's set, by its name: the
    /// protocol's table's, else this client's.
    public static func system(for name: String) -> String? {
        ProtocolSymbols.names[name]?.apple ?? AppleSymbols.names[name]?.apple
    }

    /// The symbol to draw for `icon`: its system symbol, or `nil` when its
    /// shape must be drawn.
    public static func symbol(for icon: Navree.Icon) -> String? {
        if let named = icon.symbol?.apple, !named.isEmpty { return named }
        return icon.name.flatMap(system(for:))
    }

    /// The filled form of `icon`'s symbol, where a table has one: for a
    /// selected destination.
    public static func filled(for icon: Navree.Icon) -> String? {
        if let named = icon.symbol?.appleFilled, !named.isEmpty { return named }
        if let named = icon.symbol?.apple, !named.isEmpty { return named }
        guard let name = icon.name else { return nil }
        if let pair = ProtocolSymbols.names[name] { return pair.filled ?? pair.apple }
        return AppleSymbols.names[name].map { $0.filled ?? $0.apple }
    }

    /// The level `icon`'s symbol shows, for a symbol that draws a level
    /// (signal bars at three of four): only from this client's table, and
    /// never for a symbol the server named itself.
    public static func level(for icon: Navree.Icon) -> Double? {
        if let named = icon.symbol?.apple, !named.isEmpty { return nil }
        guard let name = icon.name, ProtocolSymbols.names[name] == nil else { return nil }
        return AppleSymbols.names[name]?.value
    }

    /// `icon` as the system's symbol at its level, or `nil` when its shape
    /// must be drawn.
    public static func image(for icon: Navree.Icon) -> Image? {
        symbol(for: icon).map { Image(systemName: $0, variableValue: level(for: icon)) }
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
