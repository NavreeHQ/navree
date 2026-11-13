import Foundation
import NavreeCoreBridge
import NavreeProtocol
import Testing

@testable import NavreeRuntime

/// The runtime core's own conversations: each step an event and what came of it.
enum Transcripts {
    static let folder = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()  // NavreeRuntimeTests
        .deletingLastPathComponent()  // Tests
        .deletingLastPathComponent()  // apple
        .deletingLastPathComponent()  // packages
        .deletingLastPathComponent()
        .appendingPathComponent("crates/navree-native-core/transcripts")

    static let origin = "https://shop.example"

    /// Every conversation, by name.
    static var names: [String] {
        let files = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
        return files.filter { $0.hasSuffix(".json") }.map { String($0.dropLast(5)) }.sorted()
    }

    static func load(_ name: String) throws -> [String: Any] {
        let data = try Data(contentsOf: folder.appendingPathComponent("\(name).json"))
        return try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    static func fixture(_ path: String) throws -> String {
        try String(contentsOf: folder.appendingPathComponent("fixtures").appendingPathComponent(path), encoding: .utf8)
    }

    /// The configuration the core's own replays use, with a conversation's
    /// own settings over it.
    static func config(_ over: [String: Any] = [:]) -> Data {
        var base: [String: Any] = [
            "origin": origin,
            "client": ["platform": "ios", "form": "phone", "vocab": 0, "text_percent": 100] as [String: Any],
        ]
        for (key, value) in over { base[key] = value }
        return (try? JSONSerialization.data(withJSONObject: base)) ?? Data()
    }
}

/// Carries out nothing, and keeps every effect it was asked for.
@MainActor
final class Recorder: EffectRunner {
    var effects: [CoreEffect] = []
    /// What to do with an effect, in place of nothing.
    var react: ((CoreEffect, NativeClient) -> Void)?

    func run(_ effect: CoreEffect, for client: NativeClient) {
        effects.append(effect)
        react?(effect, client)
    }

    /// The fetches asked for since the last time, as their numbers and addresses.
    func takeFetches() -> [(Int, String, String)] {
        defer { effects.removeAll() }
        return effects.compactMap { effect in
            if case .fetch(let request) = effect { return (request.request, request.method, request.url) }
            return nil
        }
    }

    var lastFetch: FetchRequest? {
        effects.reversed().lazy.compactMap { effect -> FetchRequest? in
            if case .fetch(let request) = effect { return request }
            return nil
        }.first
    }
}

/// A client over a fresh core, with the problems it logs kept for the test to see.
@MainActor
struct Harness {
    let client: NativeClient
    let recorder: Recorder
    let core: Core
    let problems: Problems

    final class Problems {
        var list: [String] = []
    }

    init(config: Data = Transcripts.config()) throws {
        core = try #require(Core(config: config))
        recorder = Recorder()
        client = NativeClient(core: core, effects: recorder)
        let problems = Problems()
        self.problems = problems
        client.problem = { problems.list.append($0) }
    }

    /// Answers request `request` with a fixture of the core's conversations.
    func answer(_ request: Int, with fixture: String, status: Int = 200) throws {
        client.send(.response(Response(
            request: request, status: status, contentType: "application/vnd.navree.native+json",
            body: try Transcripts.fixture(fixture)
        )))
    }

    /// The app started, its shell arrived, and its first page.
    func started(shell: String = "store/shell.json", first: String = "store/dashboard.json") throws {
        client.begin()
        try answer(1, with: shell)
        try answer(2, with: first)
    }
}

/// What the view state holds, written out so two readings can be compared:
/// the model kept part by part, and the whole state read at once.
func fingerprint(app: AppPart?, nav: NavPart, screens: [ScreenID: ScreenState], messages: [Message]) -> String {
    var lines: [String] = []
    lines.append("app \(app?.name ?? "-") \(app?.user?.name ?? "-") \(app?.home ?? "-")")
    lines.append(
        "nav \(nav.stage) \(String(describing: nav.failure)) \(nav.selected) \(String(describing: nav.front)) "
            + "\(nav.canGoBack) \(nav.width)"
    )
    for destination in nav.destinations {
        lines.append("  destination \(destination.href) \(destination.stack)")
    }
    for modal in nav.modals {
        lines.append("  modal \(modal.present) \(modal.locked) \(modal.stack)")
    }
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    for id in screens.keys.sorted() {
        let screen = screens[id]!
        let content: String
        switch screen.content {
        case .blank: content = "blank"
        case .native(let tree):
            let data = (try? encoder.encode(tree)) ?? Data()
            content = "native \(tree.title) \(data.count) \(data.hashValue)"
        case .web(let url, let title, let why, let serial): content = "web \(url) \(title ?? "-") \(why) \(serial)"
        }
        lines.append(
            "screen \(id) \(screen.url) \(screen.title ?? "-") \(content) loading=\(screen.loading) "
                + "refreshing=\(screen.refreshing) cached=\(screen.cached) \(String(describing: screen.failure)) "
                + "\(screen.status) chrome=\(screen.chrome) overlays=\(screen.overlays) from=\(screen.from ?? "-")"
        )
        for (node, states) in screen.states.sorted(by: { $0.key < $1.key }) {
            lines.append("  state \(node) \(states.map(\.rawValue))")
        }
        for (name, form) in screen.forms.sorted(by: { $0.key < $1.key }) {
            lines.append("  form \(name) pending=\(form.pending)")
            for (field, view) in form.fields.sorted(by: { $0.key < $1.key }) {
                lines.append(
                    "    field \(field) \(view.name) \(view.value) \(view.errors) options=\(view.options?.count ?? -1) "
                        + "\(view.more) \(view.searching)"
                )
            }
        }
        for (name, region) in screen.regions.sorted(by: { $0.key < $1.key }) {
            lines.append("  region \(name) \(region.loading) \(String(describing: region.failure))")
        }
        for (name, list) in screen.lists.sorted(by: { $0.key < $1.key }) {
            lines.append(
                "  list \(name) \(list.loading) \(list.failed) \(list.loadingBefore) \(list.failedBefore) \(list.across)"
            )
        }
        if let focus = screen.focus { lines.append("  focus \(focus.target) \(focus.serial)") }
        if let question = screen.question { lines.append("  question \(question.id) \(question.title)") }
    }
    lines.append("messages \(messages.map(\.id))")
    return lines.joined(separator: "\n")
}

extension NativeClient {
    /// What the model holds now.
    var fingerprint: String {
        NavreeRuntimeTests.fingerprint(
            app: app, nav: nav, screens: screens.mapValues(\.state), messages: messages
        )
    }
}

extension Core {
    /// The whole view state read at once.
    func wholeFingerprint() throws -> String {
        let whole = view()
        let state = try WireDecoder.decode(
            ViewState.self, from: Repair.withoutUnknownStates(whole) ?? whole, limit: NativeClient.nesting
        )
        return NavreeRuntimeTests.fingerprint(
            app: state.app, nav: state.nav, screens: state.screens, messages: state.messages
        )
    }
}

func json(_ value: Any) -> Data {
    (try? JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed])) ?? Data()
}

func object(_ data: Data) -> [String: Any] {
    (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
}
