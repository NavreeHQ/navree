import AuthenticationServices
import CryptoKit
import Foundation
import NavreeProtocol
import NavreeRuntime

/// What one of the system's sign-in sheets is asked for, read from what the
/// core handed over with a device call: plain values, so what is asked can be
/// checked without the sheet.
enum SheetRequest: Equatable, Sendable {
    /// Sign in with Apple, asking for the person's name and email, with the
    /// nonce as Apple's request takes it: the lower-case hex SHA-256 of the
    /// one the core asked the server for.
    case appleID(nonce: String)
    /// A passkey's signature over the server's challenge.
    case assertion(PasskeyAssertion)
    /// A new passkey for the person signed in.
    case registration(PasskeyRegistration)
}

/// How much the sheet asks the person to prove it is them, in WebAuthn's words.
enum Verification: String, Sendable {
    case required, preferred, discouraged
}

/// A passkey's signature over a challenge, as WebAuthn's request asks for it.
struct PasskeyAssertion: Equatable, Sendable {
    var relyingParty: String
    var challenge: Data
    /// The passkeys the server takes; none: any of the person's for the site.
    var allowed: [Data]
    var verification: Verification
}

/// A new passkey, as WebAuthn's creation options ask for it.
struct PasskeyRegistration: Equatable, Sendable {
    var relyingParty: String
    var challenge: Data
    var userID: Data
    var name: String
    var displayName: String?
    var verification: Verification
    /// `none`, `indirect`, `direct` or `enterprise`.
    var attestation: String
    /// The person's passkeys already on the server, which the system will not make again.
    var excluded: [Data]
}

/// What the sheet gave back, as plain values.
enum SheetAnswer: Equatable, Sendable {
    case appleID(identityToken: Data?, authorizationCode: Data?, givenName: String?, familyName: String?)
    case assertion(credentialID: Data, clientDataJSON: Data, authenticatorData: Data, signature: Data, userID: Data?)
    case registration(credentialID: Data, clientDataJSON: Data, attestationObject: Data?)
}

/// A sheet to show, or why the call is refused without one.
enum SheetAsk: Equatable, Sendable {
    case sheet(SheetRequest)
    case refused(Navree.Refusal)
}

/// How a sheet ended.
enum SheetOutcome: Equatable, Sendable {
    case answered(SheetAnswer)
    case refused(Navree.Refusal)
}

// MARK: - Reading what the core handed over

extension SheetRequest {
    /// Sign in with Apple, with the nonce the core asked the server for.
    static func appleID(options: Navree.JSONValue?) -> SheetAsk {
        guard let nonce = options?["nonce"]?.text, !nonce.isEmpty else { return .refused(.failed) }
        return .sheet(.appleID(nonce: hashed(nonce)))
    }

    /// The nonce as Apple's request takes it: SHA-256, lower-case hex.
    static func hashed(_ nonce: String) -> String {
        SHA256.hash(data: Data(nonce.utf8)).map { String(format: "%02x", $0) }.joined()
    }

    /// A confirmation: a passkey over the challenge the core asked the
    /// server for, with the person verified whatever the options say. A
    /// device key of its own is not built on Apple's systems yet.
    static func confirm(_ confirm: Navree.Confirm, options: Navree.JSONValue?) -> SheetAsk {
        if confirm.method == .deviceKey { return .refused(.unsupported) }
        guard case .sheet(.assertion(var assertion)) = assertion(options?["publicKey"]) else { return .refused(.failed) }
        assertion.verification = .required
        return .sheet(.assertion(assertion))
    }

    /// A passkey call, with the server's options for its ceremony when the
    /// core hands them over (`{"publicKey": {...}}`, or the options alone).
    /// Without them there is nothing to ask the system: no request is made here.
    static func passkey(_ passkey: Navree.Passkey, options: Navree.JSONValue?) -> SheetAsk {
        let publicKey = options?["publicKey"] ?? options
        switch passkey.mode {
        case .signIn: return assertion(publicKey)
        case .register: return registration(publicKey)
        }
    }

    /// `PublicKeyCredentialRequestOptions`, as JSON.
    static func assertion(_ options: Navree.JSONValue?) -> SheetAsk {
        guard let options, let rp = options["rpId"]?.text, !rp.isEmpty,
            let challenge = options["challenge"]?.text.flatMap(Base64URL.decode), !challenge.isEmpty
        else { return .refused(.failed) }
        return .sheet(.assertion(PasskeyAssertion(
            relyingParty: rp, challenge: challenge, allowed: ids(options["allowCredentials"]),
            verification: Verification(rawValue: options["userVerification"]?.text ?? "") ?? .preferred
        )))
    }

    /// `PublicKeyCredentialCreationOptions`, as JSON.
    static func registration(_ options: Navree.JSONValue?) -> SheetAsk {
        guard let options, let rp = options["rp"]?["id"]?.text, !rp.isEmpty,
            let challenge = options["challenge"]?.text.flatMap(Base64URL.decode), !challenge.isEmpty,
            let user = options["user"], let userID = user["id"]?.text.flatMap(Base64URL.decode), !userID.isEmpty,
            let name = user["name"]?.text, !name.isEmpty
        else { return .refused(.failed) }
        return .sheet(.registration(PasskeyRegistration(
            relyingParty: rp, challenge: challenge, userID: userID, name: name,
            displayName: user["displayName"]?.text,
            verification: Verification(rawValue: options["authenticatorSelection"]?["userVerification"]?.text ?? "")
                ?? .preferred,
            attestation: options["attestation"]?.text ?? "none",
            excluded: ids(options["excludeCredentials"])
        )))
    }

    /// The ids of a list of credential descriptors.
    private static func ids(_ list: Navree.JSONValue?) -> [Data] {
        guard case .array(let entries)? = list else { return [] }
        return entries.compactMap { $0["id"]?.text.flatMap(Base64URL.decode) }
    }
}

// MARK: - What goes back to the core

extension SheetAnswer {
    /// The result's fields, in the protocol's names, every value as text:
    /// `nil` when the sheet gave nothing the server can read.
    var fields: [Navree.Pair]? {
        switch self {
        case .appleID(let token, let code, let given, let family):
            guard let token = token.flatMap({ String(data: $0, encoding: .utf8) }), !token.isEmpty else { return nil }
            var fields = [Navree.Pair(name: "id_token", value: token)]
            if let code = code.flatMap({ String(data: $0, encoding: .utf8) }), !code.isEmpty {
                fields.append(Navree.Pair(name: "authorization_code", value: code))
            }
            // Apple gives the name only the first time a person signs in to the app.
            if let given, !given.isEmpty { fields.append(Navree.Pair(name: "first_name", value: given)) }
            if let family, !family.isEmpty { fields.append(Navree.Pair(name: "last_name", value: family)) }
            return fields
        case .assertion(let id, let clientData, let authenticatorData, let signature, let user):
            let json = WebAuthn.assertion(
                credentialID: id, clientDataJSON: clientData, authenticatorData: authenticatorData,
                signature: signature, userID: user
            )
            return [Navree.Pair(name: "credential", value: json)]
        case .registration(let id, let clientData, let attestation):
            guard let attestation, !attestation.isEmpty else { return nil }
            let json = WebAuthn.registration(credentialID: id, clientDataJSON: clientData, attestationObject: attestation)
            return [Navree.Pair(name: "credential", value: json)]
        }
    }
}

extension SheetOutcome {
    /// How the device call ended, as the core hears it.
    var device: CoreEvent.DeviceOutcome {
        switch self {
        case .answered(let answer): answer.fields.map(CoreEvent.DeviceOutcome.done) ?? .refused(.failed)
        case .refused(let refusal): .refused(refusal)
        }
    }

    /// The refusal an authorization's error means.
    static func refusal(_ error: any Error) -> Navree.Refusal {
        switch (error as? ASAuthorizationError)?.code {
        case .canceled?: .cancelled
        case .notHandled?, .notInteractive?, .unknown?: .unavailable
        default: .failed
        }
    }
}

/// A credential as WebAuthn's `toJSON()` writes it: what the server's passkey
/// and confirmation routes read.
enum WebAuthn {
    static func assertion(
        credentialID: Data, clientDataJSON: Data, authenticatorData: Data, signature: Data, userID: Data?
    ) -> String {
        var response: [String: Any] = [
            "clientDataJSON": Base64URL.encode(clientDataJSON),
            "authenticatorData": Base64URL.encode(authenticatorData),
            "signature": Base64URL.encode(signature),
        ]
        if let userID, !userID.isEmpty { response["userHandle"] = Base64URL.encode(userID) }
        return credential(credentialID, response)
    }

    static func registration(credentialID: Data, clientDataJSON: Data, attestationObject: Data) -> String {
        credential(credentialID, [
            "clientDataJSON": Base64URL.encode(clientDataJSON),
            "attestationObject": Base64URL.encode(attestationObject),
            // A passkey on Apple's systems lives on this device and syncs, and
            // another device nearby can use it.
            "transports": ["hybrid", "internal"],
        ])
    }

    private static func credential(_ id: Data, _ response: [String: Any]) -> String {
        let id = Base64URL.encode(id)
        let object: [String: Any] = [
            "id": id, "rawId": id, "type": "public-key", "authenticatorAttachment": "platform",
            "response": response, "clientExtensionResults": [String: Any](),
        ]
        let data = (try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }
}

/// Base64 for addresses, with no padding, as WebAuthn writes its bytes.
enum Base64URL {
    static func encode(_ data: Data) -> String {
        data.base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    static func decode(_ text: String) -> Data? {
        var base64 = text.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        base64 += String(repeating: "=", count: (4 - base64.count % 4) % 4)
        return Data(base64Encoded: base64)
    }
}

extension Navree.JSONValue {
    subscript(_ key: String) -> Navree.JSONValue? {
        if case .object(let object) = self { return object[key] }
        return nil
    }

    var text: String? {
        if case .string(let text) = self { return text }
        return nil
    }
}

// MARK: - The system's sheets

/// What shows one of the system's sign-in sheets: the system itself, or a
/// stand-in in tests.
@MainActor
protocol AuthorizationSheet: AnyObject {
    func present(_ request: SheetRequest, done: @escaping @MainActor (SheetOutcome) -> Void)
}

extension SheetRequest {
    /// The system's request for it.
    var system: ASAuthorizationRequest {
        switch self {
        case .appleID(let nonce):
            let request = ASAuthorizationAppleIDProvider().createRequest()
            request.requestedScopes = [.fullName, .email]
            request.nonce = nonce
            return request
        case .assertion(let assertion):
            let provider = ASAuthorizationPlatformPublicKeyCredentialProvider(relyingPartyIdentifier: assertion.relyingParty)
            let request = provider.createCredentialAssertionRequest(challenge: assertion.challenge)
            request.userVerificationPreference = assertion.verification.system
            request.allowedCredentials = assertion.allowed.map {
                ASAuthorizationPlatformPublicKeyCredentialDescriptor(credentialID: $0)
            }
            return request
        case .registration(let registration):
            let provider = ASAuthorizationPlatformPublicKeyCredentialProvider(
                relyingPartyIdentifier: registration.relyingParty
            )
            let request = provider.createCredentialRegistrationRequest(
                challenge: registration.challenge, name: registration.name, userID: registration.userID
            )
            request.displayName = registration.displayName
            request.userVerificationPreference = registration.verification.system
            request.attestationPreference = ASAuthorizationPublicKeyCredentialAttestationKind(
                rawValue: registration.attestation
            )
            request.excludedCredentials = registration.excluded.map {
                ASAuthorizationPlatformPublicKeyCredentialDescriptor(credentialID: $0)
            }
            return request
        }
    }
}

extension Verification {
    var system: ASAuthorizationPublicKeyCredentialUserVerificationPreference {
        switch self {
        case .required: .required
        case .preferred: .preferred
        case .discouraged: .discouraged
        }
    }
}

extension SheetAnswer {
    /// What the system's credential holds, as plain values.
    init?(_ credential: ASAuthorizationCredential) {
        if let apple = credential as? ASAuthorizationAppleIDCredential {
            self = .appleID(
                identityToken: apple.identityToken, authorizationCode: apple.authorizationCode,
                givenName: apple.fullName?.givenName, familyName: apple.fullName?.familyName
            )
        } else if let assertion = credential as? ASAuthorizationPlatformPublicKeyCredentialAssertion {
            self = .assertion(
                credentialID: assertion.credentialID, clientDataJSON: assertion.rawClientDataJSON,
                authenticatorData: assertion.rawAuthenticatorData, signature: assertion.signature,
                userID: assertion.userID
            )
        } else if let registration = credential as? ASAuthorizationPlatformPublicKeyCredentialRegistration {
            self = .registration(
                credentialID: registration.credentialID, clientDataJSON: registration.rawClientDataJSON,
                attestationObject: registration.rawAttestationObject
            )
        } else {
            return nil
        }
    }
}

/// `ASAuthorizationController` over the app's key window.
@MainActor
final class SystemAuthorizationSheet: NSObject, AuthorizationSheet {
    /// Each open sheet's controller and the window it stands over, kept
    /// until it answers: the controller holds both weakly.
    private var open:
        [ObjectIdentifier: (controller: ASAuthorizationController, anchor: SheetAnchor, done: @MainActor (SheetOutcome) -> Void)] =
            [:]

    func present(_ request: SheetRequest, done: @escaping @MainActor (SheetOutcome) -> Void) {
        // No window yet, nothing to stand over: the system could not show it.
        guard let window = Windows.anchor else { return done(.refused(.unavailable)) }
        let anchor = SheetAnchor(window)
        let controller = ASAuthorizationController(authorizationRequests: [request.system])
        controller.delegate = self
        controller.presentationContextProvider = anchor
        open[ObjectIdentifier(controller)] = (controller, anchor, done)
        PresenceModel.shared.systemSheet(opened: true)
        controller.performRequests()
    }

    private func finish(_ controller: ObjectIdentifier, _ outcome: SheetOutcome) {
        guard let sheet = open.removeValue(forKey: controller) else { return }
        PresenceModel.shared.systemSheet(opened: false)
        sheet.done(outcome)
    }
}

extension SystemAuthorizationSheet: ASAuthorizationControllerDelegate {
    nonisolated func authorizationController(
        controller: ASAuthorizationController, didCompleteWithAuthorization authorization: ASAuthorization
    ) {
        let key = ObjectIdentifier(controller)
        let outcome = SheetAnswer(authorization.credential).map(SheetOutcome.answered) ?? .refused(.failed)
        MainActor.assumeIsolated { finish(key, outcome) }
    }

    nonisolated func authorizationController(controller: ASAuthorizationController, didCompleteWithError error: any Error) {
        let key = ObjectIdentifier(controller)
        let refusal = SheetOutcome.refusal(error)
        MainActor.assumeIsolated { finish(key, .refused(refusal)) }
    }
}
