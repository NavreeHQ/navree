import Foundation

/// The Mac demo's picture steps about where the app stands: none yet, so
/// every step is left to the others.
@MainActor
enum PresenceSteps {
    /// The lines the step writes to the log, or nil for a step it does not know.
    static func run(_ step: String, _ argument: String, out: URL) async -> [String]? {
        nil
    }
}
