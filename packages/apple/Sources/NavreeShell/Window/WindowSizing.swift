import SwiftUI

/// A main window's sizes, as the app's window look says. With no sizes the
/// window takes its content's own.
struct WindowSizing: ViewModifier {
    let look: WindowLook

    func body(content: Content) -> some View {
        content
    }
}
