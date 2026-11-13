import Accessibility
import Foundation
import NavreeDesign
import NavreeProtocol
import Observation
import SwiftUI
import WebKit

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// The app's client: it holds the view state, hands every event to the
/// runtime, and carries out the effects the runtime asks for (the network,
/// the system browser, the sign-in session, VoiceOver, haptics, the clipboard).
@MainActor
@Observable
public final class NativeClient {
    public private(set) var state = ViewState()
    @ObservationIgnored private var runtime: InterimRuntime
    public let configuration: ShellConfiguration
    public let transport: Transport
    public let jar: CookieJar
    let signIn = SignInSession()
    @ObservationIgnored private var started = false

    public init(configuration: ShellConfiguration) {
        self.configuration = configuration
        runtime = InterimRuntime(configuration: configuration)
        jar = CookieJar(origin: configuration.origin, persistent: configuration.keepsSession)
        transport = Transport(
            base: configuration.base, origin: configuration.origin,
            descriptor: Platform.descriptor(app: configuration.appVersion, caps: configuration.caps), jar: jar,
            configuration: configuration.sessionConfiguration()
        )
    }

    /// Starts the app once: the shell, then the first destination.
    public func start() {
        guard !started else { return }
        started = true
        send(.start)
    }

    public func send(_ event: Event) {
        let effects = runtime.send(event)
        state = runtime.state
        for effect in effects { run(effect) }
    }

    /// The values a form's field holds now, as the runtime keeps them.
    public func values(at ref: PageRef, form: FormRef, name: String) -> [String]? {
        state.page(ref)?.values[form.key]?[name]
    }

    public func presentation(of address: String) -> Navree.Present {
        runtime.presentation(of: address)
    }

    /// The context a screen's drawing reaches the runtime through.
    func context(for ref: PageRef) -> DrawingContext {
        let page = state.page(ref)
        return DrawingContext(
            send: { [weak self] intent in self?.send(.intent(intent, at: ref)) },
            values: { [weak self] form, name in self?.values(at: ref, form: form, name: name) },
            icons: page?.icons ?? [:],
            address: page?.address ?? "/",
            base: configuration.base,
            presentation: { [weak self] address in self?.presentation(of: address) ?? .push },
            depth: ref.index
        )
    }

    private func run(_ effect: Effect) {
        switch effect {
        case .fetch(let ticket, let request):
            Task { [transport] in
                let reply = await transport.send(request)
                self.send(.answered(ticket, reply))
            }
        case .openExternal(let url):
            Platform.open(url)
        case .signIn(let url):
            let callback = configuration.signInCallbackPath
            let host = configuration.base.host ?? ""
            signIn.start(url, host: host, path: callback) { [weak self] result in
                self?.send(.signedIn(result))
            }
        case .announce(let words):
            AccessibilityNotification.Announcement(words).post()
        case .haptic(let haptic):
            Platform.feel(haptic)
        case .copy(let text):
            Platform.copy(text)
        }
    }

    /// Ends the session on this device: the jar, the Keychain, the web view.
    public func forgetSession() async {
        await jar.clear(WKWebsiteDataStore.default().httpCookieStore)
    }
}

/// What the client reads from and does to the device.
@MainActor
enum Platform {
    /// The descriptor the transport sends: what this device and app are.
    static func descriptor(app: String, caps: [String]) -> ClientDescriptor {
        #if canImport(UIKit)
        let device = UIDevice.current
        let tablet = device.userInterfaceIdiom == .pad
        let traits = UITraitCollection.current
        let scheme = traits.userInterfaceStyle == .dark ? "dark" : "light"
        let contrast = traits.accessibilityContrast == .high ? "more" : "normal"
        let platform = tablet ? "ipados" : "ios"
        let os = device.systemVersion
        let form = tablet ? "tablet" : "phone"
        let text = UIFontMetrics.default.scaledValue(for: 17) / 17
        #else
        let platform = "macos"
        let version = ProcessInfo.processInfo.operatingSystemVersion
        let os = "\(version.majorVersion).\(version.minorVersion)"
        let form = "desktop"
        let scheme = NSApplication.shared.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? "dark" : "light"
        let contrast = "normal"
        let text = 1.0
        #endif
        return ClientDescriptor(
            platform: platform, os: os, form: form, app: app, lib: Library.version,
            scheme: scheme, contrast: contrast, text: Double(text),
            locale: Locale.current.identifier.replacingOccurrences(of: "_", with: "-"),
            timeZone: TimeZone.current.identifier, caps: caps
        )
    }

    static var platform: String {
        #if canImport(UIKit)
        UIDevice.current.userInterfaceIdiom == .pad ? "ipados" : "ios"
        #else
        "macos"
        #endif
    }

    static func open(_ url: URL) {
        #if canImport(UIKit)
        UIApplication.shared.open(url)
        #else
        NSWorkspace.shared.open(url)
        #endif
    }

    static func feel(_ haptic: Haptic) {
        #if canImport(UIKit)
        let generator = UINotificationFeedbackGenerator()
        switch haptic {
        case .success: generator.notificationOccurred(.success)
        case .warning: generator.notificationOccurred(.warning)
        case .error: generator.notificationOccurred(.error)
        }
        #endif
    }

    static func copy(_ text: String) {
        #if canImport(UIKit)
        UIPasteboard.general.string = text
        #else
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        #endif
    }
}

/// This client library.
public enum Library {
    public static let version = "0.1.0"
}
