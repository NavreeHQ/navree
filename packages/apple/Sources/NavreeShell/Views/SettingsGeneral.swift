import NavreeDesign
import SwiftUI

/// A pane of the Settings window.
public enum SettingsPane: String, Sendable, Equatable, CaseIterable, Identifiable {
    /// What the app offers on this device: where it stands, opening at login.
    case general
    /// The server's settings screen.
    case server

    public var id: String { rawValue }
}

extension SettingsRules {
    /// The Settings window's panes, in order: the server's settings screen.
    public static func panes(presence: Presence, hasServerSettings: Bool) -> [SettingsPane] {
        [.server]
    }
}

/// The General pane: what the app offers on this device.
struct GeneralSettingsPane: View {
    let configuration: ShellConfiguration

    var body: some View {
        EmptyView()
    }
}
