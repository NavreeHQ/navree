import Foundation
import NavreeCoreBridge
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

/// The web view stores a test saw removed.
@MainActor
final class RemovedStores {
    var ids: [UUID] = []
    /// What a removal answers: whether the store is gone.
    var succeeds = true
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
