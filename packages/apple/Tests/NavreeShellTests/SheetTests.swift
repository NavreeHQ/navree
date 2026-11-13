import AuthenticationServices
import Foundation
import NavreeCoreBridge
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

/// Sign in with Apple, a confirmation with a passkey, a passkey made or used,
/// and the prompt to allow notifications: each request built from what the
/// core handed over, each answer sent back as the core's event, a person who
/// closes the sheet and a sheet that fails, and the core's own conversation
/// replayed with the client's handler in the loop.
@MainActor
struct SheetTests {
    static let signInWithApple = DeviceRequest.call(.platformSignIn(.init(
        nonce: "/api/auth/social/apple/native/nonce", provider: .apple, signIn: "/api/auth/social/apple/native/session",
        web: "/api/auth/social/apple/redirect"
    )))
    static let confirm = DeviceRequest.call(.confirm(.init(
        challenge: "/api/auth/confirm/options", detail: "₹1,499.00 to Tejinder Randhawa", reason: "Approve refund"
    )))

    static func options(_ json: String) -> Navree.JSONValue? {
        try? JSONDecoder().decode(Navree.JSONValue.self, from: Data(json.utf8))
    }

    /// The core's answer for a confirmation (`made/confirm-options.json`).
    static let confirmOptions = options(
        #"{"publicKey":{"challenge":"mC1uY2hhbGxlbmdl","rpId":"shop.example","timeout":120000,"userVerification":"required","allowCredentials":[{"type":"public-key","id":"cGstMQ"}]}}"#
    )

    /// What the server's passkey routes answer for a registration.
    static let registrationOptions = options(
        #"{"publicKey":{"challenge":"cmVnLWNoYWxsZW5nZQ","rp":{"id":"shop.example","name":"Charkha Supply Co."},"user":{"id":"dXNlci03","name":"ravneet.kaur@example.com","displayName":"Ravneet Kaur"},"pubKeyCredParams":[{"type":"public-key","alg":-8},{"type":"public-key","alg":-7}],"timeout":300000,"attestation":"none","authenticatorSelection":{"residentKey":"preferred","userVerification":"preferred"},"excludeCredentials":[{"type":"public-key","id":"cGstMQ"}]}}"#
    )

    /// Runs one call with a played sheet; what the core would hear.
    private func run(
        _ device: DeviceRequest, options: Navree.JSONValue?, signedWith: Set<Entitlement>, _ outcomes: [SheetOutcome]
    ) -> (asked: [SheetRequest], answers: [CoreEvent.DeviceOutcome]) {
        let calls = DeviceCalls(signedWith: signedWith, origin: URL(string: "https://shop.example"))
        let sheet = PlayedSheet(outcomes)
        calls.sheet = sheet
        var answers: [CoreEvent.DeviceOutcome] = []
        calls.answer = { _, outcome in answers.append(outcome) }
        calls.run(call: 1, device: device, posts: true, options: options)
        return (sheet.asked, answers)
    }

    // MARK: - Sign in with Apple

    @Test func signInWithAppleAsksForTheNameAndEmailWithTheHashOfTheCoresNonce() throws {
        let (asked, _) = run(
            Self.signInWithApple, options: Self.options(#"{"nonce":"n-7f2c91"}"#), signedWith: [.signInWithApple], []
        )
        let hashed = SheetRequest.hashed("n-7f2c91")
        #expect(asked == [.appleID(nonce: hashed)])
        // SHA-256, lower-case hex: the test vector of FIPS 180-2.
        #expect(SheetRequest.hashed("abc") == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        let request = try #require(SheetRequest.appleID(nonce: hashed).system as? ASAuthorizationAppleIDRequest)
        #expect(request.nonce == hashed)
        #expect(Set(request.requestedScopes ?? []) == [.fullName, .email])
    }

    @Test func applesAnswerGoesBackAsTheTokenTheCodeAndTheName() {
        let answer = SheetAnswer.appleID(
            identityToken: Data("eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIwMDEifQ.c2ln".utf8),
            authorizationCode: Data("c1a2b3".utf8), givenName: "Ravneet", familyName: "Kaur"
        )
        let (_, answers) = run(
            Self.signInWithApple, options: Self.options(#"{"nonce":"n-7f2c91"}"#), signedWith: [.signInWithApple],
            [.answered(answer)]
        )
        #expect(answers == [.done([
            Navree.Pair(name: "id_token", value: "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIwMDEifQ.c2ln"),
            Navree.Pair(name: "authorization_code", value: "c1a2b3"),
            Navree.Pair(name: "first_name", value: "Ravneet"),
            Navree.Pair(name: "last_name", value: "Kaur"),
        ])])
        // A later sign-in: Apple gives no name. No token: nothing the server can read.
        #expect(
            SheetAnswer.appleID(identityToken: Data("t".utf8), authorizationCode: nil, givenName: nil, familyName: nil)
                .fields == [Navree.Pair(name: "id_token", value: "t")]
        )
        #expect(SheetAnswer.appleID(identityToken: nil, authorizationCode: nil, givenName: "R", familyName: nil).fields == nil)
    }

    @Test func aSheetClosedFailedOrNeverShownIsARefusal() {
        let nonce = Self.options(#"{"nonce":"n-7f2c91"}"#)
        #expect(run(Self.signInWithApple, options: nonce, signedWith: [.signInWithApple], [.refused(.cancelled)]).answers
            == [.refused(.cancelled)])
        // Apple gave a credential with no token.
        let empty = SheetAnswer.appleID(identityToken: nil, authorizationCode: nil, givenName: nil, familyName: nil)
        #expect(run(Self.signInWithApple, options: nonce, signedWith: [.signInWithApple], [.answered(empty)]).answers
            == [.refused(.failed)])
        // No nonce handed over: no sheet.
        let none = run(Self.signInWithApple, options: nil, signedWith: [.signInWithApple], [])
        #expect(none.asked.isEmpty && none.answers == [.refused(.failed)])
        // Another provider: Apple's sheet is for Apple only.
        let google = DeviceRequest.call(.platformSignIn(.init(nonce: "/n", provider: .google, signIn: "/s")))
        #expect(run(google, options: nonce, signedWith: [.signInWithApple], []).answers == [.refused(.unsupported)])
        // Not signed for it.
        #expect(run(Self.signInWithApple, options: nonce, signedWith: [], []).answers == [.refused(.notBuilt)])
    }

    @Test func theSystemsErrorsAreTheProtocolsRefusals() {
        #expect(SheetOutcome.refusal(ASAuthorizationError(.canceled)) == .cancelled)
        #expect(SheetOutcome.refusal(ASAuthorizationError(.notHandled)) == .unavailable)
        #expect(SheetOutcome.refusal(ASAuthorizationError(.notInteractive)) == .unavailable)
        #expect(SheetOutcome.refusal(ASAuthorizationError(.unknown)) == .unavailable)
        #expect(SheetOutcome.refusal(ASAuthorizationError(.failed)) == .failed)
        #expect(SheetOutcome.refusal(ASAuthorizationError(.invalidResponse)) == .failed)
        #expect(SheetOutcome.refusal(URLError(.notConnectedToInternet)) == .failed)
    }

    @Test func oneSheetAtATime() {
        let calls = DeviceCalls(signedWith: [.signInWithApple], origin: nil)
        let sheet = PlayedSheet()
        sheet.holds = true
        calls.sheet = sheet
        var answers: [(Int, CoreEvent.DeviceOutcome)] = []
        calls.answer = { answers.append(($0, $1)) }
        let nonce = Self.options(#"{"nonce":"n-7f2c91"}"#)
        calls.run(call: 1, device: Self.signInWithApple, posts: true, options: nonce)
        calls.run(call: 2, device: Self.signInWithApple, posts: true, options: nonce)
        #expect(answers.map(\.0) == [2] && answers.first?.1 == .refused(.busy))
        sheet.finish(.refused(.cancelled))
        #expect(answers.map(\.0) == [2, 1])
        calls.run(call: 3, device: Self.signInWithApple, posts: true, options: nonce)
        #expect(sheet.asked.count == 2)
    }

    /// The core's conversation of a provider's sign-in, with Apple's sheet played.
    @Test func thePlatformSignInConversationReplaysWithTheClientsHandler() throws {
        let token = Data("eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIwMDEifQ.c2ln".utf8)
        let sheet = PlayedSheet([
            .answered(.appleID(identityToken: token, authorizationCode: Data("c1a2b3".utf8), givenName: "Ravneet", familyName: "Kaur")),
            .answered(.appleID(identityToken: token, authorizationCode: nil, givenName: nil, familyName: nil)),
            .refused(.cancelled),
        ])
        let replay = try Replay("platform-sign-in", signedWith: [.signInWithApple], sheet: sheet)
        replay.play()
        #expect(replay.problems.isEmpty, "\(replay.problems)")
        #expect(sheet.asked == Array(repeating: .appleID(nonce: SheetRequest.hashed("n-7f2c91")), count: 3))
        #expect(replay.fetched.map(Fetch.init) == replay.transcript.fetches)
        // Two answers posted to the sign-in route; the closed sheet posted nothing.
        #expect(replay.fetched.filter { $0.url.hasSuffix("/apple/native/session") }.count == 2)
    }

    // MARK: - A confirmation with a passkey

    @Test func aConfirmationSignsTheServersChallengeWithThePersonVerified() throws {
        let (asked, _) = run(Self.confirm, options: Self.confirmOptions, signedWith: [.associatedDomains], [])
        let challenge = try #require(Base64URL.decode("mC1uY2hhbGxlbmdl"))
        let expected = PasskeyAssertion(
            relyingParty: "shop.example", challenge: challenge, allowed: [Data("pk-1".utf8)], verification: .required
        )
        #expect(asked == [.assertion(expected)])
        let request = try #require(SheetRequest.assertion(expected).system as? ASAuthorizationPlatformPublicKeyCredentialAssertionRequest)
        #expect(request.relyingPartyIdentifier == "shop.example")
        #expect(request.challenge == challenge)
        #expect(request.userVerificationPreference == .required)
        #expect(request.allowedCredentials.map(\.credentialID) == [Data("pk-1".utf8)])
        // Whatever the options say, the person is verified.
        let lax = Self.options(#"{"publicKey":{"challenge":"bS1jaGFsbGVuZ2U","rpId":"shop.example","userVerification":"discouraged"}}"#)
        guard case .sheet(.assertion(let read)) = SheetRequest.confirm(.init(challenge: "/c", reason: "r"), options: lax) else {
            Issue.record("no sheet")
            return
        }
        #expect(read.verification == .required && read.allowed.isEmpty)
    }

    @Test func aConfirmationGoesBackAsTheAssertionWebAuthnWrites() throws {
        let answer = SheetAnswer.assertion(
            credentialID: Data("pk-1".utf8), clientDataJSON: Data("{}".utf8), authenticatorData: Data([1]),
            signature: Data([0x30, 0x45, 0x02]), userID: Data("user-7".utf8)
        )
        let (_, answers) = run(Self.confirm, options: Self.confirmOptions, signedWith: [.associatedDomains], [.answered(answer)])
        guard case .done(let fields)? = answers.first, fields.count == 1, fields[0].name == "credential" else {
            Issue.record("\(answers)")
            return
        }
        let credential = try #require(try JSONSerialization.jsonObject(with: Data(fields[0].value.utf8)) as? [String: Any])
        #expect(credential["id"] as? String == "cGstMQ")
        #expect(credential["rawId"] as? String == "cGstMQ")
        #expect(credential["type"] as? String == "public-key")
        #expect(credential["authenticatorAttachment"] as? String == "platform")
        let response = try #require(credential["response"] as? [String: String])
        #expect(response == [
            "clientDataJSON": "e30", "authenticatorData": "AQ", "signature": "MEUC", "userHandle": "dXNlci03",
        ])
    }

    @Test func aConfirmationThatCannotBeSignedHereIsRefused() {
        // Closed by the person.
        #expect(run(Self.confirm, options: Self.confirmOptions, signedWith: [.associatedDomains], [.refused(.cancelled)])
            .answers == [.refused(.cancelled)])
        // No challenge handed over, or an unreadable one: no sheet.
        for options in [nil, Self.options(#"{"publicKey":{"rpId":"shop.example"}}"#), Self.options(#"{"challenge":"x"}"#)] {
            let (asked, answers) = run(Self.confirm, options: options, signedWith: [.associatedDomains], [])
            #expect(asked.isEmpty && answers == [.refused(.failed)])
        }
        // A key of this device's own is not built on Apple's systems.
        let deviceKey = DeviceRequest.call(.confirm(.init(challenge: "/c", method: .deviceKey, reason: "Approve")))
        #expect(run(deviceKey, options: Self.confirmOptions, signedWith: [.associatedDomains], []).answers == [.refused(.unsupported)])
        // Not signed with the site's domain.
        #expect(run(Self.confirm, options: Self.confirmOptions, signedWith: [], []).answers == [.refused(.notBuilt)])
    }

    /// The core's conversation of device calls, with the passkey's sheet
    /// played for its confirmation: the core posts the assertion this client
    /// wrote, as a form, where the call said.
    @Test func theConfirmationInTheDeviceCallConversationReplaysWithTheClientsHandler() throws {
        let answer = SheetAnswer.assertion(
            credentialID: Data("pk-1".utf8), clientDataJSON: Data("{}".utf8), authenticatorData: Data([1]),
            signature: Data([0x30, 0x45, 0x02]), userID: nil
        )
        let sheet = PlayedSheet([.answered(answer)])
        let replay = try Replay("device-call", signedWith: [.associatedDomains], sheet: sheet)
        replay.play()
        #expect(replay.problems.isEmpty, "\(replay.problems)")
        #expect(sheet.asked.count == 1)
        let approved = "https://shop.example/orders/1/refund/approved"
        let got = replay.fetched.map(Fetch.init)
        let expected = replay.transcript.fetches
        #expect(got.map(\.url) == expected.map(\.url))
        #expect(got.filter { $0.url != approved } == expected.filter { $0.url != approved })
        let posted = formPairs(got.first { $0.url == approved }?.body)
        #expect(posted["outcome"] == "ok")
        #expect(posted["credential"] == answer.fields?.first?.value)
        #expect(posted["_call"] == nil)
    }

    // MARK: - Passkeys

    @Test func aPasskeyIsMadeFromTheServersCreationOptions() throws {
        let register = DeviceRequest.call(.passkey(.init(mode: .register, options: "/api/auth/passkeys/register/options")))
        let (asked, _) = run(register, options: Self.registrationOptions, signedWith: [.associatedDomains], [])
        let expected = PasskeyRegistration(
            relyingParty: "shop.example", challenge: Data("reg-challenge".utf8), userID: Data("user-7".utf8),
            name: "ravneet.kaur@example.com", displayName: "Ravneet Kaur", verification: .preferred, attestation: "none",
            excluded: [Data("pk-1".utf8)]
        )
        #expect(asked == [.registration(expected)])
        let request = try #require(
            SheetRequest.registration(expected).system as? ASAuthorizationPlatformPublicKeyCredentialRegistrationRequest
        )
        #expect(request.relyingPartyIdentifier == "shop.example")
        #expect(request.challenge == Data("reg-challenge".utf8))
        #expect(request.userID == Data("user-7".utf8))
        #expect(request.name == "ravneet.kaur@example.com")
        #expect(request.displayName == "Ravneet Kaur")
        #expect(request.userVerificationPreference == .preferred)
        #expect(request.attestationPreference == .none)
        #expect(request.excludedCredentials?.map(\.credentialID) == [Data("pk-1".utf8)])
    }

    @Test func aNewPasskeyGoesBackAsTheRegistrationWebAuthnWrites() throws {
        let register = DeviceRequest.call(.passkey(.init(mode: .register, options: "/api/auth/passkeys/register/options")))
        let answer = SheetAnswer.registration(
            credentialID: Data("pk-2".utf8), clientDataJSON: Data("{}".utf8), attestationObject: Data([0xA3, 0x63])
        )
        let (_, answers) = run(register, options: Self.registrationOptions, signedWith: [.associatedDomains], [.answered(answer)])
        guard case .done(let fields)? = answers.first, fields.map(\.name) == ["credential"] else {
            Issue.record("\(answers)")
            return
        }
        let credential = try #require(try JSONSerialization.jsonObject(with: Data(fields[0].value.utf8)) as? [String: Any])
        #expect(credential["id"] as? String == "cGstMg")
        #expect(credential["type"] as? String == "public-key")
        let response = try #require(credential["response"] as? [String: Any])
        #expect(response["clientDataJSON"] as? String == "e30")
        #expect(response["attestationObject"] as? String == "o2M")
        #expect(response["transports"] as? [String] == ["hybrid", "internal"])
        // No attestation object: nothing the server can register.
        let bare = SheetAnswer.registration(credentialID: Data("pk-2".utf8), clientDataJSON: Data(), attestationObject: nil)
        #expect(run(register, options: Self.registrationOptions, signedWith: [.associatedDomains], [.answered(bare)]).answers
            == [.refused(.failed)])
    }

    @Test func aPasskeySignsInWithTheServersRequestOptions() {
        let signIn = DeviceRequest.call(.passkey(.init(mode: .signIn, options: "/api/auth/passkeys/login/options")))
        let options = Self.options(#"{"publicKey":{"challenge":"bG9naW4","rpId":"shop.example","userVerification":"preferred","allowCredentials":[]}}"#)
        let (asked, answers) = run(signIn, options: options, signedWith: [.associatedDomains], [.refused(.cancelled)])
        #expect(asked == [.assertion(.init(relyingParty: "shop.example", challenge: Data("login".utf8), allowed: [], verification: .preferred))])
        #expect(answers == [.refused(.cancelled)])
        // The options alone, without `publicKey` around them, read the same.
        let bare = Self.options(#"{"challenge":"bG9naW4","rpId":"shop.example"}"#)
        #expect(run(signIn, options: bare, signedWith: [.associatedDomains], []).asked.count == 1)
    }

    /// The core never fetches a passkey's options: the call arrives with
    /// none, and no request is made here to get them.
    @Test func aPasskeyCallWithoutItsOptionsIsRefusedWithoutASheet() {
        let register = DeviceRequest.call(.passkey(.init(mode: .register, options: "/api/auth/passkeys/register/options")))
        let (asked, answers) = run(register, options: nil, signedWith: [.associatedDomains], [])
        #expect(asked.isEmpty && answers == [.refused(.failed)])
        #expect(run(register, options: Self.registrationOptions, signedWith: [], []).answers == [.refused(.notBuilt)])
        // And no build offers it while that is so.
        #expect(!CoreConfiguration.caps(signedWith: Set(Entitlement.allCases), signInScheme: "charkha").contains("passkey"))
    }

    // MARK: - Notifications

    @Test func thePromptToAllowNotificationsAnswersWhatThePersonSaid() {
        for granted in [true, false] {
            let calls = DeviceCalls(signedWith: [.pushNotifications], origin: nil)
            let permission = PlayedPermission(granted: granted)
            calls.permission = permission
            var tokenAsked = false
            calls.allowed = { tokenAsked = true }
            var answers: [CoreEvent.DeviceOutcome] = []
            calls.answer = { _, outcome in answers.append(outcome) }
            calls.run(call: 1, device: .call(.registerPush(.init())), posts: true, options: nil)
            #expect(permission.asked == 1)
            #expect(answers == [.done([Navree.Pair(name: "granted", value: granted ? "true" : "false")])])
            #expect(tokenAsked == granted)
        }
        // A build not signed for push never asks.
        let calls = DeviceCalls(signedWith: [], origin: nil)
        let permission = PlayedPermission(granted: true)
        calls.permission = permission
        var answers: [CoreEvent.DeviceOutcome] = []
        calls.answer = { _, outcome in answers.append(outcome) }
        calls.run(call: 1, device: .call(.registerPush(.init())), posts: true, options: nil)
        #expect(permission.asked == 0 && answers == [.refused(.notBuilt)])
    }

    @Test func thePushConversationReplaysWithTheClientsPrompt() throws {
        let permission = PlayedPermission(granted: true)
        let replay = try Replay("push", signedWith: [.pushNotifications], permission: permission)
        replay.play()
        #expect(replay.problems.isEmpty, "\(replay.problems)")
        #expect(permission.asked == 1)
        #expect(replay.fetched.map(Fetch.init) == replay.transcript.fetches)
        #expect(formPairs(replay.fetched.last { $0.url.hasSuffix("/account/notifications") }.map(Fetch.init)?.body)
            == ["granted": "true", "_call": "tk_p19", "outcome": "ok"])
    }

    @Test func theTokenAndATappedNotificationAreReadAsTheServerWritesThem() throws {
        #expect(AppEvents.hex(Data([0x0A, 0x1B, 0xFF, 0x00])) == "0a1bff00")
        #expect(AppEvents.href(["aps": ["alert": ["title": "Order CS-20481 shipped"]], "href": "/orders/1"]) == "/orders/1")
        #expect(AppEvents.href(["aps": ["content-available": 1], "refresh": "inbox"]) == nil)
        // A debug build's token is the sandbox's: the core is told so.
        let event = try #require(
            try JSONSerialization.jsonObject(with: CoreEvent.pushToken(token: "0a1bff00", sandbox: true).json) as? [String: String]
        )
        #expect(event == ["t": "push_token", "token": "0a1bff00", "environment": "sandbox"])
    }
}
