import Accessibility
import Foundation
import NavreeProtocol
import NavreeRuntime
import Security
import UniformTypeIdentifiers
import WebKit
import os

#if canImport(UIKit)
import SafariServices
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// Carries out the runtime core's effects with the system's own means. What
/// comes of each is a later event, sent to the client on the main actor.
@MainActor
public final class SystemEffects: EffectRunner {
    /// What answers the core's requests.
    public let transport: any Transport
    /// The core's key-value cache, kept between launches.
    private(set) var cache: KeptFiles
    /// What is kept of a launch the system may end.
    private(set) var snapshots: KeptFiles
    let signIn: SignInSession
    let devices: DeviceCalls
    /// The origin addresses are checked against.
    let site: Origin?
    /// Other hosts pictures may come from.
    let pictureHosts: [String]
    /// What the build is signed with.
    let signedWith: Set<Entitlement>
    /// The push token was given up when a session ended: it is asked for
    /// again once somebody is signed in.
    private(set) var pushAwaitsSignIn = false
    private var timers: [Int: Task<Void, Never>] = [:]

    static let log = Logger(subsystem: "com.navree.native", category: "effects")

    public init(
        transport: any Transport, cache: KeptFiles = KeptFiles("navree-cache", in: .cachesDirectory),
        snapshots: KeptFiles = KeptFiles("navree-snapshot"), signedWith: Set<Entitlement> = [], origin: URL? = nil,
        site: Origin? = nil, pictureHosts: [String] = []
    ) {
        self.transport = transport
        self.cache = cache
        self.snapshots = snapshots
        let site = site ?? transport.jar?.site
        self.site = site
        self.pictureHosts = pictureHosts
        self.signedWith = signedWith
        signIn = SignInSession(signedWith: signedWith, origin: origin)
        devices = DeviceCalls(signedWith: signedWith, origin: origin, site: site)
    }

    /// The cookie store the web view shares.
    public var jar: CookieJar? { transport.jar }

    /// Gives the core of one scene (a window, on iPad and the Mac) a cache
    /// and a snapshot folder of its own inside the app's, before it starts:
    /// the core serving another scene, perhaps for another person, never
    /// reads or writes them, and they are emptied with this scene's session.
    func keep(for scene: String) {
        guard !scene.isEmpty else { return }
        cache.clearLoose()
        snapshots.clearLoose()
        cache = cache.within(scene)
        snapshots = snapshots.within(scene)
    }

    /// What loads the pictures of a screen, with the session's cookies.
    public private(set) lazy var pictures = PictureLoader(jar: transport.jar, site: site, hosts: pictureHosts)

    public func run(_ effect: CoreEffect, for client: NativeClient) {
        if pushAwaitsSignIn, client.app?.user != nil {
            pushAwaitsSignIn = false
            AppEvents.registerForPush()
        }
        switch effect {
        case .fetch(let request):
            transport.fetch(request) { [weak client] response in client?.send(.response(response)) }
        case .cancel(let request):
            transport.cancel(request)
        case .openURL(let url, let inside):
            Self.open(url, inside: inside)
        case .signIn(let url, let callback, let ephemeral):
            guard let address = URL(string: url) else { return client.send(.signInDone(url: nil)) }
            signIn.start(address, callback: callback, ephemeral: ephemeral) { [weak client] returned in
                client?.send(.signInDone(url: returned?.absoluteString))
            }
        case .message:
            // Messages are drawn from the view state's list.
            break
        case .haptic(let kind):
            Self.feel(kind)
        case .device(let call, let device, let posts, let options):
            devices.answer = { [weak client] call, outcome in client?.send(.deviceResult(call: call, outcome: outcome)) }
            devices.allowed = { AppEvents.registerForPush() }
            devices.run(call: call, device: device, posts: posts, options: options)
        case .random(let request, let count):
            // Answered at once. The bytes are a secret: never logged, never kept.
            guard let bytes = Self.secureRandom(max(count, 48)) else {
                return client.send(.random(request: request, bytes: ""))
            }
            client.send(.random(request: request, bytes: bytes))
        case .forgetSession:
            // The person signed out: what was kept of them goes with the
            // session, here and now, before the next effect; the web view's
            // store before it is used again.
            snapshots.clear()
            cache.clear()
            FileStore.shared.clear()
            pictures.forget()
            transport.jar?.forget(WKWebsiteDataStore.default())
            // This install's token goes with the person; it is asked for again
            // once somebody signs in.
            if signedWith.contains(.pushNotifications) {
                AppEvents.unregisterForPush()
                pushAwaitsSignIn = true
            }
        case .cacheRead(let key):
            client.send(.cache(key: key, value: cache.read(key)))
        case .cacheWrite(let key, let value):
            cache.write(key, value)
        case .cacheClear:
            cache.clear()
        case .announce(let text, let notice, let count):
            if let words = text ?? Words.announcement(notice, count: count) {
                AccessibilityNotification.Announcement(words).post()
            }
        case .copy(let text, let sensitive):
            Self.copy(text, sensitive: sensitive)
        case .timer(let timer, let ms):
            timers[timer]?.cancel()
            timers[timer] = Task { [weak self, weak client] in
                try? await Task.sleep(for: .milliseconds(ms))
                guard !Task.isCancelled, self?.timers.removeValue(forKey: timer) != nil else { return }
                client?.send(.timer(timer))
            }
        case .cancelTimer(let timer):
            timers.removeValue(forKey: timer)?.cancel()
        case .streamOpen(let stream, let url, let headers):
            transport.open(stream: stream, url: url, headers: headers) { [weak client] state in
                client?.send(.stream(stream: stream, state: state))
            }
        case .streamClose(let stream):
            transport.close(stream: stream)
        case .exit:
            // An app on Apple's systems never ends itself: the person leaves it.
            break
        case .report(let what, let detail):
            #if DEBUG
            Self.log.debug("\(what, privacy: .public): \(detail)")
            #endif
        case .unknown(let tag):
            Self.log.error("an effect this build does not know: \(tag, privacy: .public)")
        }
    }

    // MARK: - What the person sees and feels

    /// The schemes an address the app opens may have: a web page, mail, a
    /// call, a message, a place. Never a file or an app named by its scheme.
    nonisolated static let openSchemes: Set<String> = ["http", "https", "mailto", "tel", "sms", "geo"]

    /// The address the system may be asked to open, or `nil`: a whole `http`
    /// or `https` address with a host and no user, written again from its
    /// parts, or a `mailto:`, `tel:`, `sms:` or `geo:` one. Text with white
    /// space, a control character or a backslash is never opened.
    nonisolated static func openable(_ address: String) -> URL? {
        guard Origin.isClean(address), let url = URL(string: address), let scheme = url.scheme?.lowercased(),
            openSchemes.contains(scheme)
        else { return nil }
        if scheme == "http" || scheme == "https" { return Origin.read(address)?.url }
        return url
    }

    /// The system browser or the address's own app; with `inside`, Safari
    /// inside the app. Never the web view, and only what `openable` allows.
    static func open(_ address: String, inside: Bool) {
        guard let url = openable(address) else {
            Self.log.error("an address the app does not open was left unopened")
            return
        }
        #if canImport(UIKit)
        let web = ["http", "https"].contains(url.scheme?.lowercased() ?? "")
        if inside, web, let top = Windows.top {
            top.present(SFSafariViewController(url: url), animated: true)
        } else {
            UIApplication.shared.open(url)
        }
        #else
        NSWorkspace.shared.open(url)
        #endif
    }

    static func feel(_ kind: String) {
        #if os(iOS)
        switch kind {
        case "success": UINotificationFeedbackGenerator().notificationOccurred(.success)
        case "warning": UINotificationFeedbackGenerator().notificationOccurred(.warning)
        case "error": UINotificationFeedbackGenerator().notificationOccurred(.error)
        case "selection": UISelectionFeedbackGenerator().selectionChanged()
        case "heavy": UIImpactFeedbackGenerator(style: .heavy).impactOccurred()
        case "medium": UIImpactFeedbackGenerator(style: .medium).impactOccurred()
        default: UIImpactFeedbackGenerator(style: .light).impactOccurred()
        }
        #endif
    }

    /// The clipboard. A secret stays on this device (no Universal Clipboard)
    /// and is gone after a minute.
    static func copy(_ text: String, sensitive: Bool) {
        #if canImport(UIKit)
        if sensitive {
            UIPasteboard.general.setItems(
                [[UTType.utf8PlainText.identifier: text]],
                options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(60)]
            )
        } else {
            UIPasteboard.general.string = text
        }
        #else
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        #endif
    }

    /// `count` bytes from the system's secure generator, as hex; `nil` when it failed.
    nonisolated static func secureRandom(_ count: Int) -> String? {
        var bytes = [UInt8](repeating: 0, count: count)
        guard SecRandomCopyBytes(kSecRandomDefault, count, &bytes) == errSecSuccess else { return nil }
        defer { for index in bytes.indices { bytes[index] = 0 } }
        return bytes.map { String(format: "%02x", $0) }.joined()
    }
}

/// What the app hears from the system outside any view: the push token, a
/// notification tapped. The app's delegate passes them on here; the shell
/// tells the core.
public enum AppEvents {
    static let pushToken = Notification.Name("navree.push-token")
    static let notification = Notification.Name("navree.notification")

    /// The system gave or changed the push token
    /// (`didRegisterForRemoteNotificationsWithDeviceToken`).
    @MainActor public static func received(pushToken token: Data) {
        NotificationCenter.default.post(name: pushToken, object: nil, userInfo: ["token": hex(token)])
    }

    /// A notification was tapped; `href` is the path its data carries.
    @MainActor public static func opened(notification href: String) {
        NotificationCenter.default.post(name: notification, object: nil, userInfo: ["href": href])
    }

    /// Asks the system for this install's push token: at every launch of a
    /// build signed for push, and when the person allows notifications.
    @MainActor static func registerForPush() {
        #if canImport(UIKit)
        UIApplication.shared.registerForRemoteNotifications()
        #endif
    }

    /// Gives up this install's push token: the person signed out.
    @MainActor static func unregisterForPush() {
        #if canImport(UIKit)
        UIApplication.shared.unregisterForRemoteNotifications()
        #endif
    }

    /// Apple's token as hex, two characters a byte.
    static func hex(_ token: Data) -> String {
        token.map { String(format: "%02x", $0) }.joined()
    }

    /// Whether this build talks to the push service's sandbox: a debug build.
    static var sandbox: Bool {
        #if DEBUG
        true
        #else
        false
        #endif
    }
}

/// The client's own words: the core names a notice or a failure, and the app
/// says it in the person's language.
public enum Words {
    public static func notice(_ notice: CoreNotice?) -> String {
        switch notice {
        case .offline?: String(localized: "You're offline. Check your connection and try again.")
        case .timeout?: String(localized: "The server took too long to answer. Try again.")
        case .unsaved?: String(localized: "Your latest change wasn't saved. Check it and save again.")
        case .loading?: String(localized: "Still loading. Try again in a moment.")
        case .copied?: String(localized: "Copied")
        case .secondStep?: String(localized: "One more step is needed to sign in. Sign in another way to finish.")
        case .fieldsNeedAttention?: String(localized: "Some fields need attention.")
        case .loaded?: String(localized: "More loaded.")
        case .failed?, nil: String(localized: "That didn't work. Try again.")
        }
    }

    /// What VoiceOver says for a notice.
    public static func announcement(_ notice: CoreNotice?, count: Int?) -> String? {
        switch notice {
        case .fieldsNeedAttention?:
            let count = count ?? 1
            return count == 1
                ? String(localized: "1 field needs attention")
                : String(localized: "\(count) fields need attention")
        case .loaded?:
            let count = count ?? 0
            return count == 1 ? String(localized: "1 more row loaded") : String(localized: "\(count) more rows loaded")
        case .copied?:
            return String(localized: "Copied")
        case nil:
            return nil
        default:
            return Words.notice(notice)
        }
    }

    /// Why a screen or the app could not be loaded.
    public static func failure(_ failure: Failure?) -> (title: String, text: String) {
        switch failure {
        case .offline?:
            (String(localized: "You're Offline"), String(localized: "Check your connection and try again."))
        case .timeout?:
            (String(localized: "The Server Took Too Long"), String(localized: "Try again in a moment."))
        case .protocol?, .config?:
            (String(localized: "This App Needs an Update"), String(localized: "The server answered in a way this version can't show."))
        case .server?, nil:
            (String(localized: "Couldn't Load This Page"), String(localized: "Something went wrong on the server. Try again."))
        }
    }
}
