import Foundation
import NavreeRuntime
import Testing
import WebKit

@testable import NavreeShell

@MainActor
struct SessionEndTests {
    static let address = URL(string: "https://shop.example/orders")!

    static func answer(setting cookie: String) -> HTTPURLResponse {
        HTTPURLResponse(url: address, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: ["Set-Cookie": cookie])!
    }

    @Test func aCookieThatAnAnswerOfAnEndedSessionSetsIsDropped() async {
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        jar.merge([CookieRecord(name: "session", value: "a", domain: "shop.example")])
        let asked = jar.session
        await jar.forget(nil).value
        #expect(jar.header(for: Self.address) == nil)
        #expect(jar.session == asked + 1)
        // An answer to a request sent before the session ended.
        jar.take(from: Self.answer(setting: "session=b; Path=/; Secure"), for: Self.address, session: asked)
        #expect(jar.header(for: Self.address) == nil)
        // One sent after.
        jar.take(from: Self.answer(setting: "session=c; Path=/; Secure"), for: Self.address, session: jar.session)
        #expect(jar.header(for: Self.address) == "session=c")
    }

    @Test func theJarIsEmptiedBeforeTheNextEffectNotLater() {
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        jar.merge([CookieRecord(name: "session", value: "a", domain: "shop.example")])
        jar.forget(nil)
        #expect(jar.header(for: Self.address) == nil)
    }

    @Test(.timeLimit(.minutes(1)))
    func theWipeFindsTheWebViewsCookiesByTheOriginsHostEvenWhenTheJarHoldsNone() async throws {
        let store = WKWebsiteDataStore.nonPersistent()
        let ours = try #require(HTTPCookie(properties: [.name: "session", .value: "a", .domain: "shop.example", .path: "/"]))
        let wide = try #require(HTTPCookie(properties: [.name: "wide", .value: "b", .domain: ".shop.example", .path: "/"]))
        let other = try #require(HTTPCookie(properties: [.name: "other", .value: "c", .domain: "other.example", .path: "/"]))
        for cookie in [ours, wide, other] { await store.httpCookieStore.setCookie(cookie) }
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        #expect(jar.hosts.isEmpty)
        await jar.forget(store).value
        let left = await store.httpCookieStore.allCookies().map(\.name)
        #expect(left == ["other"])
    }

    @Test(.timeLimit(.minutes(1)))
    func waitingForTheWipeEndsWhenEitherItOrItsLimitDoes() async {
        let never = Task { _ = try? await Task.sleep(for: .seconds(3600)) }
        let soon = Task {}
        await CookieJar.first(never, soon)
        #expect(!never.isCancelled)
        never.cancel()
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

    @Test func twoScenesNeverShareWhatTheirCoresKeep() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-scenes-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: folder) }
        func effects() -> SystemEffects {
            SystemEffects(
                transport: FakeTransport(), cache: KeptFiles(folder: folder.appendingPathComponent("cache")),
                snapshots: KeptFiles(folder: folder.appendingPathComponent("snapshot"))
            )
        }
        // What a build that kept one store for every scene left goes.
        KeptFiles(folder: folder.appendingPathComponent("cache")).write("page:https://shop.example/orders", "old")
        let first = effects()
        let second = effects()
        first.keep(for: "A1B2-window")
        second.keep(for: "../C3")
        #expect(first.cache.folder != second.cache.folder)
        #expect(first.cache.folder.deletingLastPathComponent() == folder.appendingPathComponent("cache"))
        #expect(second.cache.folder.lastPathComponent == "scene----C3")
        #expect(KeptFiles(folder: folder.appendingPathComponent("cache")).read("page:https://shop.example/orders") == nil)
        first.cache.write("page:https://shop.example/orders", "Gurpreet's")
        first.snapshots.write("snapshot", "Gurpreet's")
        #expect(second.cache.read("page:https://shop.example/orders") == nil)
        second.cache.write("page:https://shop.example/orders", "Simran's")
        // A session that ends in one scene empties that scene's store only.
        first.cache.clear()
        first.snapshots.clear()
        #expect(first.cache.read("page:https://shop.example/orders") == nil)
        #expect(second.cache.read("page:https://shop.example/orders") == "Simran's")
    }

    @Test func signingOutEmptiesWhatWasKeptOfThePersonBeforeTheNextEffect() throws {
        let shell = try Shell()
        shell.effects.cache.write("page:https://shop.example/orders", "kept")
        shell.effects.snapshots.write("snapshot", "kept")
        shell.transport.jar?.merge([CookieRecord(name: "session", value: "a", domain: "shop.example")])
        shell.effects.run(.forgetSession, for: shell.client)
        #expect(shell.effects.cache.read("page:https://shop.example/orders") == nil)
        #expect(shell.effects.snapshots.read("snapshot") == nil)
        #expect(shell.transport.jar?.header(for: Self.address) == nil)
        // A build not signed for push has no token to give up.
        #expect(!shell.effects.pushAwaitsSignIn)
    }
}
