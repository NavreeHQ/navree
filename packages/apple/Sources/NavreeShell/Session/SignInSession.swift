import AuthenticationServices
import Foundation

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// Sign-in with another service (Apple, Google, a company's own) through the
/// system's sign-in session, never the app's web view: providers refuse
/// embedded browsers, and the system session shows the person the real
/// address and their saved passwords.
///
/// The session ends at an https address on the app's own origin; only this
/// session sees it, so no other app can catch the callback as it could a
/// custom scheme.
@MainActor
final class SignInSession: NSObject, ASWebAuthenticationPresentationContextProviding {
    private var session: ASWebAuthenticationSession?

    func start(_ url: URL, host: String, path: String, done: @escaping @MainActor @Sendable (URL?) -> Void) {
        let session = ASWebAuthenticationSession(url: url, callback: .https(host: host, path: path)) { callback, _ in
            Task { @MainActor in done(callback) }
        }
        session.presentationContextProvider = self
        session.prefersEphemeralWebBrowserSession = false
        self.session = session
        if !session.start() {
            done(nil)
        }
    }

    nonisolated func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        MainActor.assumeIsolated {
            #if canImport(UIKit)
            let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows)
            return windows.first(where: \.isKeyWindow) ?? windows.first ?? ASPresentationAnchor()
            #else
            return NSApplication.shared.keyWindow ?? ASPresentationAnchor()
            #endif
        }
    }
}
