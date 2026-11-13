import AuthenticationServices
import CryptoKit
import Foundation
import NavreeProtocol
import NavreeRuntime
import StoreKit
import UserNotifications

#if canImport(UIKit)
import UIKit
#endif

/// The device calls this build carries out, each answered to the core with
/// the result's fields or one of the protocol's refusals. What the core then
/// posts, where, and with which ticket is the core's business: no request is
/// made here.
///
/// - `share`: the system's share sheet; `completed` says whether the person shared.
/// - `open_settings`, `request_review`: the system's settings, its review prompt.
/// - `register_push`: the system's prompt to allow notifications, answered
///   with `granted`; the token comes later, as an event of its own.
/// - `platform_sign_in`: Sign in with Apple, with the hash of the core's nonce.
/// - `confirm`: a passkey's signature over the challenge the core asked the server for.
///
/// The last two need a build signed by a developer team with their
/// entitlements (Sign in with Apple; the origin's associated domain): until
/// the app says it has them, they are not among its capabilities, the server
/// never offers them, and a call that comes anyway is refused as not built.
@MainActor
final class DeviceCalls: NSObject {
    /// What the build is signed with.
    let signedWith: Set<Entitlement>
    /// Where answers go.
    var answer: (Int, CoreEvent.DeviceOutcome) -> Void = { _, _ in }
    /// Notifications were allowed: the app may ask the system for its token.
    var allowed: () -> Void = {}
    /// The origin, which a shared address inside the app is read against.
    let origin: URL?

    private var authorizing: [ObjectIdentifier: (call: Int, controller: ASAuthorizationController)] = [:]

    init(signedWith: Set<Entitlement>, origin: URL?) {
        self.signedWith = signedWith
        self.origin = origin
    }

    func run(call: Int, device: DeviceRequest, posts: Bool, options: Navree.JSONValue?) {
        switch device {
        case .call(.share(let share)) where posts:
            self.share(call, share)
        case .call(.openSettings(let settings)):
            open(settings: settings.page)
            if posts { answer(call, .done([])) }
        case .reply(.requestReview):
            requestReview()
        case .call(.registerPush) where posts:
            allowNotifications(call)
        case .call(.platformSignIn(let signIn)) where posts && signedWith.contains(.signInWithApple):
            signInWithApple(call, provider: signIn.provider, nonce: options?["nonce"]?.text)
        case .call(.confirm) where posts && signedWith.contains(.associatedDomains):
            confirm(call, options: options?["publicKey"])
        default:
            // Not carried out by this build: it cannot be asked for when the
            // capabilities are honest, and is answered when it posts.
            if posts { answer(call, .refused(.notBuilt)) }
        }
    }

    // MARK: - Share

    private func share(_ call: Int, _ share: Navree.Share) {
        #if canImport(UIKit)
        var items: [Any] = []
        if let text = share.text { items.append(text) }
        let link = share.url.flatMap { URL(string: $0) } ?? share.href.flatMap { URL(string: $0, relativeTo: origin)?.absoluteURL }
        if let link { items.append(link) }
        if items.isEmpty, let title = share.title { items.append(title) }
        guard !items.isEmpty, let top = Windows.top else { return answer(call, .refused(.unavailable)) }
        let sheet = UIActivityViewController(activityItems: items, applicationActivities: nil)
        if let title = share.title { sheet.title = title }
        sheet.completionWithItemsHandler = { [weak self] _, completed, _, _ in
            MainActor.assumeIsolated {
                self?.answer(call, .done([Navree.Pair(name: "completed", value: completed ? "true" : "false")]))
            }
        }
        if let popover = sheet.popoverPresentationController, let view = top.view {
            popover.sourceView = view
            popover.sourceRect = CGRect(x: view.bounds.midX, y: view.bounds.midY, width: 1, height: 1)
            popover.permittedArrowDirections = []
        }
        top.present(sheet, animated: true)
        #else
        answer(call, .refused(.unsupported))
        #endif
    }

    // MARK: - Settings and the review prompt

    private func open(settings page: Navree.SettingsPage) {
        #if canImport(UIKit)
        let address = page == .notifications ? UIApplication.openNotificationSettingsURLString : UIApplication.openSettingsURLString
        if let url = URL(string: address) { UIApplication.shared.open(url) }
        #endif
    }

    private func requestReview() {
        #if canImport(UIKit)
        if let scene = Windows.scene { AppStore.requestReview(in: scene) }
        #endif
    }

    // MARK: - Notifications

    /// The system's prompt; a no, now or before, is an answer too.
    private func allowNotifications(_ call: Int) {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound]) { [weak self] granted, _ in
            Task { @MainActor in
                guard let self else { return }
                if granted && self.signedWith.contains(.pushNotifications) { self.allowed() }
                self.answer(call, .done([Navree.Pair(name: "granted", value: granted ? "true" : "false")]))
            }
        }
    }

    // MARK: - Sign in with Apple

    /// Apple's own sheet, with the lower-case hex SHA-256 of the core's nonce;
    /// the identity token and code go back as text.
    private func signInWithApple(_ call: Int, provider: Navree.SignInProvider, nonce: String?) {
        guard provider == .apple else { return answer(call, .refused(.unsupported)) }
        guard let nonce else { return answer(call, .refused(.unavailable)) }
        let request = ASAuthorizationAppleIDProvider().createRequest()
        request.requestedScopes = [.fullName, .email]
        request.nonce = Self.hashed(nonce)
        authorize(call, [request])
    }

    /// The nonce as Apple's request takes it: SHA-256, lower-case hex.
    static func hashed(_ nonce: String) -> String {
        SHA256.hash(data: Data(nonce.utf8)).map { String(format: "%02x", $0) }.joined()
    }

    // MARK: - Confirm with a passkey

    /// Signs the server's challenge with a passkey of the person's, with user
    /// verification required, as WebAuthn's request asked.
    private func confirm(_ call: Int, options publicKey: Navree.JSONValue?) {
        guard let publicKey, let rp = publicKey["rpId"]?.text,
            let challenge = publicKey["challenge"]?.text.flatMap(Base64URL.decode)
        else { return answer(call, .refused(.failed)) }
        let provider = ASAuthorizationPlatformPublicKeyCredentialProvider(relyingPartyIdentifier: rp)
        let request = provider.createCredentialAssertionRequest(challenge: challenge)
        request.userVerificationPreference = .required
        if case .array(let allowed)? = publicKey["allowCredentials"] {
            request.allowedCredentials = allowed.compactMap { entry in
                entry["id"]?.text.flatMap(Base64URL.decode).map { ASAuthorizationPlatformPublicKeyCredentialDescriptor(credentialID: $0) }
            }
        }
        authorize(call, [request])
    }

    // MARK: - The system's sheet

    private func authorize(_ call: Int, _ requests: [ASAuthorizationRequest]) {
        let controller = ASAuthorizationController(authorizationRequests: requests)
        controller.delegate = self
        controller.presentationContextProvider = self
        authorizing[ObjectIdentifier(controller)] = (call, controller)
        controller.performRequests()
    }

    private func finish(_ controller: ObjectIdentifier, _ outcome: CoreEvent.DeviceOutcome) {
        guard let (call, _) = authorizing.removeValue(forKey: controller) else { return }
        answer(call, outcome)
    }

    /// The fields an authorization answers with, in the protocol's names.
    nonisolated static func fields(_ credential: ASAuthorizationCredential) -> [Navree.Pair]? {
        if let apple = credential as? ASAuthorizationAppleIDCredential {
            guard let token = apple.identityToken.flatMap({ String(data: $0, encoding: .utf8) }) else { return nil }
            var fields = [Navree.Pair(name: "id_token", value: token)]
            if let code = apple.authorizationCode.flatMap({ String(data: $0, encoding: .utf8) }) {
                fields.append(Navree.Pair(name: "authorization_code", value: code))
            }
            if let given = apple.fullName?.givenName { fields.append(Navree.Pair(name: "first_name", value: given)) }
            if let family = apple.fullName?.familyName { fields.append(Navree.Pair(name: "last_name", value: family)) }
            return fields
        }
        if let assertion = credential as? ASAuthorizationPlatformPublicKeyCredentialAssertion {
            return [Navree.Pair(name: "credential", value: WebAuthn.assertion(assertion))]
        }
        return nil
    }

    /// The refusal an authorization's error means.
    nonisolated static func refusal(_ error: any Error) -> Navree.Refusal {
        switch (error as? ASAuthorizationError)?.code {
        case .canceled?: .cancelled
        case .notHandled?, .unknown?: .unavailable
        default: .failed
        }
    }
}

extension DeviceCalls: ASAuthorizationControllerDelegate, ASAuthorizationControllerPresentationContextProviding {
    nonisolated func authorizationController(
        controller: ASAuthorizationController, didCompleteWithAuthorization authorization: ASAuthorization
    ) {
        let key = ObjectIdentifier(controller)
        let outcome = Self.fields(authorization.credential).map(CoreEvent.DeviceOutcome.done) ?? .refused(.failed)
        MainActor.assumeIsolated {
            finish(key, outcome)
        }
    }

    nonisolated func authorizationController(controller: ASAuthorizationController, didCompleteWithError error: any Error) {
        let key = ObjectIdentifier(controller)
        let refusal = Self.refusal(error)
        MainActor.assumeIsolated {
            finish(key, .refused(refusal))
        }
    }

    nonisolated func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor {
        MainActor.assumeIsolated { Windows.key ?? ASPresentationAnchor() }
    }
}

/// A passkey's assertion as WebAuthn's `toJSON()` writes it: what the server's
/// confirmation route reads.
enum WebAuthn {
    nonisolated static func assertion(_ assertion: ASAuthorizationPlatformPublicKeyCredentialAssertion) -> String {
        var response: [String: String] = [
            "clientDataJSON": Base64URL.encode(assertion.rawClientDataJSON),
            "authenticatorData": Base64URL.encode(assertion.rawAuthenticatorData),
            "signature": Base64URL.encode(assertion.signature),
        ]
        if let user = assertion.userID, !user.isEmpty { response["userHandle"] = Base64URL.encode(user) }
        let id = Base64URL.encode(assertion.credentialID)
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
