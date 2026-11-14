import Foundation
import NavreeDesign
import NavreeProtocol
import NavreeRuntime

/// The one handler the drawing's intents go to, as events of the core, and
/// what the drawing reads of a screen's state: its forms' values and
/// messages, how its lists and regions are coming, where its focus goes.
///
/// Nothing here decides what a tap does: each intent is said to the core as
/// it happened, with where it happened.
@MainActor
extension NativeClient {
    /// What the drawing of screen `id`, `depth` deep in its stack, reaches the core through.
    public func context(for id: ScreenID, depth: Int, base: URL?) -> DrawingContext {
        let state = screen(id)
        var context = DrawingContext(
            send: { [weak self] intent in self?.perform(intent, on: id) },
            values: { [weak self] form, name in self?.field(form, name, on: id)?.value.values },
            errors: { [weak self] form, name in self?.field(form, name, on: id)?.errors },
            list: { [weak self] list in
                let view = self?.screen(id)?.lists[list]
                var status = ListStatus(loading: view?.loading ?? false, failed: view?.failed ?? false)
                status.across = view?.across ?? false
                return status
            },
            region: { [weak self] region in
                let view = self?.screen(id)?.regions[region]
                return RegionStatus(loading: view?.loading ?? false, failed: view?.failure != nil)
            },
            focus: state?.focus.map { FocusRequest(target: $0.target, serial: $0.serial) },
            from: state?.from,
            address: state?.url ?? "/",
            base: base,
            depth: depth
        )
        context.opened = Set((state?.states ?? [:]).filter { $0.value.contains(.open) }.keys)
        context.routes = app?.routes ?? []
        let pictures = (effects as? SystemEffects)?.pictures
        context.picture = { url in await pictures?.load(url) }
        let site = pictures?.site
        let hosts = pictures?.hosts ?? []
        context.pictureAddress = { source in site?.picture(source, hosts: hosts) }
        return context
    }

    /// The core's state of a form's field, found by the field's name in the
    /// form the control stands in.
    func field(_ form: FormRef, _ name: String, on id: ScreenID) -> FieldView? {
        screen(id)?.forms[form.key]?.field(named: name)?.view
    }

    /// Says an intent of the drawing of screen `id` to the core.
    public func perform(_ intent: Intent, on id: ScreenID) {
        switch intent {
        case .act(let action, let place):
            if MessageButton.is(action) {
                if let message = messages.first { send(.messageAction(id: message.id)) }
                return
            }
            send(.act(CoreEvent.Act(
                screen: id, action: action, form: Self.formID(place.form), overlay: place.overlay, text: place.text,
                ask: place.ask, from: place.from, now: Int(Date().timeIntervalSince1970)
            )))
        case .change(let form, let name, let values):
            guard let found = screen(id)?.forms[form.key]?.field(named: name) else {
                return problem("a field the core does not hold changed: \(form.key) \(name)")
            }
            let value = found.view.value.replaced(with: values)
            send(.field(screen: id, form: Self.formID(form), field: found.id, value: value))
        case .appear:
            // A region is fetched by the core as the screen arrives.
            break
        case .retry(let region):
            send(.regionRetry(screen: id, region: region))
        case .more(let target, _):
            send(.more(screen: id, id: target, before: false))
        case .search(let form, let name, let text):
            if form.key == SearchForm.key {
                send(.search(screen: id, text: text))
            } else {
                // A table's query: the words in its field, then the query sent.
                perform(.change(form: form, name: name, values: [text]), on: id)
                send(.act(CoreEvent.Act(screen: id, action: .submit(Navree.Action.Submit()), form: Self.formID(form))))
            }
        case .dismissed(let overlay):
            send(.overlay(screen: id, id: overlay, open: false))
        case .refresh:
            send(.refresh(screen: id))
        case .noticeDismissed:
            if let message = messages.first { send(.messageShown(id: message.id)) }
        case .tab(let tabs, let index):
            send(.tab(screen: id, id: tabs, index: index))
        case .focused(let serial):
            send(.focused(screen: id, serial: serial))
        case .files(let form, let name, let urls):
            guard let found = screen(id)?.forms[form.key]?.field(named: name) else {
                return problem("a file field the core does not hold changed: \(form.key) \(name)")
            }
            let files = urls.compactMap { url -> FileRef? in
                // A file from another app's storage is lent for as long as it is copied.
                let lent = url.startAccessingSecurityScopedResource()
                defer { if lent { url.stopAccessingSecurityScopedResource() } }
                return FileStore.shared.keep(url)
            }
            send(.field(screen: id, form: Self.formID(form), field: found.id, value: .files(files)))
        case .choose(let table, let row, let on, let across):
            send(.choose(screen: id, table: table, row: row, on: on, across: across))
        case .move(let item, let from, let to, let before):
            send(.move(screen: id, item: item, from: from, to: to, before: before))
        }
    }

    /// A form's id as the core knows it: none for the fields that stand in no
    /// form. The core names a form the server gave no id `@` and its address,
    /// and the drawing reads that name from the tree: it goes back as it is.
    static func formID(_ form: FormRef?) -> String? {
        guard let key = form?.key, !key.isEmpty else { return nil }
        return key
    }
}

/// The form a screen's own search stands for in the drawing.
enum SearchForm {
    static let key = "search"
}

/// The button of a message: the drawing taps it like any action, and the
/// handler says `message_action` for the message shown.
enum MessageButton {
    static let tag = "navree.message"
    static let action = Navree.Action.unknown(Navree.Action.Unknown(tag: tag))

    static func `is`(_ action: Navree.Action) -> Bool {
        if case .unknown(let unknown) = action { return unknown.tag == tag }
        return false
    }
}

/// What the drawing is given of the core's state that the tree has no field for.
enum Drawable {
    /// The tree as it is drawn: a part the core keeps `hidden` stands as a
    /// hidden stack, which is drawn as nothing.
    static func screen(_ screen: Navree.Screen, states: [String: [Navree.State]]) -> Navree.Screen {
        let hidden = Set(states.filter { $0.value.contains(.hidden) }.keys)
        guard !hidden.isEmpty else { return screen }
        var screen = screen
        screen.body = without(hidden, screen.body)
        screen.actions = screen.actions.map { without(hidden, $0) }
        return screen
    }

    static func without(_ hidden: Set<String>, _ node: Navree.Node) -> Navree.Node {
        if let id = node.id, hidden.contains(id) {
            return .stack(Navree.Stack(hidden: true, id: id))
        }
        var node = node
        node.forEachChild { child in child = without(hidden, child) }
        return node
    }

    /// The message a screen shows, as the drawing's notice: a failure is an
    /// alert, anything else a line of status.
    static func notice(_ message: Message?) -> (notice: Notice?, failure: Notice?) {
        guard let message else { return (nil, nil) }
        let notice = Notice(
            id: String(message.id), tone: message.tone, title: message.title,
            text: message.text ?? Words.notice(message.notice),
            action: message.action == nil ? nil : MessageButton.action, actionLabel: message.action
        )
        return PresentationRules.message(message.tone) == .alert ? (nil, notice) : (notice, nil)
    }
}
