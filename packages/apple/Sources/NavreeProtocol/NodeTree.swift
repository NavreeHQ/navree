extension Navree.Node {
    /// The kind's name on the wire: `stack`, `table`, `feed_row`.
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
        case .toolbar: "toolbar"
        case .gauge: "gauge"
        case .figure: "figure"
        case .timeline: "timeline"
        case .feedRow: "feed_row"
        case .message: "message"
        case .pane: "pane"
        case .ring: "ring"
        case .rings: "rings"
        case .dayClock: "day_clock"
        case .dayPager: "day_pager"
        case .meter: "meter"
        case .columns: "columns"
        case .strip: "strip"
        case .range: "range"
        case .calendarHeatmap: "calendar_heatmap"
        case .matrixHeatmap: "matrix_heatmap"
        case .scatter: "scatter"
        case .card: "card"
        case .layers: "layers"
        case .shape: "shape"
        case .carousel: "carousel"
        case .adaptive: "adaptive"
        case .unknown(let part): part.tag
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
        case .toolbar(let n): n.id
        case .gauge(let n): n.id
        case .figure(let n): n.id
        case .timeline(let n): n.id
        case .feedRow(let n): n.id
        case .message(let n): n.id
        case .pane(let n): n.id
        case .ring(let n): n.id
        case .rings(let n): n.id
        case .dayClock(let n): n.id
        case .dayPager(let n): n.id
        case .meter(let n): n.id
        case .columns(let n): n.id
        case .strip(let n): n.id
        case .range(let n): n.id
        case .calendarHeatmap(let n): n.id
        case .matrixHeatmap(let n): n.id
        case .scatter(let n): n.id
        case .card(let n): n.id
        case .layers(let n): n.id
        case .shape(let n): n.id
        case .carousel(let n): n.id
        case .adaptive(let n): n.id
        case .unknown(let n): n.id
        }
    }

    /// The nodes directly inside this one, in the order a reader meets them:
    /// the same nodes, in the same order, as the protocol crate's own walk.
    public var children: [Navree.Node] {
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
            (n.query?.children ?? []) + n.rows.flatMap { $0.cells + $0.actions + $0.detail } + [n.empty].compactMap { $0 }
        case .link(let n): n.children
        case .figure(let n): [n.aside, n.chart].compactMap { $0 }
        case .dayPager(let n): [n.scores].compactMap { $0 }
        case .menu(let n): n.items
        case .form(let n): n.children
        case .sheet(let n): n.children + n.actions
        case .dialog(let n): n.children + n.actions
        case .popover(let n): n.children
        case .native(let n): [n.alt].compactMap { $0 }
        case .toolbar(let n): n.items.flatMap(\.menu)
        case .timeline(let n): n.items.flatMap(\.detail)
        case .feedRow(let n): n.detail
        case .message(let n): n.body
        case .pane(let n): n.children
        case .card(let n): n.children + n.menu
        case .layers(let n): n.children
        case .carousel(let n): n.children
        case .adaptive(let n): n.compact + n.wide
        case .unknown(let n): [n.alt].compactMap { $0 }
        case .divider, .spacer, .text, .rich, .image, .icon, .avatar, .badge, .progress, .chart, .code,
            .button, .field, .web, .gauge, .ring, .rings, .dayClock, .meter, .columns, .strip, .range,
            .calendarHeatmap, .matrixHeatmap, .scatter, .shape:
            []
        }
    }

    /// Visits this node and every node inside it, depth first, without
    /// recursion so a deep tree can't overflow the stack.
    public func walk(_ visit: (Navree.Node) -> Void) {
        var pending: [Navree.Node] = [self]
        while let node = pending.popLast() {
            visit(node)
            pending.append(contentsOf: node.children.reversed())
        }
    }

    /// The first node, here or inside, that matches.
    public func first(where matches: (Navree.Node) -> Bool) -> Navree.Node? {
        var pending: [Navree.Node] = [self]
        while let node = pending.popLast() {
            if matches(node) { return node }
            pending.append(contentsOf: node.children.reversed())
        }
        return nil
    }

    /// The first node with this id among the given kinds (all kinds when none
    /// are given). Ids are not unique in real pages: a dialog and the form in it
    /// may share one, so a caller names the kinds it means.
    public func find(_ id: String, kinds: Swift.Set<String> = []) -> Navree.Node? {
        first { $0.id == id && (kinds.isEmpty || kinds.contains($0.kind)) }
    }
}

extension Navree.Shell {
    /// Every destination, in order, groups flattened.
    public var destinations: [Navree.NavItem] {
        nav.groups.flatMap(\.items)
    }
}

extension Navree.Input {
    /// The value the control holds as the screen arrives, as a form sends it.
    /// A range sends its two ends under names of their own, which a field's
    /// one name does not hold, and a control this build does not know sends
    /// nothing.
    public var initialValues: [String] {
        switch self {
        case .text(let input): [input.value]
        case .toggle(let input): input.on ? [input.value ?? "on"] : []
        case .choice(let input): input.value.map { [$0] } ?? []
        case .choices(let input): input.values
        case .date(let input): [input.value]
        case .slider(let input): [input.value]
        case .colour(let input): [input.value]
        case .file, .range, .unknown: []
        }
    }
}
