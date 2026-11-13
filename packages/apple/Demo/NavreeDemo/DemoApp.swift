import Foundation
import NavreeShell
import SwiftUI

/// The store demo as a native app, on the runtime core. Its origin is
/// configuration: the Info.plist key `NavreeOrigin` (a store demo running on
/// this Mac by default); or the store's answers kept in the app, when launched
/// with `-NavreeRecorded YES`; or the answers in a folder outside the app,
/// when launched with `-NavreeAnswers <folder>` (or `NAVREE_ANSWERS` in the
/// environment), so it runs with no server at all.
@main
struct DemoApp: App {
    private let configuration = DemoConfiguration.make()

    var body: some Scene {
        WindowGroup {
            AppShell(configuration: configuration)
        }
    }
}

enum DemoConfiguration {
    nonisolated static let recordedOrigin = "https://charkha.example"

    static func make() -> ShellConfiguration {
        let defaults = UserDefaults.standard
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
                origin: recordedOrigin, appVersion: version, signInScheme: "charkha", keepsSession: false,
                transport: { RecordedStore() },
                webContent: .supplied({ address in RecordedStore.page(for: address) }),
                launch: launch
            )
        }
        let origin = defaults.string(forKey: "NavreeOrigin")
            ?? Bundle.main.object(forInfoDictionaryKey: "NavreeOrigin") as? String
            ?? "http://127.0.0.1:8095"
        return ShellConfiguration(
            origin: origin, loopbackHTTP: origin.hasPrefix("http://"), appVersion: version, signInScheme: "charkha",
            launch: launch
        )
    }

    /// `-NavreeSelect /orders -NavreeStack "/orders=/orders/1" -NavreeSheet /orders/new`:
    /// the screens a launch opens, as the person would by tapping.
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
        return Launch(select: select, push: push, sheet: defaults.string(forKey: "NavreeSheet"))
    }
}
