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

/// The General pane: what the app offers on this device. On a Mac, where
/// it stands (a radio group of the offered places) and opening at login (a
/// checkbox the person alone turns on, with System Settings a click away when
/// the system waits for their approval there).
struct GeneralSettingsPane: View {
    let configuration: ShellConfiguration

    var body: some View {
        #if os(macOS)
        GeneralForm(presence: configuration.presence)
        #else
        EmptyView()
        #endif
    }
}

#if os(macOS)
struct GeneralForm: View {
    let presence: Presence
    /// As the system says each time the pane shows: the app keeps nothing of it.
    @State private var login: LoginStatus = .notRegistered
    /// The system's reason when it did not do what the person asked.
    @State private var problem: String?

    var body: some View {
        let model = PresenceModel.shared
        let choices = PresenceRules.choices(presence)
        let app = PresenceModel.appName
        Form {
            if !choices.isEmpty {
                Section {
                    Picker(String(localized: "Show \(app) in:"), selection: Binding(get: { model.place }, set: { model.choose($0) })) {
                        ForEach(choices, id: \.self) { place in
                            Text(Self.words(place)).tag(place)
                        }
                    }
                    .pickerStyle(.radioGroup)
                } footer: {
                    if presence.dockWhileWindowOpen, choices.contains(.menuBar) {
                        Text("In the menu bar only, \(app) is in the Dock while one of its windows is open.")
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: 280, alignment: .leading)
                    }
                }
            }
            if let row = LoginItemRules.row(status: login, offered: presence.offersOpenAtLogin) {
                Section {
                    Toggle(String(localized: "Open at login"), isOn: Binding(get: { row.on }, set: { on in
                        problem = LoginItem.set(on)
                        login = LoginItem.status
                    }))
                    .toggleStyle(.checkbox)
                    if row.needsApproval {
                        Button(String(localized: "Allow in System Settings…")) { LoginItem.openSystemSettings() }
                    }
                } footer: {
                    if let problem {
                        Text(problem)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: 280, alignment: .leading)
                    }
                }
            }
        }
        .padding(20)
        .onAppear { login = LoginItem.status }
    }

    static func words(_ place: MacPlace) -> String {
        switch place {
        case .dock: String(localized: "Dock")
        case .menuBar: String(localized: "Menu bar")
        case .both: String(localized: "Dock and menu bar")
        }
    }
}
#endif
