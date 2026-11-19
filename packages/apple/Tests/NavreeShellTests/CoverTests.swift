import Foundation
import NavreeCoreBridge
import NavreeProtocol
import NavreeRuntime
import SwiftUI
import Testing

@testable import NavreeShell

@Test func theContentIsCoveredWhileTheSceneIsNotActiveOnlyWhenTheAppAsks() {
    let plain = ShellConfiguration(origin: "https://shop.example", appVersion: "1")
    #expect(!plain.coversWhenInactive)
    for phase in [ScenePhase.active, .inactive, .background] {
        #expect(!CoverRules.covers(plain, phase: phase))
    }
    let covered = ShellConfiguration(origin: "https://shop.example", appVersion: "1", coversWhenInactive: true)
    #expect(!CoverRules.covers(covered, phase: .active))
    #if os(iOS)
    #expect(CoverRules.covers(covered, phase: .inactive))
    #expect(CoverRules.covers(covered, phase: .background))
    #else
    #expect(!CoverRules.covers(covered, phase: .inactive))
    #endif
}

/// While a locked sign-in cover or the blank cover waiting for the shell stands
/// over the window, every web page underneath is taken down and makes no
/// request; when the shell names the same person again, the web page loads anew
/// over the same session's store, and when another person signs in, that store
/// is retired first.
@MainActor
@Test func webPagesUnderTheSignInAreTakenDownAndComeBackForTheSamePerson() throws {
    let places = SessionEndTests.Places()
    defer { places.remove() }
    let removed = RemovedStores()
    let keeper = SessionKeeper.open(
        origin: "https://shop.example", slot: MemorySlot(), cache: places.cache, snapshots: places.snapshots,
        files: places.files, record: places.record,
        removes: { id in
            removed.ids.append(id)
            return true
        }
    )
    let configuration = ShellConfiguration(origin: "https://shop.example", appID: "com.navree.demo", appVersion: "1.0")
    let core = try #require(Core(config: CoreConfiguration.json(configuration, device: DeviceTraits())))
    let transport = FakeTransport()
    transport.jar = keeper.networkJar()
    transport.headers = [["x-navree-person", "0193a4b2-0000-7000-8000-00000000ra01"]]
    transport.answers = [
        "/_navree/native/shell": (200, "made/shell-routes.json"),
        "/": (200, "store/dashboard.json"),
        "/orders": (200, "store/order.json"),
        "/sign-in?next=%2Forders%2F1": (200, "made/sign-in.json"),
    ]
    let effects = SystemEffects(transport: transport, keeper: keeper, origin: configuration.base, site: configuration.site)
    let client = NativeClient(core: core, effects: effects)
    var problems: [String] = []
    client.problem = { problems.append($0) }

    client.begin()
    #expect(client.app?.user?.name == "Ravneet Kaur")
    let firstStore = try #require(keeper.storeIdentifier())
    guard case .web(let firstURL, _, _, let firstSerial)? = client.screen(1)?.content else {
        Issue.record("screen 1 did not load in the web view: \(String(describing: client.screen(1)?.content))")
        return
    }
    #expect(firstURL == "https://shop.example/")
    #expect(!ModalRules.isUnderLockedCover(1, in: client.nav))

    // Her session ends while the web page stands on destination 0.
    transport.answers["/"] = (401, "made/session-ended.json")
    transport.headers = []
    client.send(.refresh(screen: 1))

    #expect(client.app?.user == nil)
    #expect(ModalRules.lockedIndex(in: client.nav.modals) == 0)
    #expect(ModalRules.isUnderLockedCover(1, in: client.nav))
    if case .blank? = client.screen(1)?.content {} else {
        Issue.record("the web screen was not taken down when the sign-in cover went up: \(String(describing: client.screen(1)?.content))")
    }
    let cover = try #require(client.nav.modals.first?.stack.first)
    #expect(!ModalRules.isUnderLockedCover(cover, in: client.nav))

    // She submits the sign-in form; hold the shell answer back to inspect the waiting cover.
    transport.answers.removeValue(forKey: "/_navree/native/shell")
    transport.answers["/"] = (200, "store/dashboard.json")
    transport.redirects["/sign-in"] = (303, "/")
    let beforeSignInCount = transport.fetched.count
    client.send(.field(screen: cover, form: "sign-in", field: "sign-in.email", value: .text("ravneet.kaur@example.com")))
    client.send(.field(screen: cover, form: "sign-in", field: "sign-in.password", value: .text("right")))
    client.send(.act(.init(screen: cover, action: .submit(.init()), form: "sign-in")))

    // Only the sign-in post and the shell check went out: nothing asked for `/` under the cover.
    let whileCovered = transport.fetched.dropFirst(beforeSignInCount).map(\.url)
    #expect(whileCovered == ["https://shop.example/sign-in", "https://shop.example/_navree/native/shell"])
    #expect(client.app?.user == nil)
    #expect(ModalRules.lockedIndex(in: client.nav.modals) == 0)
    #expect(ModalRules.isUnderLockedCover(1, in: client.nav))
    if case .blank? = client.screen(1)?.content {} else {
        Issue.record("the web screen did not stay empty while waiting for the shell")
    }

    // The shell names the same person: the cover goes, her store is still hers, and `/` loads again.
    let shellRequest = try #require(transport.fetched.last?.request)
    transport.headers = [["x-navree-person", "0193a4b2-0000-7000-8000-00000000ra01"]]
    transport.answers["/_navree/native/shell"] = (200, "made/shell-routes.json")
    client.send(.response(Response(
        request: shellRequest, status: 200, contentType: "application/vnd.navree.native+json",
        headers: transport.headers, body: Fixtures.text("made/shell-routes.json")
    )))

    #expect(client.nav.modals.isEmpty)
    #expect(ModalRules.lockedIndex(in: client.nav.modals) == nil)
    #expect(!ModalRules.isUnderLockedCover(1, in: client.nav))
    #expect(keeper.number == 0)
    #expect(keeper.storeIdentifier() == firstStore)
    #expect(removed.ids.isEmpty)
    guard case .web(let reloadedURL, _, _, let reloadedSerial)? = client.screen(1)?.content else {
        Issue.record("the web screen did not come back for the same person: \(String(describing: client.screen(1)?.content))")
        return
    }
    #expect(reloadedURL == "https://shop.example/")
    #expect(reloadedSerial > firstSerial)

    // When the session ends again and another person signs in, the first person's store is retired first.
    transport.answers["/"] = (401, "made/session-ended.json")
    transport.headers = []
    client.send(.refresh(screen: 1))
    let secondCover = try #require(client.nav.modals.first?.stack.first)
    transport.answers.removeValue(forKey: "/_navree/native/shell")
    transport.answers["/"] = (200, "store/dashboard.json")
    client.send(.field(screen: secondCover, form: "sign-in", field: "sign-in.email", value: .text("harpreet.singh@example.com")))
    client.send(.field(screen: secondCover, form: "sign-in", field: "sign-in.password", value: .text("right")))
    client.send(.act(.init(screen: secondCover, action: .submit(.init()), form: "sign-in")))
    let secondShellRequest = try #require(transport.fetched.last?.request)
    transport.headers = [["x-navree-person", "0193a4b2-0000-7000-8000-00000000hs02"]]
    transport.answers["/_navree/native/shell"] = (200, "made/shell-other.json")
    client.send(.response(Response(
        request: secondShellRequest, status: 200, contentType: "application/vnd.navree.native+json",
        headers: transport.headers, body: Fixtures.text("made/shell-other.json")
    )))

    #expect(client.app?.user?.name == "Harpreet Singh")
    #expect(removed.ids == [firstStore])
    #expect(keeper.storeIdentifier() != firstStore)
    #expect(problems.isEmpty, "\(problems)")
}
