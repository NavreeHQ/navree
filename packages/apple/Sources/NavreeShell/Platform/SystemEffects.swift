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
///
/// Each window has one of these for its core. The session is not a window's:
/// every one of them shares the app's ``SessionKeeper``, and writes for the
/// person only while its core runs in the session as it is now.
@MainActor
public final class SystemEffects: EffectRunner {
    /// What answers the core's requests.
    public let transport: any Transport
    /// The app's one session, for every window.
    let keeper: SessionKeeper
    let signIn: SignInSession
    let devices: DeviceCalls
    /// The origin addresses are checked against.
    let site: Origin?
    /// Other hosts pictures may come from.
    let pictureHosts: [String]
    /// What the build is signed with.
    let signedWith: Set<Entitlement>
    /// The number of the session this window's core runs in: the session's
    /// own, from the moment the core is told of each change of it.
    private(set) var session: Int
    /// The client whose effects these are, once it has asked for one.
    private weak var client: NativeClient?
    private var timers: [Int: Task<Void, Never>] = [:]

    static let log = Logger(subsystem: "com.navree.native", category: "effects")

    /// Effects for a window that is the app's only one: a session of its own
    /// over the transport's jar, kept in `cache` and `snapshots`.
    public convenience init(
        transport: any Transport, cache: KeptFiles = KeptFiles("navree-cache", in: .cachesDirectory),
        snapshots: KeptFiles = KeptFiles("navree-snapshot"), signedWith: Set<Entitlement> = [], origin: URL? = nil,
        site: Origin? = nil, pictureHosts: [String] = []
    ) {
        self.init(
            transport: transport, keeper: SessionKeeper(jar: transport.jar, cache: cache, snapshots: snapshots),
            signedWith: signedWith, origin: origin, site: site, pictureHosts: pictureHosts
        )
    }

    /// Effects for one window of the app, in the session `keeper` holds.
    init(
        transport: any Transport, keeper: SessionKeeper, signedWith: Set<Entitlement> = [], origin: URL? = nil,
        site: Origin? = nil, pictureHosts: [String] = []
    ) {
        self.transport = transport
        self.keeper = keeper
        session = keeper.number
        let site = site ?? transport.jar?.site
        self.site = site
        self.pictureHosts = pictureHosts
        self.signedWith = signedWith
        signIn = SignInSession(signedWith: signedWith, origin: origin)
        devices = DeviceCalls(signedWith: signedWith, origin: origin, site: site)
        keeper.join(self)
    }

    /// The cookie store the web view shares.
    public var jar: CookieJar? { transport.jar }

    /// The core's key-value cache, kept between launches: the session's.
    var cache: KeptFiles { keeper.cache }
    /// What is kept of each window for a launch the system may end: the session's.
    var snapshots: KeptFiles { keeper.snapshots }

    /// The push token was given up when a session ended: it is asked for
    /// again once somebody is signed in.
    var pushAwaitsSignIn: Bool { keeper.pushAwaitsSignIn }

    /// Whether this window's core runs in the session as it is now. While
    /// it does not, nothing it asks is written for the person.
    var isCurrent: Bool { session == keeper.number }

    /// The window's core is about to start: it starts in the session as it
    /// is now. `restoring` says whether the system brings a main window back
    /// (`nil` for a window that is not one of the app's main ones).
    func begins(restoring: Bool?) {
        session = keeper.number
        keeper.windowBegins(restoring: restoring)
    }

    /// Writes what is kept of the window under `key`, or deletes it; nothing
    /// is written for a session that has ended.
    func keep(snapshot: Data?, under key: String) {
        guard snapshot == nil || isCurrent else { return }
        snapshots.writeData(key, snapshot)
    }

    /// The session ended: the requests and streams under way are dropped,
    /// and none of them is answered.
    func stopWork() {
        transport.stop()
    }

    /// The session ended in another window: this window's core is told the
    /// client ended it, drops what it held of the person and starts again.
    func sessionEnded() {
        session = keeper.number
        named = nil
        if let client, client.began { client.send(.signedOut) }
    }

    /// This window's own core ended the session, and goes on in the one that follows.
    func caughtUp() {
        session = keeper.number
        named = nil
    }

    /// This install's push token goes with the person, once for a sign-out;
    /// it is asked for again when somebody signs in.
    private func giveUpPushToken() {
        guard signedWith.contains(.pushNotifications), !keeper.pushAwaitsSignIn else { return }
        AppEvents.unregisterForPush()
        keeper.pushAwaitsSignIn = true
    }

    /// The person this window's core last said is signed in.
    private var named: SessionKeeper.Person?

    /// Hears who the window's core says is signed in. Somebody else than
    /// before, with no sign-out between, is a change of person the core was
    /// not told of by a sign-out: the session takes away what was the last
    /// person's (``SessionKeeper/personChanged(by:)``).
    private func hear(_ user: Navree.User?) {
        guard let user else { return }
        let now = SessionKeeper.Person(name: user.name, email: user.email)
        defer { named = now }
        if let named, named != now { keeper.personChanged(by: self) }
    }

    /// What loads the pictures of a screen, with the session's cookies.
    public private(set) lazy var pictures = PictureLoader(jar: transport.jar, site: site, hosts: pictureHosts)

    public func run(_ effect: CoreEffect, for client: NativeClient) {
        self.client = client
        hear(client.app?.user)
        if keeper.pushAwaitsSignIn, client.app?.user != nil {
            keeper.pushAwaitsSignIn = false
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
        case .sessionEnding:
            // The person signed out, and the core is still asking the server
            // to forget the push token: everything on the device goes now,
            // for every window, without waiting for that answer.
            keeper.ending(by: self)
            giveUpPushToken()
        case .forgetSession:
            // The person signed out: the session ends for every window, here
            // and now, before the next effect; or it already has, when the
            // core said before that it was ending.
            keeper.end(by: self)
            giveUpPushToken()
        case .cacheRead(let key):
            client.send(.cache(key: key, value: isCurrent ? cache.read(key) : nil))
        case .cacheWrite(let key, let value):
            // Taking a key away is always done; a value is kept only for the
            // session as it is now.
            if value == nil || isCurrent { cache.write(key, value) }
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
    /// The words of a notice. One this build has no words for (a newer
    /// core's), and a message that names none, is said by the message's
    /// `tone` alone: that something failed, or only that something happened.
    /// It never says to try again: the notice may be about a payment or an
    /// order the server took.
    public static func notice(_ notice: CoreNotice?, tone: Navree.Tone = .neutral) -> String {
        switch notice {
        case .offline?: String(localized: "You're offline. Check your connection and try again.")
        case .timeout?: String(localized: "The server took too long to answer. Try again.")
        case .unsaved?: String(localized: "Your latest change wasn't saved. Check it and save again.")
        case .loading?: String(localized: "Still loading. Try again in a moment.")
        case .copied?: String(localized: "Copied")
        case .secondStep?: String(localized: "One more step is needed to sign in. Sign in another way to finish.")
        case .fieldsNeedAttention?: String(localized: "Some fields need attention.")
        case .loaded?: String(localized: "More loaded.")
        case .savedNotRefreshed?: String(localized: "Saved. The screen couldn't be brought up to date.")
        case .failed?: String(localized: "That didn't work. Try again.")
        case .unknown?, nil:
            tone == .danger
                ? String(localized: "That didn't work.")
                : String(localized: "Something happened that this version of the app can't describe.")
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
        case .unknown?, nil:
            // Nothing this build can put into words is said aloud.
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
