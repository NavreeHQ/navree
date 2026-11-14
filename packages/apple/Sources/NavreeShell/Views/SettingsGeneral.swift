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
    /// The Settings window's panes, in order: General first when the app
    /// offers something of its own on this device (a choice of place, opening
    /// at login), then the server's settings screen when it has one. With
    /// nothing of the app's own, the server's screen alone, as before.
    public static func panes(presence: Presence, hasServerSettings: Bool) -> [SettingsPane] {
        let general = !PresenceRules.choices(presence).isEmpty || presence.offersOpenAtLogin
        guard general else { return [.server] }
        return hasServerSettings ? [.general, .server] : [.general]
    }
}

/// The General pane: what the app offers on this device.
struct GeneralSettingsPane: View {
    let configuration: ShellConfiguration

    var body: some View {
        EmptyView()
    }
}
