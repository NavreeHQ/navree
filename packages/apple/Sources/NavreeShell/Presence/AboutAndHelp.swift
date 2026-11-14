import SwiftUI

/// App > About: what the app adds to the system's own About panel. With
/// nothing added, the system's own item stands.
struct AboutCommands: Commands {
    let configuration: ShellConfiguration?

    var body: some Commands {
        EmptyCommands()
    }
}

/// The Help menu's item, as the app says. With nothing said, the system's own
/// item stands.
struct HelpCommands: Commands {
    let configuration: ShellConfiguration?

    var body: some Commands {
        EmptyCommands()
    }
}
