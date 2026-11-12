import NavreeProtocol
import SwiftUI

/// SF Symbols for what the server names. A standard action always takes the
/// system's own symbol; an icon of the toolkit's set takes the symbol that
/// means the same where there is one, and its own shape where there is not.
public enum Symbols {
    /// The symbol Apple's guidelines give each standard action.
    public static func standard(_ action: StandardAction) -> String? {
        switch action {
        case .share: "square.and.arrow.up"
        case .delete: "trash"
        case .add: "plus"
        case .edit: "pencil"
        case .search: "magnifyingglass"
        case .done: "checkmark"
        case .cancel: "xmark"
        case .more: "ellipsis"
        case .unrecognized: nil
        }
    }

    /// The symbol a role implies when the action has no icon of its own.
    public static func forRole(_ role: Role) -> String? {
        switch role {
        case .create: "plus"
        case .cancel: "xmark"
        case .confirm: "checkmark"
        default: nil
        }
    }

    /// The SF Symbol for an icon of the toolkit's set, by its name.
    public static func system(for name: String) -> String? {
        table[name]
    }

    /// The symbol to draw for `icon`: its system symbol, or `nil` when its
    /// shape must be drawn.
    public static func symbol(for icon: Node.Icon) -> String? {
        icon.name.flatMap(system(for:))
    }

    static let table: [String: String] = [
        "activity": "waveform.path.ecg",
        "alert-circle": "exclamationmark.circle", "circle-alert": "exclamationmark.circle",
        "alert-triangle": "exclamationmark.triangle", "triangle-alert": "exclamationmark.triangle",
        "archive": "archivebox",
        "arrow-left": "arrow.left", "arrow-right": "arrow.right", "arrow-up": "arrow.up", "arrow-down": "arrow.down",
        "banknote": "banknote",
        "bar-chart": "chart.bar", "chart-bar": "chart.bar", "chart-column": "chart.bar",
        "line-chart": "chart.xyaxis.line", "chart-line": "chart.xyaxis.line",
        "pie-chart": "chart.pie", "chart-pie": "chart.pie",
        "bell": "bell",
        "bookmark": "bookmark",
        "box": "shippingbox", "boxes": "shippingbox", "package": "shippingbox",
        "package-check": "shippingbox.fill",
        "building": "building.2", "warehouse": "building.2",
        "calendar": "calendar",
        "camera": "camera",
        "check": "checkmark", "check-circle": "checkmark.circle", "circle-check": "checkmark.circle",
        "chevron-right": "chevron.right", "chevron-left": "chevron.left",
        "clock": "clock",
        "copy": "doc.on.doc",
        "credit-card": "creditcard",
        "download": "arrow.down.circle", "upload": "arrow.up.circle",
        "eye": "eye", "eye-off": "eye.slash",
        "file": "doc", "file-text": "doc.text",
        "filter": "line.3.horizontal.decrease", "list-filter": "line.3.horizontal.decrease",
        "flag": "flag",
        "folder": "folder",
        "gift": "gift",
        "globe": "globe",
        "grid": "square.grid.2x2", "layout-grid": "square.grid.2x2", "layout-dashboard": "square.grid.2x2",
        "heart": "heart",
        "help-circle": "questionmark.circle", "circle-help": "questionmark.circle",
        "home": "house", "house": "house",
        "image": "photo",
        "inbox": "tray",
        "indian-rupee": "indianrupeesign",
        "info": "info.circle",
        "kanban": "rectangle.split.3x1", "columns": "rectangle.split.3x1",
        "link": "link",
        "list": "list.bullet",
        "lock": "lock", "unlock": "lock.open",
        "log-out": "rectangle.portrait.and.arrow.right",
        "mail": "envelope",
        "map-pin": "mappin", "map": "map",
        "message-circle": "bubble.left", "message-square": "bubble.left",
        "more-horizontal": "ellipsis", "ellipsis": "ellipsis",
        "pencil": "pencil", "pen": "pencil", "square-pen": "square.and.pencil", "edit": "square.and.pencil",
        "phone": "phone",
        "plus": "plus",
        "printer": "printer",
        "receipt": "doc.plaintext",
        "refresh-cw": "arrow.clockwise", "rotate-cw": "arrow.clockwise",
        "repeat": "repeat",
        "search": "magnifyingglass", "search-x": "magnifyingglass",
        "send": "paperplane",
        "settings": "gear", "cog": "gear",
        "share": "square.and.arrow.up", "share-2": "square.and.arrow.up",
        "shopping-bag": "bag", "shopping-cart": "cart",
        "sparkles": "sparkles",
        "star": "star",
        "store": "storefront",
        "tag": "tag",
        "trash": "trash", "trash-2": "trash",
        "truck": "truck.box",
        "undo": "arrow.uturn.backward",
        "user": "person", "users": "person.2", "user-circle": "person.crop.circle", "circle-user": "person.crop.circle",
        "x": "xmark", "x-circle": "xmark.circle", "circle-x": "xmark.circle",
    ]
}

/// The system colours and symbols a tone stands for. System colours adapt to
/// dark mode and increased contrast by themselves.
public enum Tones {
    public static func color(_ tone: Tone?) -> Color {
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
    public static func symbol(_ tone: Tone?) -> String {
        switch tone {
        case .success: "checkmark.circle.fill"
        case .warning: "exclamationmark.triangle.fill"
        case .danger: "xmark.octagon.fill"
        case .info: "info.circle.fill"
        case .accent, .neutral, nil: "circle.fill"
        }
    }
}
