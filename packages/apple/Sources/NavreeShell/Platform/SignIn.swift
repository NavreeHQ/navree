import AuthenticationServices
import Foundation
import os

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// Where a sign-in session comes back, read from the core's words.
enum SignInReturn: Equatable, Sendable {
    /// A link of a site: only this session sees it, so no other app can catch
    /// it, but the build needs the site as an associated domain.
    case https(host: String, path: String)
    /// The app's own scheme, which any build can take.
    case scheme(String)

    /// The return the core names: an `https` link by its host and path, or a
    /// scheme (`charkha`, `charkha:/signed-in`, `charkha://signed-in`).
    init?(_ text: String?) {
        guard let text, !text.isEmpty else { return nil }
        if let url = URL(string: text), url.scheme?.lowercased() == "https" {
            guard let host = url.host, !host.isEmpty else { return nil }
            self = .https(host: host.lowercased(), path: url.path.isEmpty ? "/" : url.path)
            return
        }
        let scheme = String(text.prefix { $0 != ":" })
        // A scheme as addresses write one: a letter, then letters, digits, `+`, `-` or `.`.
        guard let first = scheme.unicodeScalars.first, CharacterSet.letters.contains(first),
            scheme.unicodeScalars.allSatisfy({ $0.isASCII && (CharacterSet.alphanumerics.contains($0) || "+-.".unicodeScalars.contains($0)) }),
            !["http", "https"].contains(scheme.lowercased())
        else { return nil }
        self = .scheme(scheme)
    }

    /// Whether this build can take the return: a scheme always; a link only
    /// on the origin's own host, and only when the build is signed with the
    /// origin as its associated domain. A session whose return the app could
    /// never take would leave the person on the site's page with no way back.
    func catchable(signedWith: Set<Entitlement>, origin: URL?) -> Bool {
        switch self {
        case .scheme:
            return true
        case .https(let host, _):
            return signedWith.contains(.associatedDomains) && origin?.host?.lowercased() == host
        }
    }

    var system: ASWebAuthenticationSession.Callback {
        switch self {
        case .https(let host, let path): .https(host: host, path: path)
        case .scheme(let scheme): .customScheme(scheme)
        }
    }
}

/// What opens the system's sign-in session: the system itself, or a stand-in
/// in tests. `done` gets the whole address the session came back on, or
/// `nil` when the person closed it or it failed.
@MainActor
protocol SignInOpener: AnyObject {
    func open(_ url: URL, returning: SignInReturn, ephemeral: Bool, done: @escaping @MainActor (URL?) -> Void)
        -> (any SignInHandle)?
}

/// A session that is open.
@MainActor
protocol SignInHandle: AnyObject {
    func cancel()
}

/// The system's sign-in session, never the app's web view: providers refuse
/// embedded browsers, and the system's session shows the person the real
/// address and their saved passwords.
///
/// The runtime core gives the address to open, with the hand-over's challenge
/// and state already in it, and where the session comes back. The client
/// opens exactly that, and hands back the whole address it came back on, or
/// nothing when the person closed it. It never treats a return as a sign-in
/// itself. One session at a time: a newer one takes the place of one that is
/// open, and what the older one says afterwards is not passed on.
@MainActor
final class SignInSession {
    let signedWith: Set<Entitlement>
    let origin: URL?
    var opener: any SignInOpener = SystemSignInOpener()

    private var open: (any SignInHandle)?
    private var serial = 0

    static let log = Logger(subsystem: "com.navree.native", category: "sign-in")

    init(signedWith: Set<Entitlement> = [], origin: URL? = nil) {
        self.signedWith = signedWith
        self.origin = origin
    }

    /// The return the core names, as the system's session takes it.
    static func callback(_ text: String?) -> ASWebAuthenticationSession.Callback? {
        SignInReturn(text)?.system
    }

    /// Opens a session at `url`; `done` gets the whole address it came back
    /// on, or `nil`. A session already open is closed first.
    func start(_ url: URL, callback: String?, ephemeral: Bool, done: @escaping @MainActor (URL?) -> Void) {
        serial += 1
        let mine = serial
        open?.cancel()
        open = nil
        guard let returning = SignInReturn(callback) else {
            Self.log.error("a sign-in session with no return it can be told to come back on")
            return done(nil)
        }
        guard returning.catchable(signedWith: signedWith, origin: origin) else {
            Self.log.error(
                "a sign-in session that comes back on a link of the site needs the build signed with the site as its associated domain"
            )
            return done(nil)
        }
        Self.log.info("a sign-in session opens, to come back on \(String(describing: returning), privacy: .public)")
        let handle = opener.open(url, returning: returning, ephemeral: ephemeral) { [weak self] returned in
            guard let self, self.serial == mine, self.ended != mine else { return }
            self.ended = mine
            self.open = nil
            done(returned)
        }
        // It may have ended already, or never opened.
        guard ended != mine else { return }
        guard let handle else {
            Self.log.error("the system would not open a sign-in session")
            ended = mine
            return done(nil)
        }
        open = handle
    }

    /// The last session that has ended, by its number.
    private var ended = 0
}

/// `ASWebAuthenticationSession` over the app's key window.
@MainActor
final class SystemSignInOpener: NSObject, SignInOpener {
    func open(_ url: URL, returning: SignInReturn, ephemeral: Bool, done: @escaping @MainActor (URL?) -> Void)
        -> (any SignInHandle)?
    {
        let ended = Ended()
        let session = ASWebAuthenticationSession(url: url, callback: returning.system) { returned, _ in
            // Closed by the person, refused or failed: nobody signed in.
            Task { @MainActor in
                ended.handle?.closed()
                done(returned)
            }
        }
        session.prefersEphemeralWebBrowserSession = ephemeral
        let handle = Handle(session)
        ended.handle = handle
        #if canImport(UIKit)
        // The system opens a session only over an app in front: one asked
        // for as the app starts (a link it was opened with) waits for that.
        if UIApplication.shared.applicationState != .active {
            handle.waiting = Task { @MainActor [weak handle] in
                if UIApplication.shared.applicationState != .active {
                    for await _ in NotificationCenter.default.notifications(named: UIApplication.didBecomeActiveNotification) {
                        break
                    }
                }
                guard let handle, !Task.isCancelled else { return }
                handle.waiting = nil
                if !handle.start() {
                    SignInSession.log.error("the system would not open a sign-in session")
                    done(nil)
                }
            }
            return handle
        }
        #endif
        guard handle.start() else { return nil }
        return handle
    }

    /// The handle a session's end reaches, once there is one.
    @MainActor
    final class Ended {
        weak var handle: Handle?
    }

    final class Handle: SignInHandle {
        let session: ASWebAuthenticationSession
        /// Waiting for the app to come to the front before it opens.
        var waiting: Task<Void, Never>?
        /// The window its sheet stands over, kept here while the session is
        /// open: the session holds it weakly.
        private var anchor: SheetAnchor?

        init(_ session: ASWebAuthenticationSession) {
            self.session = session
        }

        /// Whether its sheet is up, counted among the system's sheets.
        private var up = false

        /// Opens the session over the app's window; false where the app has
        /// no window yet or the system will not open it.
        @MainActor
        func start() -> Bool {
            guard let window = Windows.anchor else { return false }
            let anchor = SheetAnchor(window)
            self.anchor = anchor
            session.presentationContextProvider = anchor
            guard session.start() else { return false }
            up = true
            PresenceModel.shared.systemSheet(opened: true)
            return true
        }

        /// Its sheet went, however it ended.
        @MainActor
        func closed() {
            guard up else { return }
            up = false
            PresenceModel.shared.systemSheet(opened: false)
        }

        func cancel() {
            waiting?.cancel()
            waiting = nil
            session.cancel()
            MainActor.assumeIsolated { closed() }
        }
    }
}

/// The app's windows, for the system's sheets that need one to stand over.
@MainActor
enum Windows {
    #if canImport(UIKit)
    static var key: UIWindow? {
        let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows)
        return windows.first(where: \.isKeyWindow) ?? windows.first
    }

    /// The view controller at the top of the key window: what a sheet is presented from.
    static var top: UIViewController? {
        var top = key?.rootViewController
        while let presented = top?.presentedViewController { top = presented }
        return top
    }

    static var scene: UIWindowScene? {
        key?.windowScene
    }

    /// What a system sheet stands over: the key window, else a window in the
    /// app's first scene; none before the app has a scene to show it in.
    static var anchor: UIWindow? {
        if let key { return key }
        let scene = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
        return scene.map { UIWindow(windowScene: $0) }
    }
    #else
    static var key: NSWindow? {
        NSApplication.shared.keyWindow ?? NSApplication.shared.windows.first
    }

    /// What a system sheet stands over: the window holding the app its item
    /// in the menu bar holds (opened for the sheet), else the key window, else
    /// a window of its own.
    static var anchor: NSWindow? {
        PresenceModel.shared.windowForSystemSheets() ?? key ?? NSWindow()
    }
    #endif
}

/// The window one system sheet stands over, chosen as the sheet opens: a
/// session or a controller asks its provider for it, and holds the provider
/// weakly.
@MainActor
final class SheetAnchor: NSObject, ASWebAuthenticationPresentationContextProviding,
    ASAuthorizationControllerPresentationContextProviding
{
    let window: ASPresentationAnchor

    init(_ window: ASPresentationAnchor) {
        self.window = window
    }

    nonisolated func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        MainActor.assumeIsolated { window }
    }

    nonisolated func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor {
        MainActor.assumeIsolated { window }
    }
}
