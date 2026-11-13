import NavreeProtocol

/// What changes when the person chooses rows or moves one, as the core is told it.
public enum EditRules {
    /// The ids of a table's rows that are chosen now.
    public static func chosen(_ table: Navree.Table) -> Set<String> {
        Set(table.rows.filter { $0.selected && !$0.unchoosable }.compactMap(\.id))
    }

    /// The ids of the rows a person can choose: a row with no id, or one the
    /// server says can't be chosen, has no box.
    public static func choosable(_ table: Navree.Table) -> Set<String> {
        Set(table.rows.filter { !$0.unchoosable }.compactMap(\.id))
    }

    /// Each row whose box changed, ticked or cleared, in the table's order;
    /// one that can't be chosen never changes.
    public static func changes(_ table: Navree.Table, to next: Set<String>) -> [(row: String, on: Bool)] {
        let before = chosen(table)
        let allowed = choosable(table)
        return table.rows.compactMap(\.id).compactMap { id in
            guard allowed.contains(id) else { return nil }
            if next.contains(id) && !before.contains(id) { return (id, true) }
            if !next.contains(id) && before.contains(id) { return (id, false) }
            return nil
        }
    }

    /// Whether to offer every record that matches the query: when the table
    /// offers it and every row it holds that can be chosen is chosen, as the
    /// system's own lists offer "Select All".
    public static func offersAcross(_ table: Navree.Table, taken: Bool) -> Bool {
        guard table.select?.across != nil, !taken else { return false }
        let allowed = choosable(table)
        return !allowed.isEmpty && chosen(table) == allowed
    }

    /// The buttons that act on the chosen rows: those of the form the table's
    /// selection names, found in the screen.
    public static func bulkActions(_ table: Navree.Table, in body: Navree.Node) -> (form: Navree.Form, buttons: [Navree.Button])? {
        guard let name = table.select?.form, case .form(let form)? = body.find(name, kinds: ["form"]) else { return nil }
        var buttons: [Navree.Button] = []
        form.walkFields { node in
            if case .button(let button) = node { buttons.append(button) }
        }
        return buttons.isEmpty ? nil : (form, buttons)
    }

    /// A row dragged within a list, by the system's offsets: the row moved and
    /// the row it now stands before (none at the end). Nothing when it did
    /// not move.
    public static func move(ids: [String], from offsets: [Int], to destination: Int) -> (item: String, before: String?)? {
        guard offsets.count == 1, let from = offsets.first, ids.indices.contains(from) else { return nil }
        guard destination != from, destination != from + 1 else { return nil }
        let item = ids[from]
        let before = destination < ids.count ? ids[destination] : nil
        return (item, before)
    }

    /// The rows of a list a person can drag: those with an id, when the list can be reordered.
    public static func movable(_ list: Navree.List) -> [String]? {
        guard list.reorder != nil, list.id != nil else { return nil }
        let ids = list.children.compactMap { node -> String? in
            if case .row(let row) = node { return row.id }
            return nil
        }
        return ids.count == list.children.count ? ids : nil
    }
}
