import Foundation
import NavreeDesign
import NavreeProtocol

/// The client's behaviour until the shared runtime core is linked: pages and
/// stacks, route rules, forms, ops and commands, regions and lists by cursor.
/// It is a pure function of the state and an event, with no network and no
/// clock, so the core can replace it behind ``Runtime`` without the drawing
/// or the effects changing.
public struct InterimRuntime: Sendable {
    public private(set) var state = ViewState()
    let origin: Origin
    let base: URL
    let deepLinks: DeepLinks
    let restoration: Restoration
    let signInPaths: RouteRules
    var rules = RouteRules([])
    var tickets = 0

    public init(configuration: ShellConfiguration) {
        origin = configuration.origin
        base = configuration.base
        deepLinks = DeepLinks(origin: configuration.origin, scheme: configuration.urlScheme)
        restoration = configuration.restoration
        signInPaths = RouteRules(configuration.signInPaths.map { Route(match: $0, present: .external) })
    }

    /// How a link to `address` is presented, by the shell's route rules.
    public func presentation(of address: String) -> Present {
        rules.presentation(of: address)
    }

    // MARK: - Events

    public mutating func send(_ event: Event) -> [Effect] {
        switch event {
        case .start:
            return [fetch(.shell, OriginRequest(address: Wire.shellPath))]
        case .select(let href):
            state.selected = href
            return ensureRoot(href)
        case .intent(let intent, let ref):
            return handle(intent, at: ref)
        case .path(let container, let steps):
            return follow(container, steps)
        case .dismissed(let container):
            state.stacks[container] = []
            if container == .cover, let back = state.afterSignIn {
                state.afterSignIn = nil
                return reload(back)
            }
            return []
        case .answered(let ticket, let reply):
            return receive(reply, for: ticket)
        case .deepLink(let url):
            return open(url)
        case .webTitle(let title, let ref):
            update(ref) { $0.webTitle = title }
            return []
        case .webLink(let address, let ref):
            return navigate(address, present: .push, from: ref)
        case .signedIn(let url):
            guard let url, case .app(let address) = origin.destination(of: url) else { return [] }
            return [fetch(.signIn, OriginRequest(address: address))]
        }
    }

    // MARK: - Pages and stacks

    mutating func fetch(_ purpose: Purpose, _ request: OriginRequest) -> Effect {
        tickets += 1
        return .fetch(Ticket(number: tickets, purpose: purpose), request)
    }

    /// Puts a screen for `address` at `index` of `container`, dropping what was
    /// above it, and asks for it.
    mutating func place(_ address: String, in container: Container, at index: Int) -> [Effect] {
        var stack = state.stacks[container] ?? []
        stack = Array(stack.prefix(index))
        stack.append(PageState(address: address))
        state.stacks[container] = stack
        let ref = PageRef(container, index)
        return [fetch(.page(ref, address: address), OriginRequest(address: address))]
    }

    mutating func ensureRoot(_ href: String) -> [Effect] {
        let container = Container.destination(href)
        if state.stacks[container]?.isEmpty == false { return [] }
        return place(href, in: container, at: 0)
    }

    mutating func reload(_ ref: PageRef) -> [Effect] {
        guard let page = state.page(ref) else { return [] }
        return [fetch(.page(ref, address: page.address), OriginRequest(address: page.address))]
    }

    mutating func update(_ ref: PageRef, _ change: (inout PageState) -> Void) {
        guard var stack = state.stacks[ref.container], ref.index < stack.count else { return }
        change(&stack[ref.index])
        state.stacks[ref.container] = stack
    }

    /// The system changed a stack: keep the screens it still shows, ask for
    /// the ones it pushed.
    mutating func follow(_ container: Container, _ steps: [Step]) -> [Effect] {
        var stack = state.stacks[container] ?? []
        guard !stack.isEmpty else { return [] }
        var effects: [Effect] = []
        let keep = steps.enumerated().prefix { offset, step in
            offset + 1 < stack.count && stack[offset + 1].address == step.address
        }.count
        stack = Array(stack.prefix(keep + 1))
        state.stacks[container] = stack
        for step in steps.dropFirst(keep) {
            effects += place(step.address, in: container, at: (state.stacks[container]?.count ?? 1))
        }
        return effects
    }

    mutating func navigate(_ href: String, present asked: Present, from ref: PageRef) -> [Effect] {
        let pageURL = state.page(ref).flatMap { URL(string: $0.address, relativeTo: base) }
        switch origin.destination(of: href, from: pageURL) {
        case .refused:
            return []
        case .external(let url):
            return [.openExternal(url)]
        case .app(let address):
            if signInPaths.rule(for: address) != nil, let url = URL(string: address, relativeTo: base)?.absoluteURL {
                return [.signIn(url)]
            }
            switch rules.presentation(of: address, asked: asked) {
            case .push:
                return place(address, in: ref.container, at: ref.index + 1)
            case .replace:
                update(ref) { $0.address = address }
                return [fetch(.query(ref, address: address, region: nil), OriginRequest(address: address))]
            case .root:
                if state.destinations.contains(where: { $0.href == address }) {
                    state.selected = address
                    let container = Container.destination(address)
                    state.stacks[container] = Array((state.stacks[container] ?? []).prefix(1))
                    return ensureRoot(address)
                }
                return place(address, in: ref.container, at: 0)
            case .sheet:
                return place(address, in: .sheet, at: 0)
            case .full:
                return place(address, in: .cover, at: 0)
            case .external:
                return URL(string: address, relativeTo: base).map { [.openExternal($0.absoluteURL)] } ?? []
            }
        }
    }

    /// A link from outside: it opens only an address the shell names, in the
    /// destination it belongs to, with a GET and nothing else.
    mutating func open(_ url: URL) -> [Effect] {
        guard let address = deepLinks.address(for: url, shell: state.shell) else {
            state.selected = state.destinations.first?.href ?? state.home
            return ensureRoot(state.selected)
        }
        let path = String(address.split(separator: "?", maxSplits: 1).first ?? "")
        let destination = state.destinations
            .filter { path == $0.href || path.hasPrefix($0.href == "/" ? "/" : $0.href + "/") }
            .max { $0.href.count < $1.href.count }?.href ?? state.destinations.first?.href ?? state.home
        state.selected = destination
        var effects = ensureRoot(destination)
        if address != destination {
            effects += place(address, in: .destination(destination), at: 1)
        }
        return effects
    }

    // MARK: - Intents

    mutating func handle(_ intent: Intent, at ref: PageRef) -> [Effect] {
        switch intent {
        case .act(let action, let place):
            return act(action, place: place, at: ref)
        case .change(let form, let name, let values):
            update(ref) { $0.values[form.key, default: [:]][name] = values }
            return []
        case .appear(let region):
            guard let page = state.page(ref), !page.asked.contains("region:" + region),
                case .screen(let screen) = page.content,
                case .region(let node)? = screen.body.find(region, kinds: ["region"]), !node.src.isEmpty
            else { return [] }
            update(ref) { $0.asked.insert("region:" + region) }
            return [fetch(.region(ref, address: page.address, id: region), OriginRequest(address: node.src))]
        case .more(let target, let href):
            guard let page = state.page(ref), !page.asked.contains("more:" + href) else { return [] }
            update(ref) { $0.asked.insert("more:" + href) }
            return [fetch(.more(ref, address: page.address, target: target), OriginRequest(address: href))]
        case .search(let form, let name, let text):
            update(ref) { $0.values[form.key, default: [:]][name] = [text] }
            return submit(form, with: nil, href: nil, at: ref, query: true)
        case .dismissed(let overlay):
            editTree(ref) { $0.edit(overlay, kinds: Node.openable) { $0.setOpen(false) } }
            return []
        case .refresh:
            return reload(ref)
        case .noticeDismissed:
            update(ref) {
                $0.notice = nil
                $0.failure = nil
            }
            return []
        }
    }

    mutating func act(_ action: Action, place: Place, at ref: PageRef) -> [Effect] {
        switch action {
        case .navigate(let href, let present):
            return navigate(href, present: present, from: ref)
        case .submit(let formID, let href, let with):
            let form = place.form.flatMap { $0.key == formID || formID == nil ? $0 : nil }
                ?? formID.flatMap { id in formRef(id, at: ref) }
            guard let form else { return [] }
            return submit(form, with: with, href: href, at: ref, query: isQuery(form, at: ref))
        case .request(let method, let href, let fields):
            guard let page = state.page(ref) else { return [] }
            let request = OriginRequest(method: method, address: href, form: fields)
            return [fetch(.action(ref, address: page.address), request)]
        case .commands(let commands):
            return commands.flatMap { run($0, place: place, at: ref) }
        case .back:
            if ref.index == 0, ref.container == .sheet || ref.container == .cover {
                state.stacks[ref.container] = []
            } else if ref.index > 0 {
                state.stacks[ref.container] = Array((state.stacks[ref.container] ?? []).prefix(ref.index))
            }
            return []
        case .refresh:
            return reload(ref)
        case .dismiss:
            if let overlay = place.overlay {
                editTree(ref) { $0.edit(overlay, kinds: Node.openable) { $0.setOpen(false) } }
            } else if ref.container == .sheet || ref.container == .cover {
                state.stacks[ref.container] = []
            }
            return []
        case .unrecognized:
            return []
        }
    }

    mutating func run(_ command: Command, place: Place, at ref: PageRef) -> [Effect] {
        switch command {
        case .open(let id):
            editTree(ref) { $0.edit(id, kinds: Node.openable) { $0.setOpen(true) } }
        case .close(let id):
            editTree(ref) { $0.edit(id, kinds: Node.openable) { $0.setOpen(false) } }
        case .toggle(let id), .show(let id), .hide(let id):
            editTree(ref) { tree in
                tree.edit(id, kinds: ["stack"]) { node in
                    guard case .stack(var stack) = node else { return }
                    switch command {
                    case .show: stack.hidden = false
                    case .hide: stack.hidden = true
                    default: stack.hidden.toggle()
                    }
                    node = .stack(stack)
                }
            }
        case .copy(let text):
            return [.copy(text), .announce("Copied")]
        case .toast(let tone, let text):
            return notice(Toast(tone: tone, text: text), at: ref)
        case .submit(let id):
            guard let form = formRef(id, at: ref) else { return [] }
            return submit(form, with: nil, href: nil, at: ref, query: isQuery(form, at: ref))
        case .text(let id, let text):
            editTree(ref) { tree in
                tree.edit(id, kinds: ["text"]) { node in
                    guard case .text(var part) = node else { return }
                    part.value = text
                    node = .text(part)
                }
            }
        case .value(let id, let value):
            editTree(ref) { tree in
                tree.walkForms { form in
                    for index in form.hidden.indices where form.hidden[index].id == id {
                        form.hidden[index].value = value
                    }
                }
            }
        case .set, .flip, .focus, .count, .unrecognized:
            break
        }
        return []
    }

    // MARK: - Forms

    /// The form with this id (or `@` and its address) on the screen at `ref`.
    func form(_ key: String, at ref: PageRef) -> Node.Form? {
        guard let page = state.page(ref), case .screen(let screen) = page.content else { return nil }
        let match = screen.body.first { node in
            if case .form(let form) = node { return (form.id ?? "@" + form.action) == key }
            return false
        }
        if case .form(let form)? = match { return form }
        return nil
    }

    func formRef(_ key: String, at ref: PageRef) -> FormRef? {
        form(key, at: ref).map(FormRef.init)
    }

    /// Whether a form is a table's query: its answer takes the table's place.
    func isQuery(_ form: FormRef, at ref: PageRef) -> Bool {
        guard let page = state.page(ref), case .screen(let screen) = page.content else { return false }
        return screen.body.first { node in
            if case .table(let table) = node, let query = table.query { return FormRef(query).key == form.key }
            return false
        } != nil || form.key == "search"
    }

    /// The pairs a form sends, as a browser sends them: its hidden values,
    /// then each field's values under its name, then the pressed button's own.
    func pairs(_ form: FormRef, with extra: Pair?, at ref: PageRef) -> [Pair] {
        let page = state.page(ref)
        let values = page?.values[form.key] ?? [:]
        var pairs = form.hidden
        if let node = self.form(form.key, at: ref) {
            Node.form(node).walk { part in
                guard case .field(let field) = part else { return }
                if case .file = field.input { return }
                for value in values[field.name] ?? field.input.initialValues {
                    pairs.append(Pair(name: field.name, value: value))
                }
            }
        } else {
            for (name, list) in values.sorted(by: { $0.key < $1.key }) {
                pairs += list.map { Pair(name: name, value: $0) }
            }
        }
        if let extra { pairs.append(extra) }
        return pairs
    }

    mutating func submit(_ form: FormRef, with extra: Pair?, href: String?, at ref: PageRef, query: Bool) -> [Effect] {
        guard let page = state.page(ref) else { return [] }
        let address = href ?? (form.action.isEmpty ? page.address : form.action)
        let request = OriginRequest(method: form.method, address: address, form: pairs(form, with: extra, at: ref))
        if query && form.method == .get {
            return [fetch(.query(ref, address: page.address, region: queryRegion(form, at: ref)), request)]
        }
        return [fetch(.action(ref, address: page.address), request)]
    }

    /// The region that holds the table whose query this form is.
    func queryRegion(_ form: FormRef, at ref: PageRef) -> String? {
        guard let page = state.page(ref), case .screen(let screen) = page.content else { return nil }
        var found: String?
        screen.body.walk { node in
            guard found == nil, case .region(let region) = node else { return }
            let holds = Node.region(region).first { inner in
                if case .table(let table) = inner, let query = table.query { return FormRef(query).key == form.key }
                return false
            } != nil
            if holds { found = region.id }
        }
        return found
    }

    // MARK: - Answers

    mutating func receive(_ reply: OriginReply, for ticket: Ticket) -> [Effect] {
        if case .shell = ticket.purpose {
            return receiveShell(reply)
        }
        if case .signIn = ticket.purpose {
            let cover = state.stacks[.cover] ?? []
            state.stacks[.cover] = []
            let back = state.afterSignIn
            state.afterSignIn = nil
            var effects = [fetch(.shell, OriginRequest(address: Wire.shellPath))]
            if let back { effects += reload(back) } else if !cover.isEmpty { effects += ensureRoot(state.selected) }
            return effects
        }
        guard let ref = ticket.purpose.ref, let page = state.page(ref), page.address == ticket.purpose.pageAddress
            || isAddressUpdate(ticket)
        else { return [] }

        switch reply {
        case .failed(let message):
            if case .page = ticket.purpose {
                update(ref) { $0.content = .failed(message) }
            } else {
                update(ref) { $0.failure = Notice(id: "failure", tone: .danger, title: "Couldn't reach the server", text: message) }
            }
            return [.haptic(.error)]
        case .leave(let url):
            if case .page = ticket.purpose, ref.index > 0 {
                state.stacks[ref.container] = Array((state.stacks[ref.container] ?? []).prefix(ref.index))
            }
            return [.openExternal(url)]
        case .redirect(let address):
            return redirect(to: address, for: ticket, at: ref)
        case .body(let status, _, _):
            guard let envelope = reply.envelope else {
                return notNative(ticket, status: status, at: ref)
            }
            return receive(envelope, for: ticket, at: ref)
        }
    }

    func isAddressUpdate(_ ticket: Ticket) -> Bool {
        if case .query = ticket.purpose { return true }
        return false
    }

    mutating func receiveShell(_ reply: OriginReply) -> [Effect] {
        let first = state.shell == nil
        if let shell = reply.shell {
            state.shell = shell
            rules = RouteRules(shell.routes)
        }
        guard first else { return [] }
        let destinations = state.destinations.map(\.href)
        let selected = restoration.selected.flatMap { destinations.contains($0) ? $0 : nil }
            ?? destinations.first { $0 == state.home } ?? destinations.first ?? state.home
        state.selected = selected
        var effects = ensureRoot(selected)
        for (destination, addresses) in restoration.stacks.sorted(by: { $0.key < $1.key }) {
            if destination != selected { effects += ensureRoot(destination) }
            for (offset, address) in addresses.enumerated() {
                effects += place(address, in: .destination(destination), at: offset + 1)
            }
        }
        if let sheet = restoration.sheet {
            effects += place(sheet, in: .sheet, at: 0)
        }
        return effects
    }

    /// A redirect is followed with a GET. One to a screen the shell shows
    /// without its chrome (sign-in) is presented over everything, and the
    /// screen that led there is shown again afterwards.
    mutating func redirect(to address: String, for ticket: Ticket, at ref: PageRef) -> [Effect] {
        if !rules.keepsChrome(address), ref.container != .cover {
            state.afterSignIn = ref
            return place(address, in: .cover, at: 0)
        }
        switch ticket.purpose {
        case .page:
            update(ref) { $0.address = address }
            return [fetch(.page(ref, address: address), OriginRequest(address: address))]
        default:
            update(ref) { $0.address = address }
            return [fetch(.query(ref, address: address, region: nil), OriginRequest(address: address))]
        }
    }

    mutating func notNative(_ ticket: Ticket, status: Int, at ref: PageRef) -> [Effect] {
        switch ticket.purpose {
        case .page(_, let address):
            if let url = URL(string: address, relativeTo: base)?.absoluteURL {
                update(ref) { $0.content = .web(url, title: nil) }
            }
            return []
        default:
            update(ref) {
                $0.failure = Notice(id: "failure", tone: .danger, title: "That didn't work", text: "The server answered with something this app can't show (\(status)).")
            }
            return [.haptic(.error)]
        }
    }

    mutating func receive(_ envelope: Envelope, for ticket: Ticket, at ref: PageRef) -> [Effect] {
        if let upgrade = envelope.upgrade {
            update(ref) { $0.content = .upgrade(upgrade) }
            return []
        }
        if let web = envelope.web {
            if let url = URL(string: web.url, relativeTo: base)?.absoluteURL {
                update(ref) { $0.content = .web(url, title: web.title) }
            }
            return []
        }
        if let screen = envelope.screen {
            switch ticket.purpose {
            case .page, .action:
                update(ref) {
                    $0.content = .screen(screen)
                    $0.icons = envelope.icons
                    $0.asked = []
                    $0.values = [:]
                }
            case .query(_, _, let region):
                if let region, case .region(let fresh)? = screen.body.find(region, kinds: ["region"]) {
                    editTree(ref) { $0.edit(region, kinds: ["region"]) { $0 = .region(fresh) } }
                } else {
                    update(ref) { $0.content = .screen(screen) }
                }
            case .region(_, _, let id):
                let children: [Node]
                if case .region(let inner)? = screen.body.find(id, kinds: ["region"]) {
                    children = inner.children
                } else {
                    children = [screen.body]
                }
                editTree(ref) { tree in
                    tree.edit(id, kinds: ["region"]) { node in
                        guard case .region(var region) = node else { return }
                        region.children = children
                        node = .region(region)
                    }
                }
            case .more(_, _, let target):
                appendMore(from: screen, target: target, at: ref)
            case .shell, .signIn:
                break
            }
        }
        return envelope.ops.flatMap { apply($0, at: ref) }
    }

    /// The next records of a table come as a whole screen: their table's rows
    /// join the ones on screen, and its address of more takes the old one's place.
    mutating func appendMore(from screen: Screen, target: String, at ref: PageRef) {
        let fresh = screen.body.first { node in
            if case .table = node { return true }
            return false
        }
        guard case .table(let next)? = fresh else { return }
        editTree(ref) { tree in
            let found = tree.edit(target, kinds: ["region"]) { region in
                region.forEachTable { table in
                    table.rows += next.rows
                    table.more = next.more
                }
            }
            if !found {
                tree.edit(target, kinds: ["table"]) { node in
                    node.forEachTable { table in
                        table.rows += next.rows
                        table.more = next.more
                    }
                }
            }
        }
    }

    // MARK: - Ops

    mutating func apply(_ op: Op, at ref: PageRef) -> [Effect] {
        switch op {
        case .morph(let target, let node):
            editTree(ref) { tree in
                let found = tree.edit(target, kinds: [node.kind]) { old in
                    let wasOpen = OverlayState.isOpen(old)
                    old = node
                    if wasOpen { old.setOpen(true) }
                }
                if !found { tree.edit(target) { $0 = node } }
            }
        case .replace(let target, let node):
            editTree(ref) { tree in
                if !tree.edit(target, kinds: [node.kind], { $0 = node }) { tree.edit(target) { $0 = node } }
            }
        case .append(let target, let nodes, let more):
            editTree(ref) { $0.edit(target) { $0.insert(nodes, atStart: false, more: more) } }
        case .prepend(let target, let nodes):
            editTree(ref) { $0.edit(target) { $0.insert(nodes, atStart: true) } }
        case .remove(let target):
            editTree(ref) { $0.remove(target) }
        case .errors(let form, let fields):
            editTree(ref) { $0.edit(form, kinds: ["form"]) { $0.setErrors(fields) } }
            let first = fields.sorted { $0.key < $1.key }.first?.value.first
            return first.map { message -> [Effect] in [.haptic(.error), .announce(message)] } ?? []
        case .toast(let toast):
            return notice(toast, at: ref)
        case .open(let target):
            editTree(ref) { $0.edit(target, kinds: Node.openable) { $0.setOpen(true) } }
        case .close(let target):
            editTree(ref) { $0.edit(target, kinds: Node.openable) { $0.setOpen(false) } }
        case .url(let href, _):
            update(ref) { $0.address = href }
        case .navigate(let href):
            return navigate(href, present: .push, from: ref)
        case .navigateExternal(let href):
            guard let url = URL(string: href), case .external = origin.destination(of: url) else { return [] }
            return [.openExternal(url)]
        case .reload:
            return reload(ref)
        case .invalidate(let target):
            guard let page = state.page(ref), case .screen(let screen) = page.content,
                case .region(let region)? = screen.body.find(target, kinds: ["region"])
            else { return [] }
            return [fetch(.region(ref, address: page.address, id: target), OriginRequest(address: region.src))]
        case .focus, .unrecognized:
            break
        }
        return []
    }

    /// What the web shows as a toast: a line of status in the screen, or an
    /// alert when something failed; and a haptic and VoiceOver's words.
    mutating func notice(_ toast: Toast, at ref: PageRef) -> [Effect] {
        let next: Action? = toast.action.map { action in
            action.post ? .request(method: .post, href: action.href, fields: []) : .navigate(href: action.href, present: .push)
        }
        let notice = Notice(
            id: toast.id ?? "notice", tone: toast.tone, title: toast.title, text: toast.text,
            action: next, actionLabel: toast.action?.label
        )
        switch PresentationRules.message(toast.tone) {
        case .alert: update(ref) { $0.failure = notice }
        case .inlineStatus: update(ref) { $0.notice = notice }
        }
        var effects: [Effect] = [.announce([toast.title, toast.text].compactMap { $0 }.joined(separator: ". "))]
        if let haptic = PresentationRules.haptic(toast.tone) { effects.insert(.haptic(haptic), at: 0) }
        return effects
    }

    mutating func editTree(_ ref: PageRef, _ change: (inout Node) -> Void) {
        update(ref) { page in
            guard case .screen(var screen) = page.content else { return }
            change(&screen.body)
            page.content = .screen(screen)
        }
    }
}

enum OverlayState {
    static func isOpen(_ node: Node) -> Bool {
        switch node {
        case .sheet(let sheet): sheet.open
        case .dialog(let dialog): dialog.open
        case .disclosure(let disclosure): disclosure.open
        default: false
        }
    }
}

extension Node {
    /// Sets the messages at the fields of a form, or clears them.
    mutating func setErrors(_ fields: [String: [String]]) {
        if case .field(var field) = self {
            let messages = fields[field.name] ?? []
            field.error = messages.isEmpty ? nil : messages.joined(separator: " ")
            self = .field(field)
            return
        }
        forEachChild { $0.setErrors(fields) }
    }

    /// Changes every table here or inside.
    mutating func forEachTable(_ change: (inout Node.Table) -> Void) {
        if case .table(var table) = self {
            change(&table)
            self = .table(table)
            return
        }
        forEachChild { $0.forEachTable(change) }
    }

    /// Changes every form here or inside.
    mutating func walkForms(_ change: (inout Node.Form) -> Void) {
        if case .form(var form) = self {
            change(&form)
            self = .form(form)
        }
        forEachChild { $0.walkForms(change) }
    }
}
