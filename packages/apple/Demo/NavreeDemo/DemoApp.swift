import Foundation
import NavreeShell
import SwiftUI

/// The store demo as a native app, on the runtime core. Its origin is
/// configuration: the Info.plist key `NavreeOrigin` (a store demo running on
/// this Mac by default), or the site a team's signing associates it with; or
/// the store's answers kept in the app, when launched with
/// `-NavreeRecorded YES`; or the answers in a folder outside the app, when
/// launched with `-NavreeAnswers <folder>` (or `NAVREE_ANSWERS` in the
/// environment), so it runs with no server at all.
@main
enum DemoMain {
    /// Starts the demo as its launch arguments ask.
    static func main() {
        DemoStyles.run()
    }
}

/// The demo's scenes: its main windows and, on a Mac, its Settings window and
/// its item in the menu bar.
struct DemoApp: App {
    #if os(macOS)
    /// What the system tells the app outside its windows, passed on to the shell.
    @NSApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
    #else
    /// The push token and a tapped notification, passed on to the shell.
    @UIApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
    #endif
    private let configuration = DemoConfiguration.make()

    var body: some Scene {
        NavreeScenes(configuration: configuration) { $0.modifier(DemoPictures()) }
    }
}

#if !os(macOS)
/// Pictures of the app's windows are taken on a Mac only; the iPhone and iPad
/// screens are pictured by the UI tests.
struct DemoPictures: ViewModifier {
    func body(content: Content) -> some View { content }
}
#endif

enum DemoConfiguration {
    nonisolated static let recordedOrigin = "https://charkha.example"

    static func make() -> ShellConfiguration {
        let defaults = UserDefaults.standard
        var configuration = source(defaults)
        configuration.presence = DemoPresence.make(defaults)
        configuration.windows = DemoWindows.make(defaults)
        configuration.icons = DemoWindows.icons(defaults)
        return configuration
    }

    /// Where the demo's answers come from: a folder, the answers kept in the
    /// app, or a server.
    static func source(_ defaults: UserDefaults) -> ShellConfiguration {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.1.0"
        let launch = Self.launch(defaults)
        if let folder = defaults.string(forKey: "NavreeAnswers") ?? ProcessInfo.processInfo.environment["NAVREE_ANSWERS"],
            !folder.isEmpty
        {
            let url = URL(fileURLWithPath: (folder as NSString).expandingTildeInPath, isDirectory: true)
            return ShellConfiguration(
                origin: AnswerFolder.origin(of: url) ?? "https://answers.example", appVersion: version,
                keepsSession: false,
                transport: { AnswerFolder(folder: url) ?? AnswerFolder.empty() },
                launch: launch
            )
        }
        if defaults.bool(forKey: "NavreeRecorded") {
            return ShellConfiguration(
                origin: recordedOrigin, appVersion: version, signInScheme: scheme, keepsSession: false,
                transport: { RecordedStore() },
                webContent: .supplied({ address in RecordedStore.page(for: address) }),
                launch: launch
            )
        }
        // A launch argument is read as a property list, where `//` starts a
        // comment: give it quoted (`-NavreeOrigin '"http://127.0.0.1:8096"'`)
        // or as `NAVREE_ORIGIN` in the environment.
        let origin = ProcessInfo.processInfo.environment["NAVREE_ORIGIN"].flatMap { $0.isEmpty ? nil : $0 }
            ?? defaults.string(forKey: "NavreeOrigin")
            ?? associated.map { "https://\($0)" }
            ?? Bundle.main.object(forInfoDictionaryKey: "NavreeOrigin") as? String
            ?? "http://127.0.0.1:8095"
        return ShellConfiguration(
            origin: origin, loopbackHTTP: origin.hasPrefix("http://"), appVersion: version, signInScheme: scheme,
            signInPaths: signInPaths(defaults), launch: launch
        )
    }

    /// The scheme a sign-in comes back on: the build's own, from its
    /// Info.plist, so the Mac demo and the menu-bar demo come back apart.
    static var scheme: String {
        let types = Bundle.main.object(forInfoDictionaryKey: "CFBundleURLTypes") as? [[String: Any]]
        return (types?.first?["CFBundleURLSchemes"] as? [String])?.first ?? "charkha"
    }

    /// The site a team's signing associates the build with, if any.
    static var associated: String? {
        let domain = (Bundle.main.object(forInfoDictionaryKey: Entitlement.domainKey) as? String)?
            .split(separator: "?").first.map(String.init)
        return domain?.isEmpty == false ? domain : nil
    }

    /// The addresses that open the system's sign-in session instead of a
    /// page: `-NavreeSignInPaths "/sign-in;/auth/*/redirect"` for a server
    /// whose sign-in is handed over to the app; else only another site's.
    static func signInPaths(_ defaults: UserDefaults) -> [String] {
        let paths = defaults.string(forKey: "NavreeSignInPaths")?.split(separator: ";").map(String.init) ?? []
        return paths.isEmpty ? ["/auth/*/redirect"] : paths
    }

    /// `-NavreeSelect /orders -NavreeStack "/orders=/orders/1" -NavreeSheet /orders/new
    /// -NavreeTap "CS-220480;Refund"`: the screens a launch opens and the
    /// controls it taps by their words, as the person would.
    static func launch(_ defaults: UserDefaults) -> Launch {
        var select = defaults.string(forKey: "NavreeSelect")
        var push: [String] = []
        if let stack = defaults.string(forKey: "NavreeStack") {
            for entry in stack.split(separator: ";") {
                let parts = entry.split(separator: "=", maxSplits: 1).map(String.init)
                guard parts.count == 2, select == nil || select == parts[0] else { continue }
                select = parts[0]
                push = parts[1].split(separator: ",").map(String.init)
                break
            }
        }
        let taps = defaults.string(forKey: "NavreeTap")?.split(separator: ";").map(String.init) ?? []
        return Launch(
            select: select, push: push, sheet: defaults.string(forKey: "NavreeSheet"), taps: taps,
            editing: defaults.bool(forKey: "NavreeEditing")
        )
    }
}
