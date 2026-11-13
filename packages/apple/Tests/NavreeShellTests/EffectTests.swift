import Foundation
import NavreeCoreBridge
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

/// Each effect, carried out with the system's own means.
@MainActor
struct EffectTests {
    @Test func aFetchGoesToTheTransportAndItsAnswerBackToTheCore() throws {
        let shell = try Shell()
        shell.client.begin()
        #expect(shell.transport.fetched.map(\.url) == ["https://shop.example/_navree/native/shell", "https://shop.example/"])
        #expect(shell.client.nav.stage == .ready)
        shell.effects.run(.cancel(request: 9), for: shell.client)
        #expect(shell.transport.cancelled == [9])
    }

    @Test func randomBytesAreAnsweredAtOnceFromTheSecureGenerator() throws {
        let one = try #require(SystemEffects.secureRandom(48))
        let two = try #require(SystemEffects.secureRandom(48))
        #expect(one.count == 96)
        #expect(one != two)
        let hex = one.allSatisfy { $0.isHexDigit }
        #expect(hex)
        #expect(Set(one).count > 1)
    }

    @Test func theCacheIsKeptAndEmptied() throws {
        let shell = try Shell()
        shell.effects.run(.cacheWrite(key: "page:/orders", value: "{\"kept\":1}"), for: shell.client)
        #expect(shell.effects.cache.read("page:/orders") == "{\"kept\":1}")
        // Keys are opaque: each is a file named by its hash.
        let file = shell.effects.cache.file("page:/orders")
        #expect(file.lastPathComponent.count == 64)
        #expect(FileManager.default.fileExists(atPath: file.path))
        #expect(shell.effects.cache.isExcludedFromBackup)
        shell.effects.run(.cacheWrite(key: "page:/orders", value: nil), for: shell.client)
        #expect(shell.effects.cache.read("page:/orders") == nil)
        shell.effects.run(.cacheWrite(key: "boot", value: "{}"), for: shell.client)
        shell.effects.run(.cacheClear, for: shell.client)
        #expect(shell.effects.cache.read("boot") == nil)
        #expect(!FileManager.default.fileExists(atPath: shell.effects.cache.folder.path))
    }

    @Test func aSignOutForgetsTheSessionAndWhatWasKept() async throws {
        let shell = try Shell()
        let jar = try #require(shell.transport.jar)
        jar.merge([CookieRecord(name: "session", value: "s3cret", domain: "shop.example")])
        shell.effects.snapshots.write("snapshot", "{\"typed\":\"Wrap the dupattas\"}")
        shell.effects.run(.forgetSession, for: shell.client)
        #expect(shell.effects.snapshots.read("snapshot") == nil)
        try await Task.sleep(for: .milliseconds(100))
        #expect(jar.header(for: URL(string: "https://shop.example/orders")!) == nil)
    }

    @Test func aTimerFiresOnceAndACancelledOneNever() async throws {
        var answers = Shell.store
        answers["/"] = (200, "made/assign.json")
        answers["/couriers/options?q=kau"] = (200, "made/couriers-kau.json")
        let shell = try Shell(answers: answers)
        shell.client.begin()
        // The core waits for a pause in the typing: a timer, then another in its place.
        shell.client.send(.options(screen: 1, form: "assign", field: "courier", text: "ka", more: false))
        shell.client.send(.options(screen: 1, form: "assign", field: "courier", text: "kau", more: false))
        func asked() -> [String] { shell.transport.fetched.map(\.url).filter { $0.contains("/couriers/options") } }
        // The timer's own time, then however long a busy machine takes; and a while more for one that should never come.
        for _ in 0..<300 where asked().isEmpty {
            try await Task.sleep(for: .milliseconds(10))
        }
        try await Task.sleep(for: .milliseconds(300))
        let asked = asked()
        #expect(asked == ["https://shop.example/couriers/options?q=kau"], "\(asked)")
        #expect(shell.problems.list.isEmpty, "\(shell.problems.list)")
    }

    @Test func aStreamIsOpenedAndClosedAsTheCoreAsks() throws {
        let shell = try Shell()
        shell.effects.run(.streamOpen(stream: 3, url: "https://shop.example/broadcasting/stream?topics=orders", headers: []), for: shell.client)
        #expect(shell.transport.streams[3] == "https://shop.example/broadcasting/stream?topics=orders")
        shell.effects.run(.streamClose(stream: 3), for: shell.client)
        #expect(shell.transport.closed == [3])
    }

    @Test func aDeviceCallThisBuildDoesNotCarryOutIsRefusedWhenItPosts() throws {
        let calls = DeviceCalls(signedWith: [], origin: nil)
        var answers: [(Int, CoreEvent.DeviceOutcome)] = []
        calls.answer = { answers.append(($0, $1)) }
        calls.run(call: 1, device: .call(.scanCode(Navree.ScanCode())), posts: true, options: nil)
        calls.run(call: 2, device: .call(.scanCode(Navree.ScanCode())), posts: false, options: nil)
        // Without the team's entitlements, Sign in with Apple and a passkey's confirmation are not built.
        calls.run(
            call: 3,
            device: .call(.platformSignIn(.init(nonce: "/nonce", provider: .apple, signIn: "/session"))),
            posts: true, options: .object(["nonce": .string("n-7f2c91")])
        )
        calls.run(
            call: 4, device: .call(.confirm(.init(challenge: "/api/auth/confirm/options", reason: "Approve refund"))),
            posts: true, options: nil
        )
        #expect(answers.map(\.0) == [1, 3, 4])
        #expect(answers.allSatisfy { $0.1 == .refused(.notBuilt) })
    }

    @Test func applesSignInTakesTheHashOfTheCoresNonce() {
        // SHA-256, lower-case hex: the test vector of FIPS 180-2.
        #expect(DeviceCalls.hashed("abc") == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    }

    @Test func webAuthnBytesAreBase64ForAddresses() {
        let bytes = Data([0xFB, 0xFF, 0xBF, 0x00, 0x01])
        #expect(Base64URL.encode(bytes) == "-_-_AAE")
        #expect(Base64URL.decode("-_-_AAE") == bytes)
        #expect(Base64URL.decode("cGstMQ") == Data("pk-1".utf8))
    }

    @Test func aSignInSessionComesBackOnALinkOfTheOriginOrTheAppsScheme() {
        #expect(SignInSession.callback("https://shop.example/app/signed-in") != nil)
        #expect(SignInSession.callback("charkha") != nil)
        #expect(SignInSession.callback("charkha://signed-in") != nil)
        #expect(SignInSession.callback(nil) == nil)
        #expect(SignInSession.callback("") == nil)
    }

    @Test func theCoresNoticesAreWordedByTheClient() {
        #expect(Words.notice(.unsaved) == "Your latest change wasn't saved. Check it and save again.")
        #expect(Words.notice(.secondStep) == "One more step is needed to sign in. Sign in another way to finish.")
        #expect(Words.announcement(.fieldsNeedAttention, count: 2) == "2 fields need attention")
        #expect(Words.announcement(.fieldsNeedAttention, count: 1) == "1 field needs attention")
        #expect(Words.announcement(.loaded, count: 25) == "25 more rows loaded")
        #expect(Words.announcement(nil, count: nil) == nil)
    }
}
