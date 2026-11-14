import Foundation
import NavreeShell

/// Where the demo stands in the system outside its windows, as its launch
/// arguments say: by default an ordinary app in the Dock.
enum DemoPresence {
    static func make(_ defaults: UserDefaults) -> Presence {
        Presence()
    }
}
