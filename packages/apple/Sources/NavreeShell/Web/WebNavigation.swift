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
/// core is told only when the system reports `WKNavigationType.linkActivated`
/// AND the web view recently received real person input (`userInteracted`),
/// never when a page's own script synthesises a click on a link. Every other
/// navigation (`other`, a script's own link click, a redirect, a form sent, a
/// reload, back and forward) may stay inside the app's own origin and may not
/// leave it: one that would is cancelled, and told to nobody.
public enum WebNavigationRules {
    /// What becomes of a navigation.
    ///
    /// - `linkActivated`: the system's type for it is an activated link.
    /// - `mainFrame`: it would replace the page itself, not a frame inside it.
    /// - `ours`: its address is the app's own, whole; for a frame, also one
    ///   of a host the app names for the web view (``WebRules``).
    /// - `loaded`: its address is the one the app itself asked the web view to load.
    /// - `empty`: its address is `about:`, an empty frame.
    /// - `userInteracted`: the web view recently saw a real touch, click,
    ///   key press or accessibility activation from the person.
    public static func decide(
        linkActivated: Bool, mainFrame: Bool, ours: Bool, loaded: Bool = false, empty: Bool = false,
        userInteracted: Bool = true
    ) -> WebNavigation {
        guard mainFrame else {
            // A frame stays where the app lets a frame be, or empty; it never takes the person anywhere.
            return ours || empty ? .allow : .cancel
        }
        // The page the app asked for, and its own reload.
        if loaded && (!linkActivated || !userInteracted) { return .allow }
        // A link the person actually tapped: the core decides, whatever it leads to.
        if linkActivated && userInteracted { return .tap }
        // A form posted, a redirect, the page's script (including `anchor.click()`):
        // inside the origin it stays in the web view; out of it, it goes nowhere.
        return ours ? .allow : .cancel
    }

    /// What becomes of a page's request for a new window: a link the person
    /// actually activated is a tap for the core to follow; a window or
    /// synthetic click a script asks for is not opened and nobody is told.
    /// No new web view is ever made.
    public static func newWindow(linkActivated: Bool, userInteracted: Bool = true) -> WebNavigation {
        linkActivated && userInteracted ? .tap : .cancel
    }
}
