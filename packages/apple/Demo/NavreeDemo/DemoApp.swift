import Foundation
import NavreeShell
import SwiftUI

/// The store demo as a native app. Its origin is configuration: the Info.plist
/// key `NavreeOrigin` (a store demo running on this Mac by default), or
/// recorded answers when launched with `-NavreeRecorded YES`, so it runs with
/// no server at all.
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
        let recorded = defaults.bool(forKey: "NavreeRecorded")
        let text = recorded
            ? recordedOrigin
            : defaults.string(forKey: "NavreeOrigin")
                ?? Bundle.main.object(forInfoDictionaryKey: "NavreeOrigin") as? String
                ?? "http://127.0.0.1:8095"
        guard let origin = Origin(text), let base = origin.url else {
            fatalError("NavreeOrigin must be one https origin, or http on this machine: \(text)")
        }
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.1.0"
        return ShellConfiguration(
            origin: origin,
            base: base,
            appVersion: version,
            urlScheme: "charkha",
            caps: ["share", "haptic"],
            keepsSession: !recorded,
            sessionConfiguration: {
                let configuration = URLSessionConfiguration.ephemeral
                if recorded { configuration.protocolClasses = [RecordedAnswers.self] }
                return configuration
            },
            webContent: recorded ? .supplied({ address in RecordedAnswers.page(for: address) }) : .network,
            restoration: restoration(defaults)
        )
    }

    /// `-NavreeSelect /orders -NavreeStack "/orders=/orders/1"`: the screens a
    /// launch starts on, as a relaunch would restore them.
    static func restoration(_ defaults: UserDefaults) -> Restoration {
        var stacks: [String: [String]] = [:]
        if let stack = defaults.string(forKey: "NavreeStack") {
            for entry in stack.split(separator: ";") {
                let parts = entry.split(separator: "=", maxSplits: 1).map(String.init)
                guard parts.count == 2 else { continue }
                stacks[parts[0]] = parts[1].split(separator: ",").map(String.init)
            }
        }
        return Restoration(
            selected: defaults.string(forKey: "NavreeSelect"),
            stacks: stacks,
            sheet: defaults.string(forKey: "NavreeSheet")
        )
    }
}
