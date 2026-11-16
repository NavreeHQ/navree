import Foundation

/// What a window's core is started from. The system may end the app and
/// bring a window back: the core is then given what was kept of the window,
/// so the person finds it as they left it. A start that follows the core
/// stopping is a plain one, because what was kept may be what brought the
/// stop, and giving it back would stop the core again at every start. What
/// was kept is set aside for that one start, and dropped at the start after.
enum StartRules {
    /// The key that says the window's core stopped, beside what is kept of the window.
    static func stoppedKey(_ kept: String) -> String {
        kept + "-stopped"
    }

    /// The key what was kept is set aside under, for the one start that follows a stop.
    static func asideKey(_ kept: String) -> String {
        kept + "-aside"
    }

    /// What the window's core is given at this start, or `nil` to start
    /// plainly. `restoring` says the system brings the window back; `kept`
    /// is the key what is kept of the window is written under.
    static func snapshot(restoring: Bool, kept: String, in store: KeptFiles) -> Data? {
        if store.holds(stoppedKey(kept)) {
            // The start after a stop: plain. What was kept is no longer
            // where a start takes it from.
            store.writeData(asideKey(kept), store.readData(kept))
            store.writeData(kept, nil)
            store.writeData(stoppedKey(kept), nil)
            return nil
        }
        // A start that follows no stop: what an earlier one set aside is dropped.
        store.writeData(asideKey(kept), nil)
        let snapshot = restoring ? store.readData(kept) : nil
        // A window that starts plainly lets go of what was kept of it.
        if snapshot == nil { store.writeData(kept, nil) }
        return snapshot
    }
}
