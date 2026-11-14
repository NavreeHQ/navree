import Foundation

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
