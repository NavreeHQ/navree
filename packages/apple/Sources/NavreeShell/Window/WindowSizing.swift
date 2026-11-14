import SwiftUI

#if os(macOS)
import AppKit
#endif

/// A main window's smallest size and its toolbar's labels, as the app's
/// window look says. The smallest size is what the window's content asks of
/// the window, which the system keeps to. With nothing chosen the window is
/// the system's.
struct WindowSizing: ViewModifier {
    let look: WindowLook

    func body(content: Content) -> some View {
        #if os(macOS)
        content
            .modifier(MinimumSize(size: look.minimumSize))
            .modifier(OptionalKeeper(look: look))
        #else
        content
        #endif
    }
}

#if os(macOS)
/// The smallest the window's content may become, where the app says.
struct MinimumSize: ViewModifier {
    let size: CGSize?

    func body(content: Content) -> some View {
        if let size {
            content.frame(minWidth: size.width, maxWidth: .infinity, minHeight: size.height, maxHeight: .infinity)
        } else {
            content
        }
    }
}

/// The toolbar labels keeper, only where the app chose labels.
struct OptionalKeeper: ViewModifier {
    let look: WindowLook

    func body(content: Content) -> some View {
        if let mode = WindowKeeper.mode(look.toolbarLabels) {
            content.background(WindowKeeper(mode: mode))
        } else {
            content
        }
    }
}

/// Gives each toolbar the window is given the labels the app chose, as the
/// toolbar's display mode. The person may still choose other labels from the
/// toolbar's own menu (Icon and Text, Icon Only, Text Only), and AppKit shows
/// symbols where an item cannot stand as words alone.
struct WindowKeeper: NSViewRepresentable {
    let mode: NSToolbar.DisplayMode

    func makeNSView(context: Context) -> KeeperView {
        KeeperView(mode: mode)
    }

    func updateNSView(_ view: KeeperView, context: Context) {}

    /// The toolbar's display mode of the same name; none for the system's own.
    nonisolated static func mode(_ labels: ToolbarLabels) -> NSToolbar.DisplayMode? {
        switch labels {
        case .automatic: nil
        case .iconOnly: .iconOnly
        case .titleAndIcon: .iconAndLabel
        case .titleOnly: .labelOnly
        }
    }

    final class KeeperView: NSView {
        let mode: NSToolbar.DisplayMode
        private var toolbarWatch: NSKeyValueObservation?

        init(mode: NSToolbar.DisplayMode) {
            self.mode = mode
            super.init(frame: .zero)
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) {
            fatalError("init(coder:) is not used")
        }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            // A new toolbar takes the app's labels; the person's choice in a
            // toolbar stands until the window is given another.
            let mode = mode
            toolbarWatch = window?.observe(\.toolbar, options: [.initial, .new]) { window, _ in
                MainActor.assumeIsolated { window.toolbar?.displayMode = mode }
            }
        }
    }
}
#endif
