import Foundation
import NavreeDesign
import NavreeProtocol
import NavreeRuntime

/// What an app built on the shell is compiled with. The origin is part of the
/// app, never something a server or a link can change: the runtime core holds
/// it, and every address any part of the app asks for is checked there.
public struct ShellConfiguration: Sendable {
    /// The one origin the app talks to, such as `https://store.example.in`.
    public var origin: String
    /// Plain `http` to this machine: for a server run locally in development only.
    public var loopbackHTTP: Bool
    /// The bundle id: without it no sign-in is handed over and no push token posted.
    public var appID: String
    /// The app's version, sent in the descriptor.
    public var appVersion: String
    /// The app's own URL scheme, when a sign-in session is to come back on it
    /// rather than on a link of the origin.
    public var signInScheme: String?
    /// Addresses that open the system's sign-in session instead of a page
    /// (`*` is one segment).
    public var signInPaths: [String]
    /// What the build is signed with beyond what any app has: it decides which
    /// device features the app says it has.
    public var signedWith: Set<Entitlement>
    /// Whether a link from outside may open any address of the origin, or only
    /// a destination and the addresses the shell's routes name.
    public var openAnyLink: Bool
    /// Whether the session's cookies are kept in the Keychain between launches.
    public var keepsSession: Bool
    /// Hosts other than the origin's that pictures may come from, over
    /// `https`, each named exactly (`images.store.example.in`). None by
    /// default. A picture from one of them is asked for with no cookie.
    public var pictureHosts: [String]
    /// What answers the core's requests: the network, or (for a demo or a
    /// test) answers the app has with it.
    public var transport: @MainActor @Sendable () -> any Transport
    /// Where the web view gets its pages: the network, or markup the app supplies.
    public var webContent: WebContent
    /// Screens opened as the app starts, as a person would open them: for a
    /// demo and its tests.
    public var launch: Launch
    /// Where the app stands in the system outside its windows: the Dock and
    /// the menu bar on a Mac, the count on its icon, its quick ways in, About
    /// and Help.
    public var presence: Presence
    /// The app's main windows, as far as the system lets an app choose (Mac).
    public var windows: WindowLook
    /// The icons the person may choose among (iOS, iPadOS): the build's
    /// alternate app icons. Empty, the app offers no choice.
    public var icons: [AppIcon]

    public init(
        origin: String, loopbackHTTP: Bool = false, appID: String = Bundle.main.bundleIdentifier ?? "",
        appVersion: String, signInScheme: String? = nil, signInPaths: [String] = ["/auth/*/redirect"],
        signedWith: Set<Entitlement>? = nil, openAnyLink: Bool = false, keepsSession: Bool = true,
        pictureHosts: [String] = [],
        transport: (@MainActor @Sendable () -> any Transport)? = nil, webContent: WebContent = .network,
        launch: Launch = Launch(), presence: Presence = Presence(), windows: WindowLook = WindowLook(),
        icons: [AppIcon] = []
    ) {
        self.origin = origin
        self.loopbackHTTP = loopbackHTTP
        self.appID = appID
        self.appVersion = appVersion
        self.signInScheme = signInScheme
        self.signInPaths = signInPaths
        // Unless the app says otherwise, what its Info.plist says it is signed with.
        self.signedWith = signedWith ?? Entitlement.declared(origin: origin)
        self.openAnyLink = openAnyLink
        self.keepsSession = keepsSession
        self.pictureHosts = pictureHosts
        let keeps = keepsSession
        let loopback = loopbackHTTP
        let origin = origin
        self.transport = transport ?? {
            HTTPTransport(jar: CookieJar(origin: origin, loopbackHTTP: loopback, persistent: keeps))
        }
        self.webContent = webContent
        self.launch = launch
        self.presence = presence
        self.windows = windows
        self.icons = icons
    }

    /// The origin as a URL.
    public var base: URL? {
        URL(string: origin.hasSuffix("/") ? origin : origin + "/")
    }

    /// The origin every address is checked against: `nil` when the
    /// configured text is not one, and then no address is the app's.
    public var site: Origin? {
        Origin(origin, loopbackHTTP: loopbackHTTP)
    }
}

/// What a build can be signed with that changes what it can do. Each needs an
/// Apple Developer Program team, and most an associated domain the origin
/// serves (`apple-app-site-association`).
public enum Entitlement: String, Sendable, CaseIterable {
    /// `webcredentials:` and `applinks:` for the origin's host: passkeys, a
    /// confirmation signed with one, and a sign-in session that comes back on
    /// a link of the origin.
    case associatedDomains
    /// Sign in with Apple.
    case signInWithApple
    /// `aps-environment`: push notifications.
    case pushNotifications
}

/// Where a web page's markup comes from.
public enum WebContent: Sendable {
    /// The origin, over the network, with the session.
    case network
    /// Markup the app supplies for an address (a demo with recorded answers):
    /// its scripts and styles still come from the origin.
    case supplied(@Sendable (String) async -> String?)
}

/// What a launch opens before the person does anything, told to the core as
/// the person's own taps would be: a destination chosen, links followed.
public struct Launch: Sendable, Equatable {
    /// The destination shown first, by its address.
    public var select: String?
    /// Addresses opened one over the other on that destination's stack.
    public var push: [String]
    /// An address presented as a sheet over everything.
    public var sheet: String?
    /// Controls tapped in turn on the screen in front, by their words.
    public var taps: [String]
    /// Lists whose rows can be chosen or moved open in edit mode.
    public var editing: Bool

    public init(
        select: String? = nil, push: [String] = [], sheet: String? = nil, taps: [String] = [], editing: Bool = false
    ) {
        self.select = select
        self.push = push
        self.sheet = sheet
        self.taps = taps
        self.editing = editing
    }

    public var isEmpty: Bool {
        select == nil && push.isEmpty && sheet == nil && taps.isEmpty
    }
}

/// This client library.
public enum Library {
    public static let version = "0.1.0"
}

/// What the device and the person's settings are as the app starts; the
/// changes after that are events.
public struct DeviceTraits: Sendable, Equatable {
    public var platform: Navree.Platform
    public var os: String
    public var form: Navree.FormFactor
    public var scheme: Navree.Scheme
    public var contrast: Navree.Contrast
    public var textPercent: Int
    public var locale: String
    public var tz: String

    public init(
        platform: Navree.Platform = .ios, os: String = "", form: Navree.FormFactor = .phone,
        scheme: Navree.Scheme = .light, contrast: Navree.Contrast = .normal, textPercent: Int = 100,
        locale: String = "en-IN", tz: String = "Asia/Kolkata"
    ) {
        self.platform = platform
        self.os = os
        self.form = form
        self.scheme = scheme
        self.contrast = contrast
        self.textPercent = textPercent
        self.locale = locale
        self.tz = tz
    }
}

public enum CoreConfiguration {
    /// The device features this build carries out, by the protocol's names:
    /// a control that calls one the build does not list is replaced by what
    /// the server wrote to stand in for it, before the app sees the tree.
    ///
    /// Always: the clipboard, the share sheet, opening an address, the
    /// system's settings and its review prompt, the app's own notifications
    /// and the count on its icon. A sign-in session only when it
    /// can come back: on the app's own scheme, or on a link of the origin the
    /// build has the associated domain for. A confirmation with a passkey, Sign
    /// in with Apple and push only when the build is signed for them.
    public static func caps(signedWith: Set<Entitlement>, signInScheme: String?) -> [String] {
        var caps = ["clipboard", "local_notification", "open_settings", "open_url", "request_review", "share"]
        if signInScheme != nil || signedWith.contains(.associatedDomains) { caps.append("auth_session") }
        if signedWith.contains(.associatedDomains) { caps.append("confirm") }
        if signedWith.contains(.signInWithApple) { caps.append("platform_sign_in") }
        if signedWith.contains(.pushNotifications) { caps.append("push") }
        return caps.sorted()
    }

    /// The core's configuration for this app on this device.
    public static func json(_ configuration: ShellConfiguration, device: DeviceTraits) -> Data {
        var signIn: [String: Any] = [
            "paths": configuration.signInPaths,
            // Apple's own sheet signs in with Apple; any other provider goes the web's way.
            "native": ["apple"],
        ]
        if let scheme = configuration.signInScheme { signIn["callback"] = scheme }
        var config: [String: Any] = [
            "origin": configuration.origin.hasSuffix("/") ? String(configuration.origin.dropLast()) : configuration.origin,
            "app_id": configuration.appID,
            "client": [
                "platform": device.platform.rawValue, "os": device.os, "form": device.form.rawValue,
                "app": configuration.appVersion, "lib": Library.version, "vocab": Navree.vocab,
                "scheme": device.scheme.rawValue, "contrast": device.contrast.rawValue,
                "text_percent": device.textPercent, "locale": device.locale, "tz": device.tz,
                "caps": caps(signedWith: configuration.signedWith, signInScheme: configuration.signInScheme),
            ] as [String: Any],
            // Back on Apple's systems goes down the stack and never across to another destination.
            "back_to_first": false,
            "deep_links": configuration.openAnyLink ? "origin" : "routes",
            "cache": ["pages": 24, "persist": true] as [String: Any],
            "undrawable": undrawable,
            "views": [String](),
            "sign_in": signIn,
        ]
        if configuration.loopbackHTTP { config["loopback_http"] = true }
        return (try? JSONSerialization.data(withJSONObject: config, options: [.sortedKeys])) ?? Data()
    }
}
