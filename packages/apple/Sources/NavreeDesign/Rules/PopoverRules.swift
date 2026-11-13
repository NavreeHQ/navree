import NavreeProtocol

public enum PopoverRules {
    /// The popover an action opens: the one an `open` command names.
    public static func opens(_ action: Navree.Action) -> String? {
        guard case .commands(let commands) = action else { return nil }
        for command in commands.list {
            if case .open(let open) = command { return open.id }
        }
        return nil
    }

    /// Every popover of a tree, by id.
    public static func popovers(in body: Navree.Node) -> [String: Navree.Popover] {
        var found: [String: Navree.Popover] = [:]
        body.walk { node in
            if case .popover(let popover) = node, found[popover.id] == nil { found[popover.id] = popover }
        }
        return found
    }

    /// Whether some control of the tree opens the popover `id`, so it can be
    /// anchored there; one that nothing opens is shown as a sheet.
    public static func anchored(_ id: String, in body: Navree.Node, actions: [Navree.Node] = []) -> Bool {
        var found = false
        for node in actions + [body] {
            node.walk { part in
                guard !found else { return }
                if case .button(let button) = part, let action = button.action, opens(action) == id { found = true }
            }
        }
        return found
    }
}
