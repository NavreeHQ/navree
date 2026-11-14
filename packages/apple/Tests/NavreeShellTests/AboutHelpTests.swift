import Foundation
import Testing

@testable import NavreeShell

@Test func anAboutWithNothingAddedIsTheSystemsPanel() {
    #expect(AboutRules.options(About()).isSystems)
    #expect(AboutRules.options(About(credits: "  ", copyright: "")).isSystems)
}

@Test func creditsKeepTheirLinks() throws {
    let panel = AboutRules.options(About(credits: "Made in Ludhiana by [Navree](https://navree.example)."))
    let credits = try #require(panel.credits)
    #expect(String(credits.characters) == "Made in Ludhiana by Navree.")
    let links = credits.runs.compactMap(\.link)
    #expect(links == [URL(string: "https://navree.example")!])
    #expect(!panel.isSystems)
}

@Test func theCopyrightIsGivenOnlyWhenSet() {
    #expect(AboutRules.options(About()).copyright == nil)
    #expect(AboutRules.options(About(copyright: "© 2026 Charkha Traders, Amritsar")).copyright == "© 2026 Charkha Traders, Amritsar")
}

@Test func theSystemsHelpItemStaysUnlessTheAppSaysOtherwise() {
    #expect(HelpRules.item(.system, appName: "Charkha") == nil)
}

@Test func aHelpPageOpensInTheApp() {
    #expect(HelpRules.item(.page("/help"), appName: "Charkha") == .page(title: "Charkha Help", href: "/help"))
    // Never another site's page inside the app.
    #expect(HelpRules.item(.page("https://elsewhere.example/help"), appName: "Charkha") == nil)
}

@Test func aHelpSiteOpensInTheBrowser() {
    let site = URL(string: "https://help.charkha.example/")!
    #expect(HelpRules.item(.site(site), appName: "Charkha") == .site(title: "Charkha Help", url: site))
    #expect(HelpRules.item(.site(URL(string: "http://help.charkha.example/")!), appName: "Charkha") == nil)
}

@Test func hiddenHelpLeavesOnlyTheSearchField() {
    #expect(HelpRules.item(.hidden, appName: "Charkha") == HelpItem.none)
}
