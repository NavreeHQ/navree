extension Node {
    /// The kind's name on the wire: `stack`, `table`, `field`.
    public var kind: String {
        switch self {
        case .stack: "stack"
        case .grid: "grid"
        case .section: "section"
        case .list: "list"
        case .row: "row"
        case .split: "split"
        case .tabs: "tabs"
        case .disclosure: "disclosure"
        case .divider: "divider"
        case .spacer: "spacer"
        case .region: "region"
        case .text: "text"
        case .rich: "rich"
        case .image: "image"
        case .icon: "icon"
        case .avatar: "avatar"
        case .badge: "badge"
        case .progress: "progress"
        case .banner: "banner"
        case .empty: "empty"
        case .facts: "facts"
        case .table: "table"
        case .chart: "chart"
        case .code: "code"
        case .button: "button"
        case .link: "link"
        case .menu: "menu"
        case .form: "form"
        case .field: "field"
        case .sheet: "sheet"
        case .dialog: "dialog"
        case .popover: "popover"
        case .native: "native"
        case .web: "web"
        case .unknown(let part): part.kind
        }
    }

    /// The node's id, when it has one: what ops and commands name.
    public var id: String? {
        switch self {
        case .stack(let n): n.id
        case .grid(let n): n.id
        case .section(let n): n.id
        case .list(let n): n.id
        case .row(let n): n.id
        case .split(let n): n.id
        case .tabs(let n): n.id
        case .disclosure(let n): n.id
        case .divider(let n): n.id
        case .spacer(let n): n.id
        case .region(let n): n.id
        case .text(let n): n.id
        case .rich(let n): n.id
        case .image(let n): n.id
        case .icon(let n): n.id
        case .avatar(let n): n.id
        case .badge(let n): n.id
        case .progress(let n): n.id
        case .banner(let n): n.id
        case .empty(let n): n.id
        case .facts(let n): n.id
        case .table(let n): n.id
        case .chart(let n): n.id
        case .code(let n): n.id
        case .button(let n): n.id
        case .link(let n): n.id
        case .menu(let n): n.id
        case .form(let n): n.id
        case .field(let n): n.id
        case .sheet(let n): n.id
        case .dialog(let n): n.id
        case .popover(let n): n.id
        case .native(let n): n.id
        case .web(let n): n.id
        case .unknown(let n): n.id
        }
    }

    /// The nodes directly inside this one, in the order a reader meets them.
    public var children: [Node] {
        switch self {
        case .stack(let n): n.children
        case .grid(let n): n.children
        case .section(let n): n.actions + n.children + n.footer
        case .list(let n): n.children
        case .row(let n): [n.leading].compactMap { $0 } + n.detail + [n.trailing].compactMap { $0 } + n.menu
        case .split(let n): [n.primary, n.detail]
        case .tabs(let n): n.items.flatMap(\.children)
        case .disclosure(let n): n.children
        case .region(let n): n.children
        case .banner(let n): n.actions
        case .empty(let n): n.actions
        case .facts(let n): n.items.flatMap(\.value)
        case .table(let n):
            (n.query?.children ?? []) + n.rows.flatMap(\.cells) + [n.empty].compactMap { $0 }
        case .link(let n): n.children
        case .menu(let n): n.items
        case .form(let n): n.children
        case .sheet(let n): n.children + n.actions
        case .dialog(let n): n.children + n.actions
        case .popover(let n): n.children
        case .native(let n): [n.alt].compactMap { $0 }
        case .unknown(let n): [n.alt].compactMap { $0 }
        case .divider, .spacer, .text, .rich, .image, .icon, .avatar, .badge, .progress, .chart, .code,
            .button, .field, .web:
            []
        }
    }

    /// Visits this node and every node inside it, depth first, without
    /// recursion so a deep tree can't overflow the stack.
    public func walk(_ visit: (Node) -> Void) {
        var pending: [Node] = [self]
        while let node = pending.popLast() {
            visit(node)
            pending.append(contentsOf: node.children.reversed())
        }
    }

    /// The first node, here or inside, that matches.
    public func first(where matches: (Node) -> Bool) -> Node? {
        var pending: [Node] = [self]
        while let node = pending.popLast() {
            if matches(node) { return node }
            pending.append(contentsOf: node.children.reversed())
        }
        return nil
    }

    /// The first node with this id among the given kinds (all kinds when none
    /// are given). Ids are not unique in real pages: a dialog and the form in it
    /// may share one, so a caller names the kinds it means.
    public func find(_ id: String, kinds: Set<String> = []) -> Node? {
        first { $0.id == id && (kinds.isEmpty || kinds.contains($0.kind)) }
    }
}
