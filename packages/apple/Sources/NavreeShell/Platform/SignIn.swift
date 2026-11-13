import AuthenticationServices
import Foundation

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// The system's sign-in session, never the app's web view: providers refuse
/// embedded browsers, and the system's session shows the person the real
/// address and their saved passwords.
///
/// The runtime core gives the address to open, with the hand-over's challenge
/// and state already in it, and where the session comes back: a link of the
/// origin (only this session sees it, so no other app can catch it, but the
/// build needs the origin's associated domain for it) or the app's own scheme.
/// The client opens exactly that, and hands back the whole address it came
/// back on, or nothing when the person closed it. It never treats a return as
/// a sign-in itself.
@MainActor
final class SignInSession: NSObject, ASWebAuthenticationPresentationContextProviding {
    private var session: ASWebAuthenticationSession?

    /// The callback a session comes back on, read from the core's words: an
    /// `https` link by its host and path, or a scheme.
    static func callback(_ text: String?) -> ASWebAuthenticationSession.Callback? {
        guard let text, !text.isEmpty else { return nil }
        if let url = URL(string: text), url.scheme?.lowercased() == "https", let host = url.host {
            return .https(host: host, path: url.path.isEmpty ? "/" : url.path)
        }
        let scheme = text.split(separator: ":").first.map(String.init) ?? text
        return .customScheme(scheme)
    }

    /// Opens a session at `url`; `done` gets the whole address it came back
    /// on, or `nil`. A session already open is closed first: the newer one
    /// takes its place.
    func start(_ url: URL, callback: String?, ephemeral: Bool, done: @escaping @MainActor (URL?) -> Void) {
        session?.cancel()
        session = nil
        guard let callback = Self.callback(callback) else { return done(nil) }
        let session = ASWebAuthenticationSession(url: url, callback: callback) { returned, _ in
            Task { @MainActor in done(returned) }
        }
        session.presentationContextProvider = self
        session.prefersEphemeralWebBrowserSession = ephemeral
        self.session = session
        if !session.start() {
            self.session = nil
            done(nil)
        }
    }

    nonisolated func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        MainActor.assumeIsolated { Windows.key ?? ASPresentationAnchor() }
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
    #else
    static var key: NSWindow? {
        NSApplication.shared.keyWindow ?? NSApplication.shared.windows.first
    }
    #endif
}
