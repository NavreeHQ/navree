import Foundation
import NavreeCoreBridge
import NavreeProtocol
import Observation
import os

/// Carries out what the core asks. Whatever comes of an effect is a later
/// event, given to ``NativeClient/send(_:)`` on the main actor.
@MainActor
public protocol EffectRunner: AnyObject {
    func run(_ effect: CoreEffect, for client: NativeClient)
}

/// One screen of the view state, observed on its own: a change to one screen
/// redraws the views that read it, and no other screen's.
@MainActor
@Observable
public final class ScreenModel: Identifiable {
    public internal(set) var state: ScreenState

    public nonisolated var id: ObjectIdentifier { ObjectIdentifier(self) }

    init(_ state: ScreenState) {
        self.state = state
    }
}

/// The loop of the runtime core, and the model the app draws from.
///
/// One event in; each part of the view state the update names read back and
/// put in place of the old one; then each effect carried out, in order. An
/// event sent while another is being handled (an effect answered at once)
/// waits its turn, so the core sees one event at a time, in the order things
/// happened, always on the main actor.
///
/// The client keeps no state of its own about navigation, trees, forms,
/// loading, errors or messages: what is here is the core's view state, part
/// by part, as it last said it.
@MainActor
@Observable
public final class NativeClient {
    /// The app's name, theme, and the person signed in.
    public private(set) var app: AppPart?
    /// Where the person is.
    public private(set) var nav = NavPart()
    /// Every screen some stack holds, by number.
    public private(set) var screens: [ScreenID: ScreenModel] = [:]
    /// The messages still waiting to be shown.
    public private(set) var messages: [Message] = []

    /// Carries out the effects.
    @ObservationIgnored public var effects: (any EffectRunner)?

    /// Failures of the core itself: an event that did not read, a part that
    /// did not decode. Bugs to log, never anything to draw.
    @ObservationIgnored public var problem: @MainActor (String) -> Void = { text in
        NativeClient.log.fault("\(text)")
    }

    @ObservationIgnored private let core: Core
    @ObservationIgnored private var queue: [(json: Data, tag: String)] = []
    @ObservationIgnored private var sending = false
    @ObservationIgnored public private(set) var began = false

    nonisolated static let log = Logger(subsystem: "com.navree.native", category: "runtime")

    /// A client over `core`, whose effects `effects` carries out.
    public init(core: Core, effects: (any EffectRunner)? = nil) {
        self.core = core
        self.effects = effects
    }

    /// The screen with this number, while a stack holds it.
    public func screen(_ id: ScreenID) -> ScreenState? {
        screens[id]?.state
    }

    /// The screen the person is looking at.
    public var front: ScreenState? {
        nav.front.flatMap(screen)
    }

    /// Starts the app once: plainly, or from what was kept of a launch the
    /// system ended (the core checks the snapshot and starts plainly when it
    /// is not for this launch). Then everything is read once.
    public func begin(restoring snapshot: Data? = nil) {
        guard !began else { return }
        began = true
        send(snapshot.map { .restore(snapshot: $0) } ?? .start)
        readAll()
    }

    /// Tells the core one thing that happened.
    public func send(_ event: CoreEvent) {
        send(json: event.json, tag: event.tag)
    }

    /// Tells the core an event already written as JSON.
    func send(json: Data, tag: String) {
        queue.append((json, tag))
        guard !sending else { return }
        sending = true
        defer { sending = false }
        while !queue.isEmpty {
            handle(queue.removeFirst())
        }
    }

    /// Reads every part again: at start, and after the interface was made
    /// again from nothing.
    public func readAll() {
        guard let data = query(Data()) else { return }
        guard let state = read(ViewState.self, data, "the view state") else { return }
        app = state.app
        nav = state.nav
        var next: [ScreenID: ScreenModel] = [:]
        for (id, screen) in state.screens {
            if let model = screens[id] {
                model.state = screen
                next[id] = model
            } else {
                next[id] = ScreenModel(screen)
            }
        }
        screens = next
        messages = state.messages
    }

    /// What the core keeps for a launch after the system ended the app, as
    /// JSON to store as it is, or `nil` when there is nothing to keep. It
    /// holds what the person typed.
    public func snapshot() -> Data? {
        query(Self.part("snapshot"))
    }

    // MARK: - The loop

    private func handle(_ event: (json: Data, tag: String)) {
        switch Update.read(core.send(event.json)) {
        case .failure(let error):
            problem("the core did not take `\(event.tag)`: \(error)")
        case .success(let update):
            apply(update.changed)
            for tag in update.unread {
                problem("an effect did not read: \(tag)")
            }
            for effect in update.effects {
                effects?.run(effect, for: self)
            }
        }
    }

    /// Puts each changed part in place of the old one.
    private func apply(_ changes: [Change]) {
        var read: Set<ScreenID> = []
        for change in changes {
            switch change {
            case .app:
                guard let data = query(Self.part("app")) else {
                    app = nil
                    continue
                }
                if let part = decode(AppPart.self, data, "the app") { app = part }
            case .nav:
                guard let data = query(Self.part("nav")), let part = decode(NavPart.self, data, "the navigation")
                else { continue }
                nav = part
                // A screen in no stack is gone.
                let held = part.held
                if screens.keys.contains(where: { !held.contains($0) }) {
                    screens = screens.filter { held.contains($0.key) }
                }
            case .screen(let id):
                if read.insert(id).inserted { readScreen(id) }
            case .form(let screen, let form):
                guard !read.contains(screen) else { continue }
                guard let model = screens[screen] else {
                    read.insert(screen)
                    readScreen(screen)
                    continue
                }
                let data = query(Self.form(screen, form))
                if let data, let view = decode(FormView.self, data, "a form") {
                    model.state.forms[form] = view
                } else if data == nil {
                    model.state.forms[form] = nil
                }
            case .rows(let screen, _), .redraw(let screen, _):
                // New rows, or rows named again: the whole screen read again is always right.
                if read.insert(screen).inserted { readScreen(screen) }
            case .messages:
                if let data = query(Self.part("messages")), let list = decode([Message].self, data, "the messages") {
                    messages = list
                } else {
                    messages = []
                }
            case .unknown:
                readAll()
                return
            }
        }
    }

    private func readScreen(_ id: ScreenID) {
        guard let data = query(Self.screen(id)) else {
            if screens[id] != nil { screens[id] = nil }
            return
        }
        guard let state = read(ScreenState.self, data, "screen \(id)") else { return }
        if let model = screens[id] {
            model.state = state
        } else {
            screens[id] = ScreenModel(state)
        }
    }

    // MARK: - Reading parts

    /// How deep a part of the view state may nest: a tree the core has read
    /// and checked, a few levels inside the part.
    nonisolated static let nesting = Wire.maxNesting + 8

    /// Reads a part: `nil` when there is none (`null`) or the core refused the query.
    private func query(_ query: Data) -> Data? {
        let data = core.view(query)
        if data.isEmpty || data == Data("null".utf8) { return nil }
        if data.starts(with: Data(#"{"error":"#.utf8)) {
            let words = String(decoding: data, as: UTF8.self)
            problem("the core refused a query: \(words)")
            return nil
        }
        return data
    }

    /// Reads a part that holds trees, on a thread with room for the deepest.
    private func read<T: Decodable & Sendable>(_ type: T.Type, _ data: Data, _ what: String) -> T? {
        do {
            return try WireDecoder.decode(type, from: data, limit: Self.nesting)
        } catch {
            // The core writes a command whose state came from a newer server
            // back as `unknown`, a word the protocol's models do not have; the
            // core skips such a command, so it goes before the tree is read.
            if let repaired = Repair.withoutUnknownStates(data),
                let state = try? WireDecoder.decode(type, from: repaired, limit: Self.nesting)
            {
                Self.log.notice("\(what, privacy: .public) held a command with a state this build does not know")
                return state
            }
            problem("\(what) did not decode: \(error)")
            return nil
        }
    }

    private func decode<T: Decodable>(_ type: T.Type, _ data: Data, _ what: String) -> T? {
        do {
            return try JSONDecoder().decode(type, from: data)
        } catch {
            problem("\(what) did not decode: \(error)")
            return nil
        }
    }

    nonisolated static func part(_ name: String) -> Data {
        Data(#"{"part":"\#(name)"}"#.utf8)
    }

    nonisolated static func screen(_ id: ScreenID) -> Data {
        Data(#"{"part":"screen","id":\#(id)}"#.utf8)
    }

    nonisolated static func form(_ screen: ScreenID, _ form: String) -> Data {
        let object: [String: Any] = ["part": "form", "screen": screen, "form": form]
        return (try? JSONSerialization.data(withJSONObject: object)) ?? Data()
    }
}

/// Mends what the core writes that the protocol's models cannot read.
enum Repair {
    /// The JSON with every `set` or `flip` command whose state is `unknown`
    /// left out of the list it stands in; `nil` when there is none.
    static func withoutUnknownStates(_ data: Data) -> Data? {
        guard data.range(of: Data(#""state":"unknown""#.utf8)) != nil,
            let value = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
        else { return nil }
        var found = false
        func mend(_ value: Any) -> Any {
            if let list = value as? [Any] {
                return list.compactMap { item -> Any? in
                    if let command = item as? [String: Any], ["set", "flip"].contains(command["c"] as? String ?? ""),
                        command["state"] as? String == "unknown"
                    {
                        found = true
                        return nil
                    }
                    return mend(item)
                }
            }
            if let object = value as? [String: Any] {
                return object.mapValues(mend)
            }
            return value
        }
        let mended = mend(value)
        guard found else { return nil }
        return try? JSONSerialization.data(withJSONObject: mended, options: [.fragmentsAllowed])
    }
}
