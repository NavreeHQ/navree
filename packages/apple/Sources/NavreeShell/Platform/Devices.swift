import Foundation
import NavreeProtocol
import NavreeRuntime
import StoreKit

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
/// - `passkey`: a passkey made or used, with the options of its ceremony when
///   the core hands them over.
///
/// The last four need a build signed by a developer team with their
/// entitlements (push; Sign in with Apple; the origin as an associated domain
/// for passkeys): until the app says it has them, they are not among its
/// capabilities, the server never offers them, and a call that comes anyway
/// is refused as not built.
@MainActor
final class DeviceCalls {
    /// What the build is signed with.
    let signedWith: Set<Entitlement>
    /// Where answers go.
    var answer: (Int, CoreEvent.DeviceOutcome) -> Void = { _, _ in }
    /// Notifications were allowed: the app may ask the system for its token.
    var allowed: () -> Void = {}
    /// The origin, which a shared address inside the app is read against.
    let origin: URL?
    /// What shows the system's sign-in sheets.
    var sheet: any AuthorizationSheet = SystemAuthorizationSheet()
    /// What asks the person to allow notifications.
    var permission: any NotificationPermission = SystemNotificationPermission()

    /// The call whose sheet is showing: one at a time.
    private var showing: Int?

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
        case .call(.registerPush) where posts && signedWith.contains(.pushNotifications):
            allowNotifications(call)
        case .call(.platformSignIn(let signIn)) where posts && signedWith.contains(.signInWithApple):
            // Apple's own sheet signs in with Apple only; the core goes the web's way for any other.
            guard signIn.provider == .apple else { return answer(call, .refused(.unsupported)) }
            ask(call, SheetRequest.appleID(options: options))
        case .call(.confirm(let confirm)) where posts && signedWith.contains(.associatedDomains):
            ask(call, SheetRequest.confirm(confirm, options: options))
        case .call(.passkey(let passkey)) where posts && signedWith.contains(.associatedDomains):
            ask(call, SheetRequest.passkey(passkey, options: options))
        default:
            // Not carried out by this build: it cannot be asked for when the
            // capabilities are honest, and is answered when it posts.
            if posts { answer(call, .refused(.notBuilt)) }
        }
    }

    /// Whether a call is carried out with one of the system's sign-in sheets.
    static func usesSheet(_ device: DeviceRequest) -> Bool {
        switch device {
        case .call(.platformSignIn), .call(.confirm), .call(.passkey): true
        default: false
        }
    }

    // MARK: - The system's sign-in sheets

    private func ask(_ call: Int, _ ask: SheetAsk) {
        switch ask {
        case .refused(let refusal):
            answer(call, .refused(refusal))
        case .sheet(let request):
            // The system shows one such sheet at a time.
            guard showing == nil else { return answer(call, .refused(.busy)) }
            showing = call
            sheet.present(request) { [weak self] outcome in
                guard let self else { return }
                if self.showing == call { self.showing = nil }
                self.answer(call, outcome.device)
            }
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

    /// The system's prompt; a no, now or before, is an answer too. Once
    /// allowed, the system is asked for this install's token.
    private func allowNotifications(_ call: Int) {
        permission.ask { [weak self] granted in
            guard let self else { return }
            if granted { self.allowed() }
            self.answer(call, .done([Navree.Pair(name: "granted", value: granted ? "true" : "false")]))
        }
    }
}
