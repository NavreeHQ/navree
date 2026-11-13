import NavreeProtocol
import SwiftUI

// A card that grows into the screen its tap opens, and the screen that
// shrinks back into it when it is left: SwiftUI's zoom navigation transition.
// The card is the source (`matchedTransitionSource`), the screen the runtime
// pushed or presented from it is the destination (`navigationTransition`),
// both matched by the card's id in one namespace. The shell owns the
// namespace (one for each navigation stack) and knows which part a screen was
// opened from (`screens[n].from`); the drawing does the rest.

extension EnvironmentValues {
    /// The namespace a navigation stack matches its zoom transitions in. The
    /// shell sets it on each stack, so its root screen and every screen pushed
    /// or presented on it see it. With none, no card is a source and no
    /// screen grows out of one.
    @Entry public var navreeZoomSpace: Namespace.ID? = nil
}

extension View {
    /// This screen grows out of the card it was opened from and shrinks back
    /// into it when it is left (a push or a sheet). `source` is what
    /// ``ZoomRules/source(from:under:reduceMotion:)`` says for the screen;
    /// with no source or no namespace the screen comes in as the system
    /// brings any screen.
    @ViewBuilder
    public func navreeZoom(from source: String?, in namespace: Namespace.ID?) -> some View {
        #if os(iOS)
        if let source, let namespace {
            navigationTransition(.zoom(sourceID: source, in: namespace))
        } else {
            self
        }
        #else
        self
        #endif
    }
}

/// The card's side of the zoom: the card is matched by its id, drawn with its
/// own ground and corner as it grows.
struct ZoomSource: ViewModifier {
    let id: String?
    let space: Namespace.ID?
    let ground: Color

    func body(content: Content) -> some View {
        #if os(iOS)
        if let id, let space {
            content.matchedTransitionSource(id: id, in: space) { source in
                source
                    .background(ground)
                    .clipShape(RoundedRectangle(cornerRadius: CompositionRules.cardCorner, style: .continuous))
            }
        } else {
            content
        }
        #else
        content
        #endif
    }
}
