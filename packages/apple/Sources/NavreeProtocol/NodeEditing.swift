extension Navree.Node {
    /// Calls `body` on each node directly inside this one, in place: the same
    /// nodes, in the same order, as ``children``.
    public mutating func forEachChild(_ body: (inout Navree.Node) -> Void) {
        func each(_ nodes: inout [Navree.Node]) {
            for index in nodes.indices { body(&nodes[index]) }
        }
        func one(_ node: inout Navree.Node?) {
            if var inner = node {
                body(&inner)
                node = inner
            }
        }
        switch self {
        case .stack(var n): each(&n.children); self = .stack(n)
        case .grid(var n): each(&n.children); self = .grid(n)
        case .section(var n): each(&n.actions); each(&n.children); each(&n.footer); self = .section(n)
        case .list(var n): each(&n.children); self = .list(n)
        case .row(var n): one(&n.leading); each(&n.detail); one(&n.trailing); each(&n.menu); self = .row(n)
        case .split(var n): body(&n.primary); body(&n.detail); self = .split(n)
        case .tabs(var n):
            for index in n.items.indices { each(&n.items[index].children) }
            self = .tabs(n)
        case .disclosure(var n): each(&n.children); self = .disclosure(n)
        case .region(var n): each(&n.children); self = .region(n)
        case .banner(var n): each(&n.actions); self = .banner(n)
        case .empty(var n): each(&n.actions); self = .empty(n)
        case .facts(var n):
            for index in n.items.indices { each(&n.items[index].value) }
            self = .facts(n)
        case .table(var n):
            if var query = n.query {
                each(&query.children)
                n.query = query
            }
            for index in n.rows.indices {
                each(&n.rows[index].cells)
                each(&n.rows[index].actions)
                each(&n.rows[index].detail)
            }
            one(&n.empty)
            self = .table(n)
        case .link(var n): each(&n.children); self = .link(n)
        case .figure(var n): one(&n.aside); one(&n.chart); self = .figure(n)
        case .dayPager(var n): one(&n.scores); self = .dayPager(n)
        case .menu(var n): each(&n.items); self = .menu(n)
        case .form(var n): each(&n.children); self = .form(n)
        case .sheet(var n): each(&n.children); each(&n.actions); self = .sheet(n)
        case .dialog(var n): each(&n.children); each(&n.actions); self = .dialog(n)
        case .popover(var n): each(&n.children); self = .popover(n)
        case .native(var n): one(&n.alt); self = .native(n)
        case .toolbar(var n):
            for index in n.items.indices { each(&n.items[index].menu) }
            self = .toolbar(n)
        case .timeline(var n):
            for index in n.items.indices { each(&n.items[index].detail) }
            self = .timeline(n)
        case .feedRow(var n): each(&n.detail); self = .feedRow(n)
        case .message(var n): each(&n.body); self = .message(n)
        case .pane(var n): each(&n.children); self = .pane(n)
        case .card(var n): each(&n.children); each(&n.menu); self = .card(n)
        case .layers(var n): each(&n.children); self = .layers(n)
        case .carousel(var n): each(&n.children); self = .carousel(n)
        case .adaptive(var n): each(&n.compact); each(&n.wide); self = .adaptive(n)
        case .unknown(var n): one(&n.alt); self = .unknown(n)
        case .divider, .spacer, .text, .rich, .image, .icon, .avatar, .badge, .progress, .chart, .code,
            .button, .field, .web, .gauge, .ring, .rings, .dayClock, .meter, .columns, .strip, .range,
            .calendarHeatmap, .matrixHeatmap, .scatter, .shape:
            break
        }
    }

    /// Changes the first node, in document order, that has `id` and is one of
    /// `kinds` (any kind when none are given). Returns whether there was one: an
    /// op whose target is gone is passed over, as a browser does.
    @discardableResult
    public mutating func edit(_ id: String, kinds: Swift.Set<String> = [], _ change: (inout Navree.Node) -> Void) -> Bool {
        if self.id == id && (kinds.isEmpty || kinds.contains(kind)) {
            change(&self)
            return true
        }
        var found = false
        forEachChild { child in
            if !found {
                found = child.edit(id, kinds: kinds, change)
            }
        }
        return found
    }

    /// Takes out the first node with `id` among `kinds`. Returns whether there was one.
    @discardableResult
    public mutating func remove(_ id: String, kinds: Swift.Set<String> = []) -> Bool {
        var found = false
        func matches(_ node: Navree.Node) -> Bool {
            !found && node.id == id && (kinds.isEmpty || kinds.contains(node.kind))
        }
        func filter(_ nodes: inout [Navree.Node]) {
            if let index = nodes.firstIndex(where: matches) {
                nodes.remove(at: index)
                found = true
            }
        }
        switch self {
        case .stack(var n): filter(&n.children); self = .stack(n)
        case .grid(var n): filter(&n.children); self = .grid(n)
        case .section(var n): filter(&n.children); self = .section(n)
        case .list(var n): filter(&n.children); self = .list(n)
        case .region(var n): filter(&n.children); self = .region(n)
        case .form(var n): filter(&n.children); self = .form(n)
        case .sheet(var n): filter(&n.children); self = .sheet(n)
        case .dialog(var n): filter(&n.children); self = .dialog(n)
        case .menu(var n): filter(&n.items); self = .menu(n)
        case .table(var n):
            if let index = n.rows.firstIndex(where: { $0.id == id }) {
                n.rows.remove(at: index)
                found = true
            }
            self = .table(n)
        default: break
        }
        if found { return true }
        forEachChild { child in
            if !found {
                found = child.remove(id, kinds: kinds)
            }
        }
        return found
    }

    /// Adds `nodes` at the end (or the start) of this node's children. A list
    /// also takes the address of the rows after these.
    public mutating func insert(_ nodes: [Navree.Node], atStart: Bool, more: String? = nil) {
        func put(_ children: inout [Navree.Node]) {
            if atStart { children.insert(contentsOf: nodes, at: 0) } else { children.append(contentsOf: nodes) }
        }
        switch self {
        case .stack(var n): put(&n.children); self = .stack(n)
        case .grid(var n): put(&n.children); self = .grid(n)
        case .section(var n): put(&n.children); self = .section(n)
        case .list(var n):
            put(&n.children)
            if !atStart { n.more = more }
            self = .list(n)
        case .region(var n): put(&n.children); self = .region(n)
        case .form(var n): put(&n.children); self = .form(n)
        case .sheet(var n): put(&n.children); self = .sheet(n)
        case .dialog(var n): put(&n.children); self = .dialog(n)
        case .menu(var n): put(&n.items); self = .menu(n)
        default: break
        }
    }

    /// Sets whether a sheet, dialog, popover or disclosure is open. Returns
    /// whether this node is one of those.
    @discardableResult
    public mutating func setOpen(_ open: Bool) -> Bool {
        switch self {
        case .sheet(var n): n.open = open; self = .sheet(n)
        case .dialog(var n): n.open = open; self = .dialog(n)
        case .disclosure(var n): n.open = open; self = .disclosure(n)
        case .popover: break
        default: return false
        }
        return true
    }

    /// The kinds an `open` or a `close` means.
    public static let openable: Swift.Set<String> = ["sheet", "dialog", "popover", "disclosure"]
}
