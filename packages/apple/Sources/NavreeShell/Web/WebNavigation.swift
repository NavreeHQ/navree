import Foundation

/// What becomes of a navigation a page in the web view asks for.
public enum WebNavigation: Equatable, Sendable {
    /// It goes on in the web view.
    case allow
    /// It is cancelled, and the core is told the person tapped a link: the
    /// core says where a tap leads (a native screen, the system browser,
    /// the dialler).
    case tap
    /// It is cancelled, and nobody is told.
    case cancel
}

/// The web view's navigation rule. The core takes a `web_link` as a tap, and
/// a tap may open the dialler, messages, mail, maps or another site. So the
/// core is told only of what the system says the person did: a navigation
/// of the type `WKNavigationType.linkActivated`, of which Apple says "This
/// action occurs when the user activates a link with an `href` attribute".
/// Every other type (`other`, which is what a page's script and a redirect
/// are, a form sent, a reload, back and forward) may stay inside the app's
/// own origin and may not leave it: one that would is cancelled, and told to
/// nobody.
public enum WebNavigationRules {
    /// What becomes of a navigation.
    ///
    /// - `linkActivated`: the system's type for it is a link the person activated.
    /// - `mainFrame`: it would replace the page itself, not a frame inside it.
    /// - `ours`: its address is the app's own, whole.
    /// - `loaded`: its address is the one the app itself asked the web view to load.
    /// - `empty`: its address is `about:`, an empty frame.
    public static func decide(
        linkActivated: Bool, mainFrame: Bool, ours: Bool, loaded: Bool = false, empty: Bool = false
    ) -> WebNavigation {
        guard mainFrame else {
            // A frame stays the origin's own, or empty; it never takes the person anywhere.
            return ours || empty ? .allow : .cancel
        }
        // The page the app asked for, and its own reload.
        if loaded && !linkActivated { return .allow }
        // A link the person tapped: the core decides, whatever it leads to.
        if linkActivated { return .tap }
        // A form posted, a redirect, the page's script: inside the origin it
        // stays in the web view; out of it, it goes nowhere.
        return ours ? .allow : .cancel
    }

    /// What becomes of a page's request for a new window: a link the person
    /// activated is a tap for the core to follow; a window a script asks for
    /// is not opened and nobody is told. No new web view is ever made.
    public static func newWindow(linkActivated: Bool) -> WebNavigation {
        linkActivated ? .tap : .cancel
    }
}
