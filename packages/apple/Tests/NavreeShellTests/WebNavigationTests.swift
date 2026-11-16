import Foundation
import Testing

@testable import NavreeShell

/// What becomes of a navigation in the web view: the core hears of a tap only
/// when the system says the person activated a link.
struct WebNavigationTests {
    @Test func onlyALinkThePersonActivatedIsToldToTheCore() {
        // A tapped link, to the app's own address or to anywhere else.
        #expect(WebNavigationRules.decide(linkActivated: true, mainFrame: true, ours: true) == .tap)
        #expect(WebNavigationRules.decide(linkActivated: true, mainFrame: true, ours: false) == .tap)
        // A tapped link to the address the page was loaded from is a tap too.
        #expect(WebNavigationRules.decide(linkActivated: true, mainFrame: true, ours: true, loaded: true) == .tap)
    }

    @Test func whatAPageDoesByItselfNeverLeavesTheOriginAndIsToldToNobody() {
        // `location = "tel:+919814012345"`, `sms:`, `mailto:`, another site;
        // a redirect the server answers; a form posted elsewhere: none of
        // them is the app's own address, and none is a link activated.
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: true, ours: false) == .cancel)
        // On the origin the same stays in the web view: a form posted, a
        // redirect of the page's own, its reload.
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: true, ours: true) == .allow)
        // The page the app asked for loads.
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: true, ours: true, loaded: true) == .allow)
    }

    @Test func aFrameStaysTheOriginsOrEmptyAndNeverTakesThePersonAnywhere() {
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: false, ours: true) == .allow)
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: false, ours: false, empty: true) == .allow)
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: false, ours: false) == .cancel)
        #expect(WebNavigationRules.decide(linkActivated: true, mainFrame: false, ours: false) == .cancel)
    }

    @Test func aNewWindowIsALinkOnlyWhenThePersonActivatedOne() {
        #expect(WebNavigationRules.newWindow(linkActivated: true) == .tap)
        // `window.open("sms:+919814012345")` from a script.
        #expect(WebNavigationRules.newWindow(linkActivated: false) == .cancel)
    }
}
