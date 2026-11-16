import NavreeProtocol

/// How things drawn one after another are told apart. SwiftUI keeps what a
/// drawn part holds (words being typed, a sheet open over a row, a picture
/// loaded, the row chosen) with the part's identity. Where that identity is a
/// place in a list, a row that arrives or leaves hands every row after it the
/// state of its neighbour: a sheet opened on one order then stands over
/// another. So a part is known by the id the tree gives it, and by its place
/// only when it has none.
public enum IdentityRules {
    /// A name for each of some parts, by their ids in order: a part's own id,
    /// or its place among them when it has none (or an empty one), each made
    /// unique. A part that arrives is a new name; the parts already drawn
    /// keep theirs wherever they now stand.
    public static func identities(_ ids: [String?]) -> [String] {
        var seen = Set<String>()
        return ids.enumerated().map { index, id in
            var identity = id.flatMap { $0.isEmpty ? nil : "#" + $0 } ?? "@\(index)"
            while seen.contains(identity) { identity += "'" }
            seen.insert(identity)
            return identity
        }
    }

    /// Each of `items` with its name among them and its place.
    public static func told<Item>(_ items: [Item], id: (Item) -> String?) -> [Told<Item>] {
        let names = identities(items.map(id))
        return items.indices.map { Told(id: names[$0], index: $0, item: items[$0]) }
    }
}

/// One of several things drawn one after another: what tells it from the
/// others (``IdentityRules/identities(_:)``), where it stands, and itself.
public struct Told<Item>: Identifiable {
    public let id: String
    public let index: Int
    public let item: Item
}

extension Array where Element == Navree.Node {
    /// The parts, each told from the others by its own id.
    var told: [Told<Navree.Node>] {
        IdentityRules.told(self) { $0.id }
    }
}

extension RowItem {
    /// What the tree gives to tell this row from the other rows of its
    /// section: a record's, a row's, a part's or a button's id; a field's;
    /// the region or the list a row that waits stands for. `nil` for a row
    /// the tree gives nothing to name.
    public var identity: String? {
        switch self {
        case .part(let node): return node.id.map { "part " + $0 }
        case .record(let row): return row.id.map { "record " + $0 }
        case .row(let row): return row.id.map { "row " + $0 }
        case .button(let button, _): return button.id.map { "button " + $0 }
        case .fact, .labeled: return nil
        case .field(let field, _): return "field " + field.key
        case .option(let field, let option, _): return "option " + field.key + " " + option.value
        case .pending(let region): return "pending " + region
        case .more(let target, _): return "more " + target
        }
    }
}
