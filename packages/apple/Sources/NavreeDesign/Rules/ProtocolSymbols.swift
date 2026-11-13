import NavreeProtocol

/// The protocol's own table of system symbols (`x-symbols` in its schema): the
/// SF Symbol each icon name of the toolkit's set and each standard action
/// stands for, plain and filled. The server fills an icon's `symbol` from the
/// same table; this copy reads an icon that came without one. A test holds it
/// to the schema, so the two never drift apart.
///
/// A name the table does not hold has no symbol: its own shape is drawn.
public enum ProtocolSymbols {
    /// A symbol in its plain and its filled form.
    public struct Pair: Sendable, Equatable {
        public let apple: String
        public let filled: String?

        public init(_ apple: String, _ filled: String? = nil) {
            self.apple = apple
            self.filled = filled
        }
    }

    /// The symbols of the standard actions.
    public static let standard: [Navree.Standard: Pair] = [
        .add: Pair("plus"),
        .cancel: Pair("xmark"),
        .delete: Pair("trash", "trash.fill"),
        .done: Pair("checkmark"),
        .edit: Pair("pencil"),
        .more: Pair("ellipsis.circle", "ellipsis.circle.fill"),
        .search: Pair("magnifyingglass"),
        .share: Pair("square.and.arrow.up", "square.and.arrow.up.fill"),
    ]

    /// The symbols of the toolkit's icon names.
    public static let names: [String: Pair] = [
        "activity": Pair("waveform.path.ecg"),
        "archive": Pair("archivebox", "archivebox.fill"),
        "arrow-up-down": Pair("arrow.up.arrow.down"),
        "bar-chart": Pair("chart.bar", "chart.bar.fill"),
        "bed": Pair("bed.double", "bed.double.fill"),
        "bell": Pair("bell", "bell.fill"),
        "bike": Pair("bicycle"),
        "box": Pair("shippingbox", "shippingbox.fill"),
        "brain": Pair("brain.head.profile"),
        "building": Pair("building.2", "building.2.fill"),
        "building-2": Pair("building.2", "building.2.fill"),
        "calendar": Pair("calendar"),
        "camera": Pair("camera", "camera.fill"),
        "chart-bar": Pair("chart.bar", "chart.bar.fill"),
        "chart-column": Pair("chart.bar", "chart.bar.fill"),
        "check": Pair("checkmark"),
        "circle-help": Pair("questionmark.circle", "questionmark.circle.fill"),
        "circle-user": Pair("person", "person.fill"),
        "clock": Pair("clock", "clock.fill"),
        "clock-4": Pair("clock", "clock.fill"),
        "credit-card": Pair("creditcard", "creditcard.fill"),
        "download": Pair("square.and.arrow.down", "square.and.arrow.down.fill"),
        "droplet": Pair("drop", "drop.fill"),
        "droplets": Pair("drop", "drop.fill"),
        "dumbbell": Pair("dumbbell", "dumbbell.fill"),
        "ellipsis": Pair("ellipsis.circle", "ellipsis.circle.fill"),
        "ellipsis-vertical": Pair("ellipsis.circle", "ellipsis.circle.fill"),
        "file": Pair("doc.text", "doc.text.fill"),
        "file-text": Pair("doc.text", "doc.text.fill"),
        "filter": Pair("line.3.horizontal.decrease.circle", "line.3.horizontal.decrease.circle.fill"),
        "flame": Pair("flame", "flame.fill"),
        "folder": Pair("folder", "folder.fill"),
        "footprints": Pair("figure.walk"),
        "heart": Pair("heart", "heart.fill"),
        "heart-pulse": Pair("bolt.heart", "bolt.heart.fill"),
        "home": Pair("house", "house.fill"),
        "hourglass": Pair("hourglass"),
        "house": Pair("house", "house.fill"),
        "image": Pair("photo", "photo.fill"),
        "inbox": Pair("tray", "tray.fill"),
        "info": Pair("info.circle", "info.circle.fill"),
        "layout-dashboard": Pair("square.grid.2x2", "square.grid.2x2.fill"),
        "layout-grid": Pair("square.grid.2x2", "square.grid.2x2.fill"),
        "link": Pair("link"),
        "list": Pair("list.bullet"),
        "list-filter": Pair("line.3.horizontal.decrease.circle", "line.3.horizontal.decrease.circle.fill"),
        "lock": Pair("lock", "lock.fill"),
        "log-out": Pair("rectangle.portrait.and.arrow.right"),
        "mail": Pair("envelope", "envelope.fill"),
        "map": Pair("map", "map.fill"),
        "map-pin": Pair("mappin"),
        "message-circle": Pair("bubble.left", "bubble.left.fill"),
        "message-square": Pair("bubble.left", "bubble.left.fill"),
        "moon": Pair("moon", "moon.fill"),
        "mountain": Pair("mountain.2", "mountain.2.fill"),
        "package": Pair("shippingbox", "shippingbox.fill"),
        "pencil": Pair("pencil"),
        "person-standing": Pair("figure.stand"),
        "phone": Pair("phone", "phone.fill"),
        "plus": Pair("plus"),
        "radio": Pair("antenna.radiowaves.left.and.right"),
        "receipt": Pair("receipt", "receipt.fill"),
        "refresh-cw": Pair("arrow.clockwise"),
        "search": Pair("magnifyingglass"),
        "send": Pair("paperplane", "paperplane.fill"),
        "settings": Pair("gearshape", "gearshape.fill"),
        "settings-2": Pair("gearshape", "gearshape.fill"),
        "share": Pair("square.and.arrow.up", "square.and.arrow.up.fill"),
        "share-2": Pair("square.and.arrow.up", "square.and.arrow.up.fill"),
        "shield-check": Pair("checkmark.shield", "checkmark.shield.fill"),
        "shopping-bag": Pair("bag", "bag.fill"),
        "shopping-cart": Pair("cart", "cart.fill"),
        "sparkles": Pair("sparkles"),
        "square-pen": Pair("pencil"),
        "star": Pair("star", "star.fill"),
        "store": Pair("storefront", "storefront.fill"),
        "sun": Pair("sun.max", "sun.max.fill"),
        "sunrise": Pair("sunrise", "sunrise.fill"),
        "tag": Pair("tag", "tag.fill"),
        "thermometer": Pair("thermometer.medium"),
        "timer": Pair("timer"),
        "trash": Pair("trash", "trash.fill"),
        "trash-2": Pair("trash", "trash.fill"),
        "trending-up": Pair("chart.line.uptrend.xyaxis"),
        "triangle-alert": Pair("exclamationmark.triangle", "exclamationmark.triangle.fill"),
        "truck": Pair("truck.box", "truck.box.fill"),
        "user": Pair("person", "person.fill"),
        "users": Pair("person.2", "person.2.fill"),
        "volume-2": Pair("speaker.wave.2", "speaker.wave.2.fill"),
        "waves": Pair("water.waves"),
        "waves-horizontal": Pair("water.waves"),
        "wind": Pair("wind"),
        "x": Pair("xmark"),
        "zap": Pair("bolt", "bolt.fill"),
    ]
}
