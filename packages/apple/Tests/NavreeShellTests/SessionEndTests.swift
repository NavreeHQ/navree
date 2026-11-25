import Foundation
import NavreeCoreBridge
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

/// A web view's cookie store that lives in the test.
@MainActor
final class FakeWebCookies: WebCookies {
    var cookies: [HTTPCookie] = []

    func held() async -> [HTTPCookie] { cookies }

    func put(_ cookie: HTTPCookie) async {
        await drop(cookie)
        cookies.append(cookie)
    }

    func drop(_ cookie: HTTPCookie) async {
        let key = CookieRecord(cookie).key
        cookies.removeAll { CookieRecord($0).key == key }
    }

    var names: [String] { cookies.map(\.name).sorted() }
}

/// A Keychain item that lives in the test, and can refuse to be deleted.
final class MemorySlot: SecretSlot, @unchecked Sendable {
    private let lock = NSLock()
    private var kept: Data?
    private var refuses = false

    init(holding cookies: [CookieRecord] = []) {
        if !cookies.isEmpty { kept = try? JSONEncoder().encode(cookies) }
    }

    /// From now on the item cannot be deleted, as when the system refuses.
    func refuseDeleting() {
        lock.withLock { refuses = true }
    }

    /// The item can be deleted again.
    func allowDeleting() {
        lock.withLock { refuses = false }
    }

    /// The values of the cookies the item holds.
    func values() -> [String] {
        guard let data = read(), let cookies = try? JSONDecoder().decode([CookieRecord].self, from: data) else { return [] }
        return cookies.map(\.value).sorted()
    }

    func read() -> Data? {
        lock.withLock { kept }
    }

    func write(_ data: Data) {
        lock.withLock { kept = data }
    }

    func delete() -> Bool {
        lock.withLock {
            if refuses { return false }
            kept = nil
            return true
        }
    }
}

/// What a test saw asked of the system's registration for push.
@MainActor
final class PlayedPush: PushToken {
    var asks = 0
    var givenUp = 0

    func ask() { asks += 1 }
    func giveUp() { givenUp += 1 }
}

/// The web view stores a test saw removed.
@MainActor
final class RemovedStores {
    var ids: [UUID] = []
    /// What a removal answers: whether the store is gone.
    var succeeds = true
}

/// The network's transport, with every answer it gave and how often it was
/// told the session ended kept for the test to see.
@MainActor
final class WatchedTransport: Transport {
    let inner: HTTPTransport
    var answered: [Response] = []
    var stops = 0

    init(_ inner: HTTPTransport) {
        self.inner = inner
    }

    var jar: CookieJar? { inner.jar }

    func fetch(_ request: FetchRequest, answer: @escaping @MainActor (Response) -> Void) {
        inner.fetch(request) { response in
            self.answered.append(response)
            answer(response)
        }
    }

    func cancel(_ request: Int) {
        inner.cancel(request)
    }

    func open(
        stream: Int, url: String, headers: [(String, String)], report: @escaping @MainActor (CoreEvent.StreamState) -> Void
    ) {
        inner.open(stream: stream, url: url, headers: headers, report: report)
    }

    func close(stream: Int) {
        inner.close(stream: stream)
    }

    func stop() {
        stops += 1
        inner.stop()
    }
}

/// Two windows of one app: a core, a client and effects each, over the app's one session.
@MainActor
struct TwoWindows {
    struct Window {
        let client: NativeClient
        let effects: SystemEffects
        let transport: FakeTransport
    }

    let keeper: SessionKeeper
    let jar: CookieJar
    let first: Window
    let second: Window
    let folder: URL

    init() throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-windows-\(UUID().uuidString)")
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        self.jar = jar
        let keeper = SessionKeeper(
            jar: jar, cache: KeptFiles(folder: folder.appendingPathComponent("cache")),
            snapshots: KeptFiles(folder: folder.appendingPathComponent("snapshot")),
            files: FileStore(folder: folder.appendingPathComponent("files"))
        )
        self.keeper = keeper
        func window() throws -> Window {
            let configuration = ShellConfiguration(origin: "https://shop.example", appID: "com.navree.demo", appVersion: "1.0")
            let core = try #require(Core(config: CoreConfiguration.json(configuration, device: DeviceTraits())))
            let transport = FakeTransport()
            transport.jar = jar
            transport.answers = Shell.store
            let effects = SystemEffects(transport: transport, keeper: keeper)
            return Window(client: NativeClient(core: core, effects: effects), effects: effects, transport: transport)
        }
        first = try window()
        second = try window()
    }
}

@MainActor
struct SessionEndTests {
    static let address = URL(string: "https://shop.example/orders")!
    static let shell = "https://shop.example/_navree/native/shell"

    static func answer(setting cookie: String) -> HTTPURLResponse {
        HTTPURLResponse(url: address, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: ["Set-Cookie": cookie])!
    }

    static func cookie(_ name: String, _ value: String) -> CookieRecord {
        CookieRecord(name: name, value: value, domain: "shop.example")
    }

    /// A folder of the test's own, with the places a session keeps things in.
    struct Places {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-session-\(UUID().uuidString)")
        var cache: KeptFiles { KeptFiles(folder: folder.appendingPathComponent("cache")) }
        var snapshots: KeptFiles { KeptFiles(folder: folder.appendingPathComponent("snapshot")) }
        var record: SessionRecord { SessionRecord(folder: folder.appendingPathComponent("session")) }
        @MainActor var files: FileStore { FileStore(folder: folder.appendingPathComponent("files")) }

        func remove() {
            try? FileManager.default.removeItem(at: folder)
        }
    }

    // MARK: - Every window

    @Test func signingOutInOneWindowEndsTheSessionInEveryWindow() throws {
        let app = try TwoWindows()
        defer { try? FileManager.default.removeItem(at: app.folder) }
        app.first.client.begin()
        app.second.client.begin()
        #expect(app.second.client.app?.user?.name == "Ravneet Kaur")
        app.jar.merge([Self.cookie("session", "ravneet")])
        app.keeper.cache.write("page:https://shop.example/orders", "Ravneet's orders")
        app.keeper.snapshots.write("snapshot-first", "what she typed in the first window")
        app.keeper.snapshots.write("snapshot-second", "what she typed in the second window")
        // From here on the server answers the second window nothing, so what it shows is what its core holds.
        app.second.transport.answers = [:]
        let asked = app.second.transport.fetched.count

        // Ravneet signs out in the first window.
        app.first.effects.run(.forgetSession, for: app.first.client)

        #expect(app.keeper.number == 1)
        #expect(app.jar.header(for: Self.address) == nil)
        // Nothing kept of her is left, for either window.
        #expect(app.keeper.cache.read("page:https://shop.example/orders") == nil)
        #expect(app.keeper.snapshots.read("snapshot-first") == nil)
        #expect(app.keeper.snapshots.read("snapshot-second") == nil)
        // The second window's work under way was dropped, its core was told, and it starts again as nobody's.
        #expect(app.second.transport.stopped == 1)
        #expect(app.second.effects.session == 1)
        #expect(app.second.client.app?.user == nil)
        #expect(app.second.client.nav.stage == .starting)
        #expect(app.second.transport.fetched.dropFirst(asked).map(\.url).contains(Self.shell))
        // The first window goes on in the session that follows.
        #expect(app.first.effects.isCurrent)

        // An answer to a request the second window sent while she was signed in sets nothing now.
        app.jar.take(from: Self.answer(setting: "session=ravneet; Path=/; Secure"), for: Self.address, session: 0)
        #expect(app.jar.header(for: Self.address) == nil)
    }

    @Test func aCookieThatAnAnswerOfAnEndedSessionSetsIsDropped() {
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        jar.merge([Self.cookie("session", "a")])
        let asked = jar.session
        #expect(jar.forget())
        #expect(jar.header(for: Self.address) == nil)
        #expect(jar.session == asked + 1)
        // An answer to a request sent before the session ended.
        jar.take(from: Self.answer(setting: "session=b; Path=/; Secure"), for: Self.address, session: asked)
        #expect(jar.header(for: Self.address) == nil)
        // One sent after.
        jar.take(from: Self.answer(setting: "session=c; Path=/; Secure"), for: Self.address, session: jar.session)
        #expect(jar.header(for: Self.address) == "session=c")
    }

    @Test func aRequestUnderWayWhenTheSessionEndsIsNeverAnswered() async throws {
        let host = "stopped.shop.example"
        StubOrigin.answer(host, .init(status: 200, headers: ["Set-Cookie": "session=late; Path=/; Secure"], body: "{}"))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        let orders = try #require(URL(string: "https://\(host)/orders"))
        let answers = Answered()
        transport.fetch(FetchRequest(request: 4, url: orders.absoluteString)) { _ in answers.count += 1 }
        transport.stop()
        try await Task.sleep(for: .milliseconds(300))
        #expect(answers.count == 0)
        #expect(jar.header(for: orders) == nil)
    }

    @MainActor
    final class Answered {
        var count = 0
    }

    @Test func signingOutEmptiesWhatWasKeptOfThePersonBeforeTheNextEffect() throws {
        let shell = try Shell()
        shell.effects.cache.write("page:https://shop.example/orders", "kept")
        shell.effects.snapshots.write("snapshot", "kept")
        shell.transport.jar?.merge([Self.cookie("session", "a")])
        shell.effects.run(.forgetSession, for: shell.client)
        #expect(shell.effects.cache.read("page:https://shop.example/orders") == nil)
        #expect(shell.effects.snapshots.read("snapshot") == nil)
        #expect(shell.transport.jar?.header(for: Self.address) == nil)
        // A build not signed for push has no token to give up.
        #expect(!shell.effects.pushAwaitsSignIn)
    }

    @Test func thePicturesKeptForASessionGoWithIt() async throws {
        let host = "kept-pictures.shop.example"
        StubOrigin.answer(host, .init(status: 200, headers: ["Content-Type": "image/png"], body: "png"))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        let pictures = PictureLoader(jar: jar, configuration: StubOrigin.configuration)
        let url = try #require(URL(string: "https://\(host)/storage/a.png"))
        #expect(await pictures.load(url) != nil)
        #expect(await pictures.load(url) != nil)
        #expect(StubOrigin.seen(host).count == 1, "the second came from what was kept")
        pictures.forget()
        #expect(await pictures.load(url) != nil)
        #expect(StubOrigin.seen(host).count == 2, "nothing was kept after the session ended")
    }

    // MARK: - A sign-out whose core is still asking the server

    @Test func theSessionEndsOnTheDeviceBeforeTheTokensRemovalIsAnsweredAndTheRemovalStillGoesOutWithItsCookie() async throws {
        let host = "ending.shop.example"
        StubOrigin.answer(host, .init(status: 204, headers: ["Set-Cookie": "session=back-again; Path=/; Secure"], body: ""))
        let places = Places()
        defer { places.remove() }
        let slot = MemorySlot()
        let keeper = SessionKeeper.open(
            origin: "https://\(host)", slot: slot, cache: places.cache, snapshots: places.snapshots, files: places.files,
            record: places.record, removes: { _ in true }
        )
        let jar = keeper.networkJar()
        jar.merge([CookieRecord(name: "session", value: "gurpreet", domain: host)])
        places.cache.write("page:https://\(host)/orders", "Gurpreet's orders")
        places.snapshots.write("snapshot-first", "what Gurpreet typed")
        // The window Gurpreet signs out in, and a second window of the app.
        let transport = WatchedTransport(HTTPTransport(jar: jar, configuration: StubOrigin.configuration))
        let window = SystemEffects(transport: transport, keeper: keeper)
        let other = FakeTransport()
        other.jar = jar
        let second = SystemEffects(transport: other, keeper: keeper)
        let configuration = ShellConfiguration(origin: "https://\(host)", appID: "com.navree.demo", appVersion: "1.0")
        let client = NativeClient(
            core: try #require(Core(config: CoreConfiguration.json(configuration, device: DeviceTraits()))), effects: window
        )
        let push = try #require(URL(string: "https://\(host)/_navree/native/push"))

        // The core's update, carried out in its order: the request that takes
        // the push token back, then the word that the session is ending.
        let removal = FetchRequest(
            request: 9, method: "delete", url: push.absoluteString, headers: [("content-type", "application/json")],
            body: .json(#"{"token":"ab12"}"#)
        )
        window.run(.fetch(removal), for: client)
        window.run(.sessionEnding, for: client)

        // At once, with the removal still out: the mark is written, and
        // everything of Gurpreet's on the device is gone, for every window.
        #expect(transport.answered.isEmpty)
        #expect(places.record.owesWipe)
        #expect(keeper.number == 1)
        #expect(slot.read() == nil)
        #expect(jar.header(for: push) == nil)
        #expect(places.cache.read("page:https://\(host)/orders") == nil)
        #expect(places.snapshots.read("snapshot-first") == nil)
        #expect(other.stopped == 1)
        #expect(second.session == 1)
        // The window that is signing out keeps its requests: the removal is one of them.
        #expect(transport.stops == 0)

        // The removal left with the cookie it was made with, and is answered.
        for _ in 0..<200 where transport.answered.isEmpty {
            try await Task.sleep(for: .milliseconds(20))
        }
        #expect(transport.answered.map(\.status) == [204])
        #expect(StubOrigin.seen(host).last?.value(forHTTPHeaderField: "Cookie") == "session=gurpreet")
        // What its answer sets is an ended session's: refused.
        #expect(jar.header(for: push) == nil)
        #expect(slot.read() == nil)

        // The core then says the session is forgotten: nothing is done twice, and the mark goes.
        window.run(.forgetSession, for: client)
        #expect(keeper.number == 1)
        #expect(other.stopped == 1)
        #expect(transport.stops == 0)
        #expect(!places.record.owesWipe)
    }

    @Test func anAppEndedWhileTheTokensRemovalIsOutFinishesTheSignOutAtTheNextStart() throws {
        let places = Places()
        defer { places.remove() }
        let slot = MemorySlot()
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        keeper.networkJar().merge([Self.cookie("session", "gurpreet")])
        let transport = FakeTransport()
        transport.jar = keeper.networkJar()
        let window = SystemEffects(transport: transport, keeper: keeper)
        keeper.ending(by: window)
        #expect(places.record.owesWipe)
        #expect(slot.read() == nil)
        // The app is ended here: its core never says the session is forgotten.
        // Whatever a last moment of that run left behind, the next start takes away before it reads anything.
        slot.write(try JSONEncoder().encode([Self.cookie("session", "left-behind")]))
        places.cache.write("page:https://shop.example/orders", "left behind")
        let next = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        #expect(next.finishedAtStart)
        #expect(slot.read() == nil)
        #expect(next.networkJar().header(for: Self.address) == nil)
        #expect(places.cache.read("page:https://shop.example/orders") == nil)
        #expect(!places.record.owesWipe)
    }

    @Test func aSignOutSaidInOneWordStillEndsTheSessionAndAnotherWindowsWordEndsItAgain() throws {
        let places = Places()
        defer { places.remove() }
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: MemorySlot(), cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        let first = SystemEffects(transport: FakeTransport(), keeper: keeper)
        let second = SystemEffects(transport: FakeTransport(), keeper: keeper)
        // No push token: the core says only that the session is forgotten.
        keeper.end(by: first)
        #expect(keeper.number == 1)
        #expect(!places.record.owesWipe)
        // One window's core says the session is ending, and the other's says it is forgotten.
        keeper.ending(by: first)
        #expect(keeper.number == 2)
        keeper.end(by: second)
        #expect(keeper.number == 3)
        #expect(!places.record.owesWipe)
        // The first one's word, when it comes, ends nothing more: the session
        // it speaks of ended when its core said it was ending. (It was a
        // session's end of its own before; a core says the word once for each
        // time it said a session was ending, also after it was told that the
        // session ended meanwhile, so the word is that sign-out's and no other's.)
        keeper.end(by: first)
        #expect(keeper.number == 3)
        #expect(keeper.passedOver == 1)
    }

    @Test func aCoresWordThatTheSessionIsForgottenEndsNothingItsOwnSignOutAlreadyEnded() throws {
        let app = try TwoWindows()
        defer { try? FileManager.default.removeItem(at: app.folder) }
        // Neither core is started: each effect below is said for it, as its core would say it.
        let shell = FetchRequest(request: 9, url: Self.shell)
        app.first.transport.answers = [:]
        app.second.transport.answers = [:]

        // The first window's core says the session is ending, and waits for
        // the push token's removal. Before that is answered the second
        // window's core says the session is forgotten: it ends for every window.
        app.first.effects.run(.sessionEnding, for: app.first.client)
        #expect(app.keeper.number == 1)
        app.second.effects.run(.forgetSession, for: app.second.client)
        #expect(app.keeper.number == 2)
        let stopped = app.second.transport.stopped
        // Somebody signs in after that.
        app.jar.merge([Self.cookie("session", "simran")])
        app.keeper.snapshots.write("snapshot-second", "what Simran typed")
        // A core told that the session ended while it waited says in answer
        // that the session is forgotten, then asks for the shell. That word
        // is its own sign-out's: nothing is taken away twice, and nothing of
        // whoever signed in since.
        app.first.effects.run(.forgetSession, for: app.first.client)
        app.first.effects.run(.fetch(shell), for: app.first.client)
        #expect(app.keeper.number == 2)
        #expect(app.keeper.passedOver == 1)
        #expect(app.jar.header(for: Self.address) == "session=simran")
        #expect(app.keeper.snapshots.read("snapshot-second") == "what Simran typed")
        #expect(app.second.transport.stopped == stopped)

        // A core that says nothing in answer to being told, and only asks for
        // the shell, has said all it will of that sign-out: when it later
        // says the session is forgotten, in one word, that is a sign-out of
        // its own, and ends the session.
        app.first.effects.run(.sessionEnding, for: app.first.client)
        app.second.effects.run(.forgetSession, for: app.second.client)
        #expect(app.keeper.number == 4)
        app.first.effects.run(.fetch(shell), for: app.first.client)
        app.jar.merge([Self.cookie("session", "harleen")])
        app.first.effects.run(.forgetSession, for: app.first.client)
        #expect(app.keeper.number == 5)
        #expect(app.keeper.passedOver == 1)
        #expect(app.jar.header(for: Self.address) == nil)
    }

    @Test func aWindowToldTheSessionEndedWhileItsOwnSignOutWaitedMovesTheSessionOnce() throws {
        let app = try TwoWindows()
        defer { try? FileManager.default.removeItem(at: app.folder) }
        app.first.client.begin()
        app.second.client.begin()
        // The second window's core is waiting on its sign-out when the first
        // window's ends the session and tells it. Whatever a core says in
        // answer to being told (nothing, or that the session is forgotten),
        // the session ends once there.
        app.second.effects.run(.sessionEnding, for: app.second.client)
        #expect(app.keeper.number == 1)
        app.first.effects.run(.forgetSession, for: app.first.client)
        #expect(app.keeper.number == 2)
        #expect(app.second.effects.session == 2)
    }

    @Test func anEffectThisBuildDoesNotKnowChangesNothing() throws {
        let shell = try Shell()
        shell.transport.jar?.merge([Self.cookie("session", "a")])
        shell.effects.cache.write("page:https://shop.example/orders", "kept")
        shell.effects.run(.unknown("from_a_newer_core"), for: shell.client)
        #expect(shell.effects.keeper.number == 0)
        #expect(shell.transport.jar?.header(for: Self.address) == "session=a")
        #expect(shell.effects.cache.read("page:https://shop.example/orders") == "kept")
        #expect(shell.transport.stopped == 0)
    }

    // MARK: - Another person, with no sign-out between

    @Test func anotherPersonSigningInWithNoSignOutTakesAwayWhatWasTheLastPersons() throws {
        let shell = try Shell()
        let played = PlayedNotifications()
        shell.effects.devices.notifications = played
        shell.client.begin()
        #expect(shell.client.app?.user?.name == "Ravneet Kaur")
        let loaded = shell.effects.pictures.era
        // Her session runs out while the app is away and Harpreet signs in on
        // the same device: the server's shell names him now.
        shell.transport.answers["/_navree/native/shell"] = (200, "made/shell-other.json")
        shell.client.send(.foreground)
        #expect(shell.client.app?.user?.name == "Harpreet Singh")
        #expect(played.signOuts == 1, "her notifications and the count go")
        #expect(shell.effects.pictures.era == loaded + 1, "a picture loaded for her is asked for again")
        // The same person named again takes nothing away.
        shell.client.send(.foreground)
        #expect(played.signOuts == 1)
    }

    @Test func anotherPersonSigningInWithNoSignOutIsToldToEveryWindow() throws {
        let app = try TwoWindows()
        defer { try? FileManager.default.removeItem(at: app.folder) }
        app.first.client.begin()
        app.second.client.begin()
        #expect(app.second.client.app?.user?.name == "Ravneet Kaur")
        app.keeper.snapshots.write("snapshot-second", "what Ravneet typed in the second window")
        let asked = app.second.transport.fetched.count
        // Her session runs out, and Harpreet signs in in the first window:
        // the server's shell names him now, whichever window asks.
        app.first.transport.answers["/_navree/native/shell"] = (200, "made/shell-other.json")
        app.second.transport.answers["/_navree/native/shell"] = (200, "made/shell-other.json")
        app.first.client.send(.foreground)
        #expect(app.first.client.app?.user?.name == "Harpreet Singh")
        // The session moved on for every window, and what was kept of hers is gone.
        #expect(app.keeper.number == 1)
        #expect(app.first.effects.isCurrent)
        #expect(app.keeper.snapshots.read("snapshot-second") == nil)
        // The second window's requests were dropped and its core told: it
        // let go of her screens and of what she typed, started again from
        // the shell, and names him too. Nothing of hers is left to send as him.
        #expect(app.second.transport.stopped == 1)
        #expect(app.second.effects.session == 1)
        #expect(app.second.transport.fetched.dropFirst(asked).map(\.url).contains(Self.shell))
        #expect(app.second.client.app?.user?.name == "Harpreet Singh")
        // Hearing him named there changes nothing again.
        #expect(app.keeper.number == 1)
    }

    @Test func aPersonsMarkTellsTwoPeopleApartAndSaysNothingOfEither() {
        let mark = SessionKeeper.mark(name: "Ravneet Kaur", email: "ravneet@charkha.example")
        #expect(mark == SessionKeeper.mark(name: "Ravneet Kaur", email: "ravneet@charkha.example"))
        #expect(mark.count == 64)
        #expect(!mark.localizedCaseInsensitiveContains("ravneet"))
        #expect(mark != SessionKeeper.mark(name: "Ravneet Kaur", email: "ravneet@example.in"))
        #expect(mark != SessionKeeper.mark(name: "Ravneet K. Sandhu", email: "ravneet@charkha.example"))
        // Somebody with no address and somebody whose address is empty are two.
        #expect(SessionKeeper.mark(name: "Ravneet Kaur", email: nil) != SessionKeeper.mark(name: "Ravneet Kaur", email: ""))
        // Where the name ends is part of the mark, so a name never runs into an address.
        #expect(SessionKeeper.mark(name: "a+b", email: "c") != SessionKeeper.mark(name: "a", email: "b+c"))
    }

    @Test func aChangeOfPersonAcrossARelaunchIsSeenAtTheNextStartAndASignOutForgetsWhoWasNamed() throws {
        let places = Places()
        defer { places.remove() }
        let slot = MemorySlot()
        // A core that gives no key for a person: each is known by the mark of their name and address.
        let gurpreet = SessionKeeper.Person(
            key: nil, mark: SessionKeeper.mark(name: "Gurpreet Kaur", email: "gurpreet@charkha.example")
        )
        let simran = SessionKeeper.Person(
            key: nil, mark: SessionKeeper.mark(name: "Simran Sandhu", email: "simran@charkha.example")
        )
        /// A start of the app: the session opened from what the device holds, and one window in it.
        func start() -> (keeper: SessionKeeper, window: SystemEffects, played: PlayedNotifications) {
            let keeper = SessionKeeper.open(
                origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
                files: places.files, record: places.record, removes: { _ in true }
            )
            let window = SystemEffects(transport: FakeTransport(), keeper: keeper)
            let played = PlayedNotifications()
            window.devices.notifications = played
            return (keeper, window, played)
        }

        // Gurpreet is named, and something is kept of her window.
        var run = start()
        run.keeper.hears(gurpreet, from: run.window)
        #expect(run.played.signOuts == 0)
        #expect(places.record.person() == gurpreet)
        places.snapshots.write("snapshot-first", "what Gurpreet typed")

        // The next start names her again: nothing changes.
        run = start()
        #expect(run.keeper.person == gurpreet)
        run.keeper.hears(gurpreet, from: run.window)
        #expect(run.played.signOuts == 0)
        #expect(run.keeper.number == 0)
        #expect(places.snapshots.read("snapshot-first") == "what Gurpreet typed")

        // Her session runs out while the app is closed, and at the start
        // after it Simran signs in: what was kept for Gurpreet goes, her
        // reminders among it, and the mark written down is Simran's.
        run = start()
        run.keeper.hears(simran, from: run.window)
        #expect(run.played.signOuts == 1)
        #expect(run.keeper.number == 1)
        #expect(places.snapshots.read("snapshot-first") == nil)
        #expect(places.record.person() == simran)

        // A sign-out forgets who was named: whoever signs in after it is
        // compared with nobody, and loses nothing.
        run.keeper.end(by: run.window)
        #expect(places.record.person() == nil)
        #expect(run.keeper.person == nil)
        let before = (number: run.keeper.number, signOuts: run.played.signOuts)
        run.keeper.hears(gurpreet, from: run.window)
        #expect(run.keeper.number == before.number)
        #expect(run.played.signOuts == before.signOuts)
        #expect(places.record.person() == gurpreet)
    }

    @Test func aCoresUserIsKnownByItsKeyWhenItHasOneAndByItsNameAndAddressOnlyWhenItHasNone() {
        let mark = SessionKeeper.mark(name: "Harpreet Singh", email: nil)
        // Two accounts of one name and no address, as a core that gives keys names them.
        let first = SystemEffects.person(Navree.User(key: "person-1f0c", name: "Harpreet Singh"))
        let second = SystemEffects.person(Navree.User(key: "person-77ab", name: "Harpreet Singh"))
        #expect(first == SessionKeeper.Person(key: "person-1f0c", mark: mark))
        #expect(!first.isSame(as: second))
        #expect(first.isSame(as: SystemEffects.person(Navree.User(key: "person-1f0c", name: "Harpreet S. Gill"))))
        // A core that gives no key, or an empty one: the name and the address alone.
        let plain = SystemEffects.person(Navree.User(name: "Harpreet Singh"))
        #expect(plain == SessionKeeper.Person(key: nil, mark: mark))
        #expect(SystemEffects.person(Navree.User(key: "", name: "Harpreet Singh")) == plain)
        #expect(plain.isSame(as: SystemEffects.person(Navree.User(name: "Harpreet Singh"))))
        // One known by a key and one known by name are never the same
        // person, though the name and the address are the same.
        #expect(!plain.isSame(as: first))
        #expect(!first.isSame(as: plain))
    }

    @Test func twoPeopleOfOneNameAreToldApartByTheCoresKeyAndAPersonWhoChangesTheirNameStaysTheSame() throws {
        let places = Places()
        defer { places.remove() }
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: MemorySlot(), cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        let window = SystemEffects(transport: FakeTransport(), keeper: keeper)
        let played = PlayedNotifications()
        window.devices.notifications = played
        // Two accounts are named Harpreet Singh, and neither has an address: one mark for both.
        let mark = SessionKeeper.mark(name: "Harpreet Singh", email: nil)

        // The core names the first of them, with his key.
        let first = SessionKeeper.Person(key: "person-1f0c", mark: mark)
        keeper.hears(first, from: window)
        #expect(places.record.person() == first)
        // He changes the name he is shown by: the same key is the same
        // person, nothing is taken away, and he is written down as he is named now.
        let renamed = SessionKeeper.Person(key: "person-1f0c", mark: SessionKeeper.mark(name: "Harpreet S. Gill", email: nil))
        keeper.hears(renamed, from: window)
        #expect(played.signOuts == 0)
        #expect(keeper.number == 0)
        #expect(places.record.person() == renamed)
        keeper.hears(first, from: window)
        #expect(played.signOuts == 0)

        // His session runs out, and the other Harpreet Singh signs in: the
        // same name, and another key. He is another person, and what was
        // kept for the first one goes.
        places.snapshots.write("snapshot-first", "what the first Harpreet typed")
        let second = SessionKeeper.Person(key: "person-77ab", mark: mark)
        keeper.hears(second, from: window)
        #expect(played.signOuts == 1)
        #expect(keeper.number == 1)
        #expect(places.snapshots.read("snapshot-first") == nil)
        #expect(places.record.person() == second)
    }

    @Test func aPersonWrittenDownByNameIsNeverTakenForOneNamedWithAKeyNorTheOtherWayRound() throws {
        let places = Places()
        defer { places.remove() }
        let slot = MemorySlot()
        // One name and one address throughout: only what the person is known by differs.
        let mark = SessionKeeper.mark(name: "Gurpreet Kaur", email: "gurpreet@charkha.example")
        let byName = SessionKeeper.Person(key: nil, mark: mark)
        let byKey = SessionKeeper.Person(key: "person-9a04", mark: mark)

        // The app as it was with a core that gave no key: she is written
        // down by the mark of her name and address, and her place is kept.
        let before = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        before.hears(byName, from: SystemEffects(transport: FakeTransport(), keeper: before))
        places.snapshots.write("snapshot-first", "what was typed under that name")
        #expect(places.record.person() == byName)

        // The app is updated to a core that gives keys, and the first person
        // it names has that very name and address. A name is no ground for
        // handing whoever signs in at the update what was kept before it:
        // the first key is another person's, and everything goes, once.
        let after = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        let window = SystemEffects(transport: FakeTransport(), keeper: after)
        let played = PlayedNotifications()
        window.devices.notifications = played
        after.hears(byKey, from: window)
        #expect(played.signOuts == 1)
        #expect(after.number == 1)
        #expect(places.snapshots.read("snapshot-first") == nil)
        #expect(places.record.person() == byKey)
        // From then on the key is what she is known by: named again, nothing goes.
        after.hears(byKey, from: window)
        #expect(played.signOuts == 1)
        #expect(after.number == 1)

        // And the other way round: a person written down with a key, and a
        // state that names the same name and address with none.
        places.snapshots.write("snapshot-first", "what was typed under that key")
        after.hears(byName, from: window)
        #expect(played.signOuts == 2)
        #expect(after.number == 2)
        #expect(places.snapshots.read("snapshot-first") == nil)
        #expect(places.record.person() == byName)
    }

    @Test func aPersonWrittenDownByAPlainKeyIsTheSameUnderTheKeyMadeWithTheInstallsSecretAndNobodyElseIs() throws {
        // Her key as the release before gave it, and as the core makes it
        // with the secret of an install; and those of another account of
        // her name and address. They are the keys the core's conversations
        // person-key-steady.json and person-key-was.json record.
        let ravneetPlain = "1c54d473261f6f17cb6193202ce56588716e85757946078ac43d03b22f93a35e"
        let ravneetKeyed = "k2:17d27bf8db683479c652471e31c96ac7d03e690c41c4f94f56ca99de886e389e"
        let otherPlain = "1bc3dcdc8ebf5d231237007ccc4903b602f64185813b65bc5b6d1f5f798fcab7"
        let otherKeyed = "k2:6bd1a2cdf43aaf682ec4d5da88fc8ca12d82976ffebb660e10e75f894e0c82d7"
        let mark = SessionKeeper.mark(name: "Ravneet Kaur", email: "ravneet.kaur@example.com")
        let plain = SessionKeeper.Person(key: ravneetPlain, mark: mark)
        let keyed = SessionKeeper.Person(key: ravneetKeyed, mark: mark)
        let other = SessionKeeper.Person(key: otherKeyed, mark: mark)
        let byName = SessionKeeper.Person(key: nil, mark: mark)

        // Which key is a plain one, and when two keys are one person's.
        #expect(plain.keepsPlainKey)
        #expect(!keyed.keepsPlainKey)
        #expect(!byName.keepsPlainKey)
        #expect(plain.isPlain(of: keyed, was: ravneetPlain))
        #expect(!plain.isSame(as: keyed))
        // A core that says no plain key, or another account's, leaves them two people.
        #expect(!plain.isPlain(of: keyed, was: nil))
        #expect(!plain.isPlain(of: keyed, was: ""))
        #expect(!plain.isPlain(of: other, was: otherPlain))
        // Only a plain key written down is held to a key made with the
        // secret: not one plain key to another, not one key made with the
        // secret to another, and not a person written down by name.
        #expect(!plain.isPlain(of: SessionKeeper.Person(key: otherPlain, mark: mark), was: ravneetPlain))
        #expect(!keyed.isPlain(of: other, was: ravneetKeyed))
        #expect(!byName.isPlain(of: keyed, was: ravneetPlain))

        /// The release before: she is written down by her plain key, and her place is kept.
        func written() -> (places: Places, slot: MemorySlot) {
            let places = Places()
            let slot = MemorySlot()
            let before = SessionKeeper.open(
                origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
                files: places.files, record: places.record, removes: { _ in true }
            )
            before.hears(plain, from: SystemEffects(transport: FakeTransport(), keeper: before))
            places.snapshots.write("snapshot-first", "what Ravneet typed")
            return (places, slot)
        }
        /// The first start after the update: the session opened from what the device holds, and one window in it.
        func start(_ kept: (places: Places, slot: MemorySlot)) -> (keeper: SessionKeeper, window: SystemEffects, played: PlayedNotifications) {
            let keeper = SessionKeeper.open(
                origin: "https://shop.example", slot: kept.slot, cache: kept.places.cache, snapshots: kept.places.snapshots,
                files: kept.places.files, record: kept.places.record, removes: { _ in true }
            )
            let window = SystemEffects(transport: FakeTransport(), keeper: keeper)
            let played = PlayedNotifications()
            window.devices.notifications = played
            return (keeper, window, played)
        }

        // The core names her by the key it makes now, and says what her key
        // was: she is the person written down. Nothing goes, and she is
        // written down by the key she has now.
        let hers = written()
        defer { hers.places.remove() }
        #expect(hers.places.record.person() == plain)
        var run = start(hers)
        #expect(run.keeper.keepsPlainKey)
        run.keeper.hears(keyed, was: ravneetPlain, from: run.window)
        #expect(run.played.signOuts == 0)
        #expect(run.keeper.number == 0)
        #expect(hers.places.snapshots.read("snapshot-first") == "what Ravneet typed")
        #expect(hers.places.record.person() == keyed)
        // For the rest of this run every core that is made is still asked,
        // so a window brought back later finds its place too.
        #expect(run.keeper.keepsPlainKey)
        // The start after reads her new key and asks the core for no plain
        // one; named again, with or without what her key was, nothing goes.
        run = start(hers)
        #expect(!run.keeper.keepsPlainKey)
        run.keeper.hears(keyed, from: run.window)
        run.keeper.hears(keyed, was: ravneetPlain, from: run.window)
        #expect(run.played.signOuts == 0)
        #expect(hers.places.snapshots.read("snapshot-first") == "what Ravneet typed")

        // Her session ran out, and another account of her name signs in at
        // the first start after the update: the core says that account's
        // own plain key, which is not the one written down. Everything goes.
        let left = written()
        defer { left.places.remove() }
        run = start(left)
        run.keeper.hears(other, was: otherPlain, from: run.window)
        #expect(run.played.signOuts == 1)
        #expect(run.keeper.number == 1)
        #expect(left.places.snapshots.read("snapshot-first") == nil)
        #expect(left.places.record.person() == other)

        // A core that does not say what her key was: the two keys are two people's.
        let unsaid = written()
        defer { unsaid.places.remove() }
        run = start(unsaid)
        run.keeper.hears(keyed, from: run.window)
        #expect(run.played.signOuts == 1)
        #expect(unsaid.places.snapshots.read("snapshot-first") == nil)
        #expect(unsaid.places.record.person() == keyed)
    }

    @Test func theInstallsSecretIsKeptInAKeychainItemOfItsOwnAndStaysThroughASignOut() throws {
        let places = Places()
        defer { places.remove() }
        let file = places.record.folder.appendingPathComponent("install")
        // The Keychain item the install's secret is kept in, beside the session's own.
        let item = MemorySlot()
        /// A start of the app over what the device holds.
        func start(keeping install: MemorySlot?) -> SessionKeeper {
            SessionKeeper.open(
                origin: "https://shop.example", slot: MemorySlot(), install: install, cache: places.cache,
                snapshots: places.snapshots, files: places.files, record: places.record, removes: { _ in true }
            )
        }

        // The first start makes it. It is what the item holds, and nothing of it is in the session's folder.
        let keeper = start(keeping: item)
        let made = keeper.installSecret
        #expect(made.count == 2 * SessionRecord.secretBytes)
        #expect(SessionRecord.reads(secret: made))
        #expect(item.read() == Data(made.utf8))
        #expect(!FileManager.default.fileExists(atPath: file.path))
        // Every window's core of the session is given the same one, and so is the next start.
        #expect(keeper.installSecret == made)
        #expect(start(keeping: item).installSecret == made)

        // What the core takes for a secret: hex, two characters a byte, of 32 bytes or more.
        #expect(!SessionRecord.reads(secret: String(made.dropLast(2))))
        #expect(!SessionRecord.reads(secret: made + "0"))
        #expect(!SessionRecord.reads(secret: "zz" + made.dropFirst(2)))
        #expect(SessionRecord.reads(secret: made + "0a"))

        // What the item holds that is no secret is replaced, and what replaces it stays.
        item.write(Data("not a secret".utf8))
        let again = start(keeping: item).installSecret
        #expect(SessionRecord.reads(secret: again))
        #expect(again != made)
        #expect(item.read() == Data(again.utf8))
        #expect(start(keeping: item).installSecret == again)

        // An item that is gone is made again.
        #expect(item.delete())
        let third = start(keeping: item)
        #expect(third.installSecret != again)
        #expect(item.read() == Data(third.installSecret.utf8))

        // A build with no Keychain of its own keeps none: each run has a
        // secret of its own for as long as it lives, and none is put in a file.
        let bare = start(keeping: nil)
        let its = bare.installSecret
        #expect(SessionRecord.reads(secret: its))
        #expect(bare.installSecret == its)
        #expect(start(keeping: nil).installSecret != its)
        #expect(!FileManager.default.fileExists(atPath: file.path))

        // A sign-out takes who was written down and the session's own item, and leaves the secret.
        let kept = third.installSecret
        let window = SystemEffects(transport: FakeTransport(), keeper: third)
        window.devices.notifications = PlayedNotifications()
        let ravneet = SessionKeeper.Person(
            key: "k2:17d27bf8db683479c652471e31c96ac7d03e690c41c4f94f56ca99de886e389e",
            mark: SessionKeeper.mark(name: "Ravneet Kaur", email: nil)
        )
        third.hears(ravneet, from: window)
        #expect(places.record.person() == ravneet)
        third.end(by: window)
        #expect(places.record.person() == nil)
        #expect(item.read() == Data(kept.utf8))
        #expect(third.installSecret == kept)
    }

    @Test func aSecretABuildBeforeKeptInAFileIsMovedIntoTheKeychainOnceAndTheFileIsDeleted() throws {
        let old = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
        /// A session's places with `text` in the file a build before kept the secret in.
        func left(_ text: String) throws -> (places: Places, file: URL) {
            let places = Places()
            let file = places.record.folder.appendingPathComponent("install")
            try FileManager.default.createDirectory(at: places.record.folder, withIntermediateDirectories: true)
            try Data(text.utf8).write(to: file)
            return (places, file)
        }
        func start(_ places: Places, keeping install: MemorySlot?) -> SessionKeeper {
            SessionKeeper.open(
                origin: "https://shop.example", slot: MemorySlot(), install: install, cache: places.cache,
                snapshots: places.snapshots, files: places.files, record: places.record, removes: { _ in true }
            )
        }
        func isThere(_ file: URL) -> Bool { FileManager.default.fileExists(atPath: file.path) }

        // The file's secret goes on being the install's, so the keys stay
        // what they were. The Keychain holds it now, and the file is gone.
        let moved = try left(old + "\n")
        defer { moved.places.remove() }
        #expect(moved.places.record.fileSecret() == old)
        let item = MemorySlot()
        #expect(start(moved.places, keeping: item).installSecret == old)
        #expect(item.read() == Data(old.utf8))
        #expect(!isThere(moved.file))
        #expect(start(moved.places, keeping: item).installSecret == old)

        // A file that turns up beside an item that holds a secret is not read, and goes.
        try Data(String(repeating: "ff", count: 32).utf8).write(to: moved.file)
        #expect(start(moved.places, keeping: item).installSecret == old)
        #expect(!isThere(moved.file))
        #expect(item.read() == Data(old.utf8))

        // A file that holds no secret is deleted too, and a secret is made.
        let broken = try left("not a secret")
        defer { broken.places.remove() }
        #expect(broken.places.record.fileSecret() == nil)
        let fresh = MemorySlot()
        let made = start(broken.places, keeping: fresh).installSecret
        #expect(SessionRecord.reads(secret: made))
        #expect(made != old)
        #expect(fresh.read() == Data(made.utf8))
        #expect(!isThere(broken.file))

        // With no Keychain to move it into, the file's secret is that run's
        // alone and the file is deleted all the same: a secret is kept in
        // the Keychain or not at all.
        let bare = try left(old)
        defer { bare.places.remove() }
        #expect(start(bare.places, keeping: nil).installSecret == old)
        #expect(!isThere(bare.file))
        #expect(start(bare.places, keeping: nil).installSecret != old)
    }

    @Test func withoutAKeyTwoPeopleOfOneNameAndNoAddressAreOneAndARenamedPersonIsAnother() throws {
        let places = Places()
        defer { places.remove() }
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: MemorySlot(), cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        let window = SystemEffects(transport: FakeTransport(), keeper: keeper)
        let played = PlayedNotifications()
        window.devices.notifications = played
        // A core that gives no key says only a name and an address. That is
        // the limit of telling people apart by them, in both directions.
        let harpreet = SessionKeeper.Person(key: nil, mark: SessionKeeper.mark(name: "Harpreet Singh", email: nil))
        keeper.hears(harpreet, from: window)
        // Another account of the same name and no address is not told from the first.
        keeper.hears(harpreet, from: window)
        #expect(played.signOuts == 0)
        #expect(keeper.number == 0)
        // And the same person under a new name is taken for another, once.
        let renamed = SessionKeeper.Person(key: nil, mark: SessionKeeper.mark(name: "Harpreet S. Gill", email: nil))
        keeper.hears(renamed, from: window)
        #expect(played.signOuts == 1)
        #expect(keeper.number == 1)
        keeper.hears(renamed, from: window)
        #expect(played.signOuts == 1)
        #expect(places.record.person() == renamed)
    }

    // MARK: - A mark and the session after it

    @Test func aSignOutsMarkNeverEndsTheSessionThatFollowsIt() throws {
        let places = Places()
        defer { places.remove() }
        let slot = MemorySlot()
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        let jar = keeper.networkJar()
        jar.merge([Self.cookie("session", "gurpreet")])
        let transport = FakeTransport()
        transport.jar = jar
        let window = SystemEffects(transport: transport, keeper: keeper)
        // Gurpreet signs out, and her core still waits for the push token's
        // removal: the mark stands, and names her session.
        keeper.ending(by: window)
        #expect(places.record.marked() == 0)
        #expect(places.record.owesWipe)
        #expect(slot.read() == nil)
        // Simran signs in before that wait is over: her cookie, and what is kept of her window.
        jar.merge([Self.cookie("session", "simran")])
        window.keep(snapshot: Data("what Simran typed".utf8), under: "snapshot-second")
        // The device's session moved on before either was written, so the
        // mark is an earlier session's now.
        #expect(places.record.serial() == 1)
        #expect(!places.record.owesWipe)
        #expect(slot.values() == ["simran"])
        // The app is ended there, and the core never says the session is
        // forgotten. The next start ends nobody's session.
        let next = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        #expect(!next.finishedAtStart)
        #expect(next.networkJar().header(for: Self.address) == "session=simran")
        #expect(places.snapshots.read("snapshot-second") == "what Simran typed")
        #expect(places.record.marked() == nil)
    }

    @Test func aSignOutThatCouldNotFinishKeepsTheNextSessionOffTheDeviceUntilItHas() throws {
        let places = Places()
        defer { places.remove() }
        let slot = MemorySlot()
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        let jar = keeper.networkJar()
        jar.merge([Self.cookie("session", "gurpreet")])
        let window = SystemEffects(transport: FakeTransport(), keeper: keeper)
        // The Keychain will not let her item go: the sign-out is not finished, and its mark stands.
        slot.refuseDeleting()
        keeper.end()
        #expect(jar.header(for: Self.address) == nil)
        #expect(places.record.owesWipe)
        #expect(slot.values() == ["gurpreet"])
        // Simran signs in. Her session is held in memory, and nothing of it
        // is written while what the device holds may still be Gurpreet's:
        // a start that found the mark would take it all for hers.
        jar.merge([Self.cookie("session", "simran")])
        window.keep(snapshot: Data("what Simran typed".utf8), under: "snapshot-second")
        #expect(jar.header(for: Self.address) == "session=simran")
        #expect(slot.values() == ["gurpreet"])
        #expect(places.snapshots.read("snapshot-second") == nil)
        #expect(keeper.storeIdentifier() == nil)
        #expect(places.record.owesWipe)
        // The item can be deleted at last: the next thing to write finishes
        // the sign-out first, and then Simran's session is kept.
        slot.allowDeleting()
        jar.merge([Self.cookie("theme", "dark")])
        #expect(!places.record.owesWipe)
        #expect(places.record.marked() == nil)
        #expect(slot.values() == ["dark", "simran"])
        #expect(keeper.storeIdentifier() != nil)
    }

    // MARK: - The push token

    @Test func thePushTokenIsAskedForOnlyWhileSomebodyIsSignedIn() throws {
        let shell = try Shell(signedWith: [.pushNotifications])
        let push = PlayedPush()
        shell.effects.push = push
        // The app has started and its core names nobody yet: nothing is asked of the system.
        shell.effects.begins(restoring: false)
        shell.effects.run(.cancel(request: 9), for: shell.client)
        #expect(push.asks == 0)
        // This launch's shell names Ravneet: the token is asked for, once.
        shell.client.begin()
        #expect(shell.client.app?.user?.name == "Ravneet Kaur")
        #expect(push.asks == 1)
        shell.effects.run(.cancel(request: 9), for: shell.client)
        #expect(push.asks == 1)
        // She signs out, in the two words of a core with a token to take
        // back: the token is given up once, at the first of them.
        shell.effects.run(.sessionEnding, for: shell.client)
        #expect(push.givenUp == 1)
        shell.effects.run(.forgetSession, for: shell.client)
        #expect(push.givenUp == 1)
        #expect(push.asks == 1)
        #expect(shell.effects.pushAwaitsSignIn)
    }

    @Test func aStartThatFinishesASignOutGivesThePushTokenUpAndAsksForNone() throws {
        let places = Places()
        defer { places.remove() }
        // The run before was ended in the middle of a sign-out.
        #expect(places.record.markWipe())
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: MemorySlot(), cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        #expect(keeper.finishedAtStart)
        let window = SystemEffects(transport: FakeTransport(), keeper: keeper, signedWith: [.pushNotifications])
        let push = PlayedPush()
        window.push = push
        window.begins(restoring: false)
        // The token that sign-out was giving up is given up, and none is asked for: nobody is signed in.
        #expect(push.givenUp == 1)
        #expect(push.asks == 0)
        #expect(window.pushAwaitsSignIn)
        // A second window of the same start gives nothing up again.
        let other = SystemEffects(transport: FakeTransport(), keeper: keeper, signedWith: [.pushNotifications])
        let more = PlayedPush()
        other.push = more
        other.begins(restoring: nil)
        #expect(more.givenUp == 0)
        #expect(more.asks == 0)
    }

    // MARK: - The web view's store

    @Test func theWebViewsStoreOfASessionThatEndedGivesTheNextOneNothing() async throws {
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        jar.merge([Self.cookie("session", "gurpreet")])
        let store = FakeWebCookies()
        await jar.ready(store, session: jar.session)
        #expect(store.names == ["session"])
        let ended = jar.session
        jar.forget()
        // The store was never emptied: the app was ended before it could be.
        // A page of the ended session finishes loading now.
        await jar.adopt(from: store, host: "shop.example", session: ended)
        #expect(jar.header(for: Self.address) == nil)
        // The next session takes nothing from a store it has not made its own.
        await jar.adopt(from: store, host: "shop.example", session: jar.session)
        #expect(jar.header(for: Self.address) == nil)
        // And before its first page loads, the store holds what the jar holds: nothing.
        await jar.ready(store, session: jar.session)
        #expect(store.cookies.isEmpty)
    }

    @Test func aCookieAnAnswerTookAwayDoesNotComeBackFromTheWebView() async throws {
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        jar.merge([Self.cookie("session", "a"), Self.cookie("remember", "always")])
        let store = FakeWebCookies()
        await jar.ready(store, session: jar.session)
        #expect(store.names == ["remember", "session"])
        // The server takes the remember-me cookie back in an answer to the app.
        var gone = Self.cookie("remember", "")
        gone.expires = Date(timeIntervalSince1970: 0)
        jar.merge([gone])
        // A page finishes; the web view's store still holds the cookie.
        await jar.adopt(from: store, host: "shop.example", session: jar.session)
        #expect(jar.records.map(\.name) == ["session"])
        // The next page's load takes it out of the store too.
        await jar.ready(store, session: jar.session)
        #expect(store.names == ["session"])
    }

    @Test func whatAPageSetIsTakenAndWhatAnAnswerSetMeanwhileStands() async throws {
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        jar.merge([Self.cookie("session", "a"), Self.cookie("theme", "dark")])
        let store = FakeWebCookies()
        await jar.ready(store, session: jar.session)
        // While the page is shown: an answer to the app renews the session, and
        // the page sets a cookie, takes one away, and one of another site is there too.
        jar.merge([Self.cookie("session", "b")])
        await store.put(try #require(Self.cookie("cart", "3").cookie))
        await store.drop(try #require(Self.cookie("theme", "dark").cookie))
        await store.put(try #require(CookieRecord(name: "tracker", value: "x", domain: "ads.example").cookie))
        await jar.adopt(from: store, host: "shop.example", session: jar.session)
        let held = Dictionary(uniqueKeysWithValues: jar.records.map { ($0.name, $0.value) })
        #expect(held == ["session": "b", "cart": "3"])
        // A page of another host gives the jar nothing.
        await store.put(try #require(Self.cookie("cart", "9").cookie))
        await jar.adopt(from: store, host: "ads.example", session: jar.session)
        #expect(jar.records.first { $0.name == "cart" }?.value == "3")
    }

    // MARK: - A sign-out that was cut short

    @Test func aSignOutCutShortIsFinishedAtTheNextStartBeforeAnythingIsRead() async throws {
        let places = Places()
        defer { places.remove() }
        let slot = MemorySlot(holding: [Self.cookie("session", "gurpreet")])
        places.cache.write("page:https://shop.example/orders", "Gurpreet's orders")
        places.snapshots.write("snapshot-first", "what Gurpreet typed")
        let hers = UUID()
        #expect(places.record.write(WebStoreList(current: hers)))
        // The sign-out began, and the app was ended before anything else was done.
        #expect(places.record.markWipe())

        let removed = RemovedStores()
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record,
            removes: { id in
                removed.ids.append(id)
                return removed.succeeds
            }
        )
        #expect(keeper.finishedAtStart)
        #expect(slot.read() == nil)
        #expect(keeper.networkJar().header(for: Self.address) == nil)
        #expect(places.cache.read("page:https://shop.example/orders") == nil)
        #expect(places.snapshots.read("snapshot-first") == nil)
        #expect(!places.record.owesWipe)
        // Her web view store is never used again, and is removed.
        let next = try #require(keeper.storeIdentifier())
        #expect(next != hers)
        await keeper.removal?.value
        #expect(removed.ids == [hers])
        #expect(places.record.stores() == WebStoreList(current: next))
    }

    @Test func aStartThatCannotFinishTheSignOutStillTakesNothingFromTheKeychain() throws {
        let places = Places()
        defer { places.remove() }
        let slot = MemorySlot(holding: [Self.cookie("session", "gurpreet")])
        slot.refuseDeleting()
        #expect(places.record.markWipe())
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        #expect(slot.read() != nil, "the item could not be deleted")
        #expect(keeper.networkJar().header(for: Self.address) == nil)
        // The mark stays for the next start, and no web view store is kept between launches meanwhile.
        #expect(places.record.owesWipe)
        #expect(keeper.storeIdentifier() == nil)
    }

    @Test func aStartWithNoSignOutOwedTakesTheSessionFromTheKeychain() throws {
        let places = Places()
        defer { places.remove() }
        let slot = MemorySlot(holding: [Self.cookie("session", "gurpreet")])
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record, removes: { _ in true }
        )
        #expect(!keeper.finishedAtStart)
        #expect(keeper.networkJar().header(for: Self.address) == "session=gurpreet")
        // The same store at the next launch, for as long as the session lasts.
        let store = try #require(keeper.storeIdentifier())
        #expect(keeper.storeIdentifier() == store)
        #expect(places.record.stores().current == store)
    }

    @Test func signingOutRetiresTheWebViewsStoreAndTheNextSessionGetsANewOne() async throws {
        let places = Places()
        defer { places.remove() }
        let slot = MemorySlot()
        let removed = RemovedStores()
        removed.succeeds = false
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record,
            removes: { id in
                removed.ids.append(id)
                return removed.succeeds
            }
        )
        keeper.networkJar().merge([Self.cookie("session", "gurpreet")])
        #expect(slot.read() != nil)
        let hers = try #require(keeper.storeIdentifier())

        keeper.end()

        #expect(slot.read() == nil)
        #expect(!places.record.owesWipe, "all of it finished, so the mark is gone")
        let next = try #require(keeper.storeIdentifier())
        #expect(next != hers)
        // A web view still holds her store, so it cannot be removed yet: it stays written down.
        #expect(places.record.stores().retired == [hers])
        // The next start removes it.
        removed.succeeds = true
        let later = SessionKeeper.open(
            origin: "https://shop.example", slot: slot, cache: places.cache, snapshots: places.snapshots,
            files: places.files, record: places.record,
            removes: { id in
                removed.ids.append(id)
                return removed.succeeds
            }
        )
        await later.removal?.value
        #expect(removed.ids.contains(hers))
        #expect(places.record.stores() == WebStoreList(current: next))
        keeper.removal?.cancel()
    }

    // MARK: - What is kept of each window

    @Test func aStartKeepsOnlyTheFilesTheStoresThemselvesWrite() throws {
        let places = Places()
        defer { places.remove() }
        places.cache.write("boot", "{}")
        let inside = KeptFiles(folder: places.cache.folder.appendingPathComponent("scene-9C1F", isDirectory: true))
        inside.write("page:https://shop.example/orders", "Simran's orders")
        let beside = KeptFiles(folder: places.snapshots.folder.appendingPathComponent("scene-9C1F", isDirectory: true))
        beside.write("snapshot", "what Simran typed")
        _ = SessionKeeper.open(
            origin: "https://shop.example", cache: places.cache, snapshots: places.snapshots, files: places.files
        )
        #expect(!FileManager.default.fileExists(atPath: inside.folder.path))
        #expect(!FileManager.default.fileExists(atPath: beside.folder.path))
        #expect(places.cache.read("boot") == "{}")
    }

    @Test func aLaunchThatBringsNoWindowBackKeepsNothingOfTheWindowsBefore() throws {
        let places = Places()
        defer { places.remove() }
        places.snapshots.write("snapshot-first", "what Simran typed last week")
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", cache: places.cache, snapshots: places.snapshots, files: places.files
        )
        // A window that is not one of the app's main ones decides nothing.
        keeper.windowBegins(restoring: nil)
        #expect(places.snapshots.read("snapshot-first") != nil)
        keeper.windowBegins(restoring: false)
        #expect(places.snapshots.read("snapshot-first") == nil)
    }

    @Test func aWindowTheSystemBringsBackFindsItsPlaceUnlessNothingWroteItForLong() throws {
        let places = Places()
        defer { places.remove() }
        places.snapshots.write("snapshot-first", "this window")
        places.snapshots.write("snapshot-closed", "a window closed long ago")
        let now = Date()
        try FileManager.default.setAttributes(
            [.modificationDate: now.addingTimeInterval(-SessionKeeper.unusedLimit - 60)],
            ofItemAtPath: places.snapshots.file("snapshot-closed").path
        )
        let keeper = SessionKeeper.open(
            origin: "https://shop.example", cache: places.cache, snapshots: places.snapshots, files: places.files
        )
        keeper.windowBegins(restoring: true, now: now)
        #expect(places.snapshots.read("snapshot-first") == "this window")
        #expect(places.snapshots.read("snapshot-closed") == nil)
        // A second window starting plainly later takes nothing away.
        keeper.windowBegins(restoring: false, now: now)
        #expect(places.snapshots.read("snapshot-first") == "this window")
    }
}
