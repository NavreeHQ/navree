import CryptoKit
import Foundation
import NavreeCoreBridge
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

/// Signing in through the system's sign-in session, handed over to the app:
/// the client opens exactly what the core says, comes back with exactly the
/// address the session returned on, and decides nothing else.
@MainActor
struct SignInTests {
    static let origin = URL(string: "https://shop.example")

    // MARK: - Where a session comes back

    @Test func theReturnIsReadFromTheCoresWords() {
        #expect(SignInReturn("https://Shop.example/app/signed-in") == .https(host: "shop.example", path: "/app/signed-in"))
        #expect(SignInReturn("https://shop.example") == .https(host: "shop.example", path: "/"))
        #expect(SignInReturn("charkha") == .scheme("charkha"))
        #expect(SignInReturn("charkha:/signed-in") == .scheme("charkha"))
        #expect(SignInReturn("charkha://signed-in") == .scheme("charkha"))
        for refused in [nil, "", "http://shop.example/app/signed-in", "https:///signed-in", ":/signed-in"] {
            #expect(SignInReturn(refused) == nil, "\(refused ?? "nil")")
        }
        #expect(SignInSession.callback("charkha:/signed-in") != nil)
    }

    @Test func aLinkOfTheSiteIsTakenOnlyByABuildSignedWithItsDomain() {
        let link = SignInReturn.https(host: "shop.example", path: "/app/signed-in")
        #expect(link.catchable(signedWith: [.associatedDomains], origin: Self.origin))
        #expect(!link.catchable(signedWith: [], origin: Self.origin))
        #expect(!link.catchable(signedWith: [.signInWithApple, .pushNotifications], origin: Self.origin))
        // A domain the build is signed with is the origin's own.
        #expect(!link.catchable(signedWith: [.associatedDomains], origin: URL(string: "https://other.example")))
        // The app's own scheme needs nothing.
        #expect(SignInReturn.scheme("charkha").catchable(signedWith: [], origin: Self.origin))
    }

    // MARK: - The session

    @Test func theSessionOpensExactlyWhereTheCoreSaysAndHandsBackTheWholeReturn() {
        let session = SignInSession(signedWith: [], origin: Self.origin)
        let opener = PlayedOpener()
        session.opener = opener
        var returned: [URL?] = []
        let start = URL(string: "https://shop.example/auth/app/sign-in?platform=apple&state=s-1")!
        session.start(start, callback: "charkha:/signed-in", ephemeral: true) { returned.append($0) }
        #expect(opener.opened == [.init(url: start, returning: .scheme("charkha"), ephemeral: true)])
        #expect(returned.isEmpty)
        opener.finish("charkha:/signed-in?code=Tq7uX0b2c9&state=s-1#top")
        #expect(returned == [URL(string: "charkha:/signed-in?code=Tq7uX0b2c9&state=s-1#top")])
    }

    @Test func aSessionClosedByThePersonOrFailedReturnsNothing() {
        let session = SignInSession(signedWith: [], origin: Self.origin)
        let opener = PlayedOpener()
        session.opener = opener
        var returned: [URL?] = []
        let start = URL(string: "https://shop.example/auth/app/sign-in")!
        session.start(start, callback: "charkha", ephemeral: false) { returned.append($0) }
        opener.finish(nil)
        #expect(returned == [nil])
        // The system would not open one.
        opener.refuses = true
        session.start(start, callback: "charkha", ephemeral: false) { returned.append($0) }
        #expect(returned == [nil, nil])
        // Nor is one opened with no return to come back on.
        opener.refuses = false
        session.start(start, callback: nil, ephemeral: false) { returned.append($0) }
        #expect(returned == [nil, nil, nil])
        #expect(opener.opened.count == 2)
    }

    @Test func aNewerSessionTakesThePlaceOfOneOpenAndTheOlderSaysNothingMore() {
        let session = SignInSession(signedWith: [], origin: Self.origin)
        let opener = PlayedOpener()
        session.opener = opener
        var returned: [String?] = []
        let start = URL(string: "https://shop.example/auth/app/sign-in")!
        session.start(start, callback: "charkha", ephemeral: false) { returned.append($0?.absoluteString) }
        let first = opener.handles[0]
        session.start(start, callback: "charkha", ephemeral: false) { returned.append($0?.absoluteString) }
        #expect(first.cancelled)
        // The first one's system session ends after it was closed: not passed on.
        first.done(URL(string: "charkha:/signed-in?code=old&state=a"))
        #expect(returned.isEmpty)
        opener.finish("charkha:/signed-in?code=new&state=b")
        #expect(returned == ["charkha:/signed-in?code=new&state=b"])
    }

    @Test func aSessionThatWouldComeBackOnALinkTheBuildCannotTakeIsNeverOpened() {
        let session = SignInSession(signedWith: [], origin: Self.origin)
        let opener = PlayedOpener()
        session.opener = opener
        var returned: [URL?] = []
        session.start(
            URL(string: "https://shop.example/auth/app/sign-in")!, callback: "https://shop.example/app/signed-in",
            ephemeral: false
        ) { returned.append($0) }
        #expect(opener.opened.isEmpty)
        #expect(returned == [nil])
    }

    // MARK: - The hand-over through the core

    /// The core's own conversation, with this client's session in the loop:
    /// each session opens exactly at the core's address and comes back on
    /// its link; each return the session hands back reaches the core, which
    /// exchanges only its own.
    @Test func theHandOverConversationReplaysWithTheClientsSession() throws {
        let replay = try Replay("hand-over", signedWith: [.associatedDomains])
        replay.play()
        #expect(replay.problems.isEmpty, "\(replay.problems)")
        let said = replay.transcript.effects("sign_in")
        #expect(replay.opener.opened.map(\.url.absoluteString) == said.map { $0["url"] as? String ?? "" })
        #expect(replay.opener.opened.allSatisfy {
            $0.returning == .https(host: "shop.example", path: "/app/signed-in") && !$0.ephemeral
        })
        #expect(replay.fetched.map(Fetch.init) == replay.transcript.fetches)
        // Six sessions opened, two returns exchanged: one signed in, one the server refused.
        #expect(said.count == 6)
        #expect(replay.fetched.filter { $0.url == "https://shop.example/api/auth/app/exchange" }.count == 2)
    }

    /// The whole hand-over with the system's own random bytes: the core opens
    /// the session with the SHA-256 of a verifier made from them, and posts
    /// that verifier with the code the session came back with.
    @Test func theCodeIsExchangedWithTheVerifierOfTheChallengeTheSessionOpenedWith() throws {
        let (client, transport, opener) = try handOver(callback: "https://shop.example/app/signed-in")
        client.begin()
        #expect(client.app?.user == nil)
        let front = try #require(client.nav.front)
        client.send(.act(.init(screen: front, action: .navigate(.init(href: "/sign-in")))))

        let opened = try #require(opener.opened.first)
        #expect(opened.returning == .https(host: "shop.example", path: "/app/signed-in"))
        let query = try #require(URLComponents(url: opened.url, resolvingAgainstBaseURL: false))
        #expect(query.host == "shop.example" && query.path == "/auth/app/sign-in")
        func value(_ name: String) -> String? { query.queryItems?.first { $0.name == name }?.value }
        #expect(value("platform") == "apple")
        #expect(value("app") == "com.charkha.supply")
        #expect(value("code_challenge_method") == "S256")
        #expect(value("redirect_uri") == "https://shop.example/app/signed-in")
        let challenge = try #require(value("code_challenge"))
        let state = try #require(value("state"))
        #expect(challenge.count == 43 && state.count >= 22)

        // Signed in on the site's pages inside the session.
        transport.answers["/_navree/native/shell"] = (200, Fixtures.text("made/shell-app.json"))
        transport.answers["/"] = (200, Fixtures.text("store/dashboard.json"))
        opener.finish("https://shop.example/app/signed-in?code=Tq7uX0b2c9&state=\(state)")

        let exchange = try #require(transport.fetched.first { $0.url == "https://shop.example/api/auth/app/exchange" })
        #expect(exchange.method == "post" && exchange.xsrf)
        guard case .json(let text)? = exchange.body else {
            Issue.record("the exchange is not JSON")
            return
        }
        let body = try #require(try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: String])
        #expect(body["platform"] == "apple")
        #expect(body["app"] == "com.charkha.supply")
        #expect(body["code"] == "Tq7uX0b2c9")
        let verifier = try #require(body["code_verifier"])
        #expect(verifier.count >= 43)
        #expect(Base64URL.encode(Data(SHA256.hash(data: Data(verifier.utf8)))) == challenge)
        // Signed in: the shell says who.
        #expect(client.app?.user?.name == "Ravneet Kaur")
    }

    @Test func theSessionComesBackOnTheAppsSchemeWhereTheServerSaysSo() throws {
        let (client, transport, opener) = try handOver(callback: "charkha:/signed-in", signedWith: [])
        client.begin()
        let front = try #require(client.nav.front)
        client.send(.act(.init(screen: front, action: .navigate(.init(href: "/sign-in")))))
        let opened = try #require(opener.opened.first)
        #expect(opened.returning == .scheme("charkha"))
        let state = try #require(URLComponents(url: opened.url, resolvingAgainstBaseURL: false)?.queryItems?
            .first { $0.name == "state" }?.value)
        opener.finish("charkha:/signed-in?code=Tq7uX0b2c9&state=\(state)")
        #expect(transport.fetched.contains { $0.url == "https://shop.example/api/auth/app/exchange" })
    }

    @Test func aSessionThePersonClosedSignsNobodyIn() throws {
        let (client, transport, opener) = try handOver(callback: "https://shop.example/app/signed-in")
        client.begin()
        let front = try #require(client.nav.front)
        client.send(.act(.init(screen: front, action: .navigate(.init(href: "/sign-in")))))
        let asked = transport.fetched.count
        opener.finish(nil)
        #expect(transport.fetched.count == asked)
        #expect(client.app?.user == nil)
        #expect(client.messages.isEmpty)
    }

    @Test func aReturnTheCoreDidNotAskForIsNotExchanged() throws {
        let (client, transport, opener) = try handOver(callback: "https://shop.example/app/signed-in")
        client.begin()
        let front = try #require(client.nav.front)
        client.send(.act(.init(screen: front, action: .navigate(.init(href: "/sign-in")))))
        opener.finish("https://shop.example/app/signed-in?code=PLANTED000&state=not-this-one")
        #expect(!transport.fetched.contains { $0.url.hasSuffix("/api/auth/app/exchange") })
        #expect(client.app?.user == nil)
    }

    @Test func aBuildWithoutTheSitesDomainOpensNoSessionThatCouldNotComeBack() throws {
        let (client, transport, opener) = try handOver(callback: "https://shop.example/app/signed-in", signedWith: [])
        client.begin()
        let front = try #require(client.nav.front)
        client.send(.act(.init(screen: front, action: .navigate(.init(href: "/sign-in")))))
        #expect(opener.opened.isEmpty)
        #expect(!transport.fetched.contains { $0.url.hasSuffix("/api/auth/app/exchange") })
    }

    /// A client over a fresh core, the system's effects, a played session, and
    /// a server whose shell names the hand-over with `callback`.
    private func handOver(callback: String, signedWith: Set<Entitlement> = [.associatedDomains]) throws
        -> (NativeClient, ScriptedTransport, PlayedOpener)
    {
        let configuration = ShellConfiguration(
            origin: "https://shop.example", appID: "com.charkha.supply", appVersion: "1.0", signInScheme: "charkha",
            signInPaths: ["/sign-in"], signedWith: signedWith
        )
        let core = try #require(Core(config: CoreConfiguration.json(configuration, device: DeviceTraits())))
        let transport = ScriptedTransport()
        let shell = Fixtures.text("made/shell-app-nobody.json")
            .replacingOccurrences(of: "\"https://shop.example/app/signed-in\"", with: "\"\(callback)\"")
        transport.answers = [
            "/_navree/native/shell": (200, shell),
            "/": (200, Fixtures.text("made/welcome.json")),
            "/api/auth/app/exchange": (200, Fixtures.text("made/exchanged.json")),
        ]
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-\(UUID().uuidString)")
        let effects = SystemEffects(
            transport: transport, cache: KeptFiles(folder: folder.appendingPathComponent("cache")),
            snapshots: KeptFiles(folder: folder.appendingPathComponent("snapshot")), signedWith: signedWith,
            origin: configuration.base
        )
        let opener = PlayedOpener()
        effects.signIn.opener = opener
        let client = NativeClient(core: core, effects: effects)
        client.problem = { Issue.record("\($0)") }
        return (client, transport, opener)
    }
}

/// What a build says it is signed with, and what that lets it offer.
struct SigningTests {
    @Test func anUnsignedBuildSaysNothingAndOffersNothingThatNeedsATeam() {
        #expect(Entitlement.declared(words: nil, domain: nil, origin: "https://shop.example").isEmpty)
        #expect(Entitlement.declared(words: "", domain: "", origin: "https://shop.example").isEmpty)
        // What the test runner's own bundle says: nothing.
        #expect(Entitlement.declared(in: .main, origin: "https://shop.example").isEmpty)
        #expect(ShellConfiguration(origin: "https://shop.example", appVersion: "1").signedWith.isEmpty)
    }

    @Test func aTeamBuildTurnsEachOnWithOneSetting() {
        let all = Entitlement.declared(
            words: "associated-domains sign-in-with-apple push", domain: "shop.example", origin: "https://shop.example"
        )
        #expect(all == Set(Entitlement.allCases))
        #expect(
            Entitlement.declared(words: "push,sign-in-with-apple", domain: nil, origin: "https://shop.example")
                == [.pushNotifications, .signInWithApple]
        )
        #expect(Entitlement.declared(words: "Push hologram", domain: nil, origin: "https://shop.example") == [.pushNotifications])
        let caps = CoreConfiguration.caps(signedWith: all, signInScheme: nil)
        #expect(Set(caps).isSuperset(of: ["auth_session", "confirm", "platform_sign_in", "push"]))
    }

    @Test func theAssociatedDomainCountsOnlyForTheOriginsOwnHostOverHTTPS() {
        func domains(_ domain: String?, _ origin: String) -> Bool {
            Entitlement.declared(words: "associated-domains", domain: domain, origin: origin).contains(.associatedDomains)
        }
        #expect(domains("shop.example", "https://shop.example"))
        #expect(domains("Shop.Example", "https://shop.example/"))
        #expect(domains("shop.example?mode=developer", "https://shop.example"))
        #expect(!domains("other.example", "https://shop.example"))
        #expect(!domains(nil, "https://shop.example"))
        #expect(!domains("127.0.0.1", "http://127.0.0.1:8095"))
    }
}
