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
    /// The clipboard a `copy` goes to: the system's, or a stand-in in tests.
    var clipboard: any Clipboard = SystemClipboard()
    /// What asks the system for this install's push token and gives it up:
    /// the system, or a stand-in in tests.
    var push: any PushToken = SystemPushToken()
    /// Where this window's core stands in a sign-out it said in two words:
    /// the session keeps it (``SessionKeeper/Tail``).
    var tail = SessionKeeper.Tail.none
    /// Whether this window's core has named who is signed in since it
    /// started, or since the session last changed under it.
    private var heard = false
    /// Whether this window has asked the system for the push token since
    /// its core last named somebody.
    private var askedPush = false
    /// What the app says at this window's file fields of the files it did
    /// not add. The words name a person's files: they go with the session.
    let fileNotes = FileNotes()
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
        heard = false
        askedPush = false
        keeper.windowBegins(restoring: restoring)
        // A sign-out the run before began was finished as this one started:
        // the push token that sign-out was giving up is given up, since its
        // removal on the server may never have been asked for.
        if signedWith.contains(.pushNotifications), keeper.takesPushRemoval() {
            push.giveUp()
            keeper.pushAwaitsSignIn = true
        }
    }

    /// Writes what is kept of the window under `key`, or deletes it; nothing
    /// is written for a session that has ended, nor while the sign-out of
    /// the session before is not finished on the device.
    func keep(snapshot: Data?, under key: String) {
        guard snapshot == nil || (isCurrent && keeper.keeps()) else { return }
        snapshots.writeData(key, snapshot)
    }

    /// The session ended: the requests and streams under way are dropped,
    /// and none of them is answered.
    func stopWork() {
        transport.stop()
    }

    /// The session ended in another window, or the person changed there:
    /// this window's core is told the client ended it, drops what it held of
    /// the person and starts again.
    func sessionEnded() {
        session = keeper.number
        heard = false
        askedPush = false
        fileNotes.clear()
        if let client, client.began { client.send(.signedOut) }
    }

    /// This window's own core ended the session, and goes on in the one that follows.
    func caughtUp() {
        session = keeper.number
        heard = false
        askedPush = false
        fileNotes.clear()
    }

    /// This window's core named the person the session is now: it goes on
    /// in the session as it is.
    func follows() {
        session = keeper.number
        fileNotes.clear()
    }

    /// This install's push token goes with the person, once for a sign-out;
    /// it is asked for again when somebody signs in.
    private func giveUpPushToken() {
        guard signedWith.contains(.pushNotifications), !keeper.pushAwaitsSignIn else { return }
        push.giveUp()
        keeper.pushAwaitsSignIn = true
    }

    /// Asks the system for this install's push token, once for each time
    /// this window's core comes to name somebody: only while somebody is
    /// signed in, so a start with nobody signed in asks for nothing, and the
    /// token a sign-out gave up stays given up until the next person signs in.
    /// Each window asks, so each window's core is handed the token.
    private func askForPush() {
        guard signedWith.contains(.pushNotifications), !askedPush else { return }
        askedPush = true
        keeper.pushAwaitsSignIn = false
        push.ask()
    }

    /// Hears who the window's core says is signed in, and tells the session
    /// (``SessionKeeper/hears(_:from:)``): somebody else than before, with
    /// no sign-out between, is a change of person the client is told of by
    /// nothing else. A core that names nobody says nothing of who was here
    /// or who comes next, and neither does one that is signing out.
    private func hear(_ client: NativeClient) {
        guard tail == .none, let user = client.app?.user else { return }
        heard = true
        askForPush()
        keeper.hears(Self.person(user), from: self)
    }

    /// Who a core's user is to the session: the key the core gives for them
    /// (none from a core that gives none, and an empty one is none), and the
    /// mark of the name and address it names them by.
    nonisolated static func person(_ user: Navree.User) -> SessionKeeper.Person {
        SessionKeeper.Person(
            key: user.key.flatMap { $0.isEmpty ? nil : $0 }, mark: SessionKeeper.mark(name: user.name, email: user.email)
        )
    }

    /// What loads the pictures of a screen, with the session's cookies.
    public private(set) lazy var pictures = PictureLoader(jar: transport.jar, site: site, hosts: pictureHosts)

    public func appChanged(for client: NativeClient) {
        self.client = client
        hear(client)
    }

    public func run(_ effect: CoreEffect, for client: NativeClient) {
        self.client = client
        if !heard { hear(client) }
        switch effect {
        case .fetch(let request):
            keeper.asks(by: self)
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
            devices.allowed = { [weak self, weak client] in
                // The person allowed notifications: the token is asked for, while somebody is signed in.
                guard let self, client?.app?.user != nil else { return }
                self.askedPush = false
                self.askForPush()
            }
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
            // core said before that it was ending, and then nothing is done
            // twice, the token's giving up neither.
            if keeper.end(by: self) { giveUpPushToken() }
        case .cacheRead(let key):
            client.send(.cache(key: key, value: isCurrent ? cache.read(key) : nil))
        case .cacheWrite(let key, let value):
            // Taking a key away is always done; a value is kept only for the
            // session as it is now, and not while the sign-out of the one
            // before is not finished on the device.
            if value == nil || (isCurrent && keeper.keeps()) { cache.write(key, value) }
        case .cacheClear:
            cache.clear()
        case .announce(let text, let notice, let count):
            if let words = text ?? Words.announcement(notice, count: count) {
                AccessibilityNotification.Announcement(words).post()
            }
        case .copy(let text, let sensitive):
            clipboard.put(text, options: ClipboardRules.options(sensitive: sensitive))
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

    /// `count` bytes from the system's secure generator, as hex; `nil` when it failed.
    nonisolated static func secureRandom(_ count: Int) -> String? {
        var bytes = [UInt8](repeating: 0, count: count)
        guard SecRandomCopyBytes(kSecRandomDefault, count, &bytes) == errSecSuccess else { return nil }
        defer { for index in bytes.indices { bytes[index] = 0 } }
        return bytes.map { String(format: "%02x", $0) }.joined()
    }
}

/// What asks the system for this install's push token and gives it up.
@MainActor
protocol PushToken: AnyObject {
    /// Asks the system for the token; it comes later, as an event of its own.
    func ask()
    /// Gives the token up: no push reaches this install until it is asked for again.
    func giveUp()
}

/// The system's own registration for push.
@MainActor
final class SystemPushToken: PushToken {
    func ask() { AppEvents.registerForPush() }
    func giveUp() { AppEvents.unregisterForPush() }
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

    /// What is said at a file field of a file the person picked and the app
    /// did not add: the file by its name, why, and what the person can do.
    public static func refused(_ name: String, _ why: FileRefusal) -> String {
        switch why {
        case .unreadable:
            String(localized: "\(name) could not be read and was not added.")
        case .placeUnchecked:
            String(localized: "\(name) is a kind of picture this device cannot check for the place it was taken, so it was not added.")
        case .placeStays:
            String(localized: "\(name) cannot be sent in this format without details that could say where it was taken, so it was not added. Save it as JPEG or PNG and pick it again.")
        case .filmPlaceStays:
            String(localized: "\(name) could not be prepared without the place it was made, so it was not added.")
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
