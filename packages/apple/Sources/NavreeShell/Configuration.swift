import Foundation
import NavreeProtocol

/// What an app built on the shell is compiled with. The origin is part of the
/// app, never something a server or a link can change.
public struct ShellConfiguration: Sendable {
    /// The origin pattern every request, page and link must be on.
    public var origin: Origin
    /// The host the app talks to now: on the origin.
    public var base: URL
    /// The app's version, sent in the descriptor.
    public var appVersion: String
    /// The app's own URL scheme, for links from other apps.
    public var urlScheme: String?
    /// The device features this build may use, sent in the descriptor.
    public var caps: [String]
    /// Addresses that start a sign-in with another service (`*` is one
    /// segment): they open in the system's sign-in session, never in the web
    /// view, since providers refuse embedded browsers.
    public var signInPaths: [String]
    /// The address on the origin the sign-in session ends at. Its query carries
    /// what the server needs to finish signing in this client.
    public var signInCallbackPath: String
    /// Whether the session is kept in the Keychain between launches.
    public var keepsSession: Bool
    /// The session configuration the transport starts from; a demo or a test
    /// answers requests itself through its `protocolClasses`.
    public var sessionConfiguration: @Sendable () -> URLSessionConfiguration
    /// Where the web view gets its pages: the network, or (for a demo with no
    /// server) markup the app supplies for an address.
    public var webContent: WebContent
    /// The destination shown first, and the screens already pushed on each
    /// destination's stack: what a relaunch restores.
    public var restoration: Restoration

    public init(
        origin: Origin, base: URL, appVersion: String, urlScheme: String? = nil, caps: [String] = [],
        signInPaths: [String] = ["/auth/*/redirect"], signInCallbackPath: String = "/_navree/native/signed-in",
        keepsSession: Bool = true, sessionConfiguration: @escaping @Sendable () -> URLSessionConfiguration = { .ephemeral },
        webContent: WebContent = .network, restoration: Restoration = Restoration()
    ) {
        self.origin = origin
        self.base = base
        self.appVersion = appVersion
        self.urlScheme = urlScheme
        self.caps = caps
        self.signInPaths = signInPaths
        self.signInCallbackPath = signInCallbackPath
        self.keepsSession = keepsSession
        self.sessionConfiguration = sessionConfiguration
        self.webContent = webContent
        self.restoration = restoration
    }
}

/// Where a web page's markup comes from.
public enum WebContent: Sendable {
    /// The origin, over the network, with the session.
    case network
    /// Markup the app supplies for an address (a demo with recorded answers):
    /// its scripts and styles still come from the origin.
    case supplied(@Sendable (String) async -> String?)
}

/// The navigation a launch starts with.
public struct Restoration: Sendable, Equatable {
    /// The destination shown first, by its address.
    public var selected: String?
    /// For each destination, the addresses pushed over its first screen.
    public var stacks: [String: [String]]
    /// An address presented in a sheet over everything.
    public var sheet: String?

    public init(selected: String? = nil, stacks: [String: [String]] = [:], sheet: String? = nil) {
        self.selected = selected
        self.stacks = stacks
        self.sheet = sheet
    }
}
