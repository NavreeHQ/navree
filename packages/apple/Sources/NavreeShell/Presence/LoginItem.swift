import Foundation
import os

#if os(macOS)
import ServiceManagement
#endif

/// Whether the app opens when the person logs in, as the system holds it
/// (`SMAppService.Status`).
public enum LoginStatus: String, Sendable, Equatable {
    case notRegistered
    /// It opens at login.
    case enabled
    /// Registered, and waiting for the person to allow it in System Settings.
    case requiresApproval
    /// The system cannot find the app to register.
    case notFound
}

/// The General pane's row for opening at login.
public struct LoginRow: Sendable, Equatable {
    /// Whether its checkbox is on.
    public var on: Bool
    /// Whether it offers "Allow in System Settings…", for an item the
    /// person has yet to allow there.
    public var needsApproval: Bool
}

/// Opening at login, as plain rules. The person turns it on; the app never
/// does, and keeps nothing of it: the system holds the state.
public enum LoginItemRules {
    /// The row: only when the app offers it, as the system says it stands.
    public static func row(status: LoginStatus, offered: Bool) -> LoginRow? {
        guard offered else { return nil }
        switch status {
        case .enabled: return LoginRow(on: true, needsApproval: false)
        case .requiresApproval: return LoginRow(on: true, needsApproval: true)
        case .notRegistered, .notFound: return LoginRow(on: false, needsApproval: false)
        }
    }
}

#if os(macOS)
/// The app as a login item, as the system holds it (`SMAppService.mainApp`).
/// Turned on only by the person, from Settings.
@MainActor
public enum LoginItem {
    static let log = Logger(subsystem: "com.navree.native", category: "presence")

    /// Whether the app opens at login, as the system says now.
    public static var status: LoginStatus {
        switch SMAppService.mainApp.status {
        case .enabled: .enabled
        case .requiresApproval: .requiresApproval
        case .notFound: .notFound
        case .notRegistered: .notRegistered
        @unknown default: .notRegistered
        }
    }

    /// Turns opening at login on or off, as the person asked: nil when the
    /// system did it, else its reason in its own words.
    @discardableResult
    public static func set(_ on: Bool) -> String? {
        do {
            if on { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
            return nil
        } catch {
            log.error("the login item was not \(on ? "added" : "removed", privacy: .public): \(error.localizedDescription, privacy: .public)")
            return error.localizedDescription
        }
    }

    /// System Settings > General > Login Items, where the person allows it.
    public static func openSystemSettings() {
        SMAppService.openSystemSettingsLoginItems()
    }
}
#endif
