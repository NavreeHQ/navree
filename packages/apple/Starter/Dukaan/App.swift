import NavreeShell
import SwiftUI

/// Dukaan: the app's screens, drawn natively from its server (the
/// Info.plist's `NavreeOrigin`, http://127.0.0.1:8095).
@main
struct DukaanApp: App {
    #if os(macOS)
    /// What the system tells the app outside its windows, passed on to the client.
    @NSApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
    #else
    /// The push token and a tapped notification, passed on to the client.
    @UIApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
    #endif

    private let configuration: ShellConfiguration = {
        let info = Bundle.main.infoDictionary ?? [:]
        let origin = info["NavreeOrigin"] as? String ?? "http://127.0.0.1:8095"
        return ShellConfiguration(
            origin: origin,
            loopbackHTTP: origin.hasPrefix("http://"),
            appVersion: info["CFBundleShortVersionString"] as? String ?? "1.0.0",
            signInScheme: "dukaan"
        )
    }()

    var body: some Scene {
        NavreeScenes(configuration: configuration)
    }
}
