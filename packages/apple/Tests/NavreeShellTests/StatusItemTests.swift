import NavreeProtocol
import Testing

@testable import NavreeShell

private let menu = StatusItem(image: .symbol("shippingbox"))
private let destinations = [
    PresenceDestination(label: "Orders", href: "/orders", symbol: "shippingbox", badge: "3"),
    PresenceDestination(label: "Inventory", href: "/inventory", symbol: "cube.box"),
]

@Test func theItemStandsInTheMenuBarOnlyWhenThePlaceSaysSo() {
    #expect(!StatusRules.inserted(style: .menu, item: menu, place: .dock))
    #expect(StatusRules.inserted(style: .menu, item: menu, place: .menuBar))
    #expect(StatusRules.inserted(style: .menu, item: menu, place: .both))
    #expect(!StatusRules.inserted(style: .menu, item: nil, place: .menuBar))
}

@Test func onlyTheItemOfTheChosenStyleIsInserted() {
    #expect(!StatusRules.inserted(style: .window, item: menu, place: .menuBar))
    // A small window is not built yet: such an item stands nowhere.
    let screen = StatusItem(image: .symbol("sun.max"), content: .screen("/today"))
    #expect(!StatusRules.inserted(style: .window, item: screen, place: .menuBar))
    #expect(!StatusRules.inserted(style: .menu, item: screen, place: .menuBar))
}

@Test func theMenuListsTheDestinationsThenSettingsThenQuit() {
    let entries = StatusRules.menu(destinations: destinations, appName: "Charkha", place: .both, user: nil)
    #expect(entries == [
        .destination(destinations[0]), .destination(destinations[1]), .divider, .settings, .divider, .quit("Quit Charkha"),
    ])
}

@Test func beforeTheShellComesTheMenuOffersToOpenTheApp() {
    let entries = StatusRules.menu(destinations: [], appName: "Charkha", place: .menuBar, user: nil)
    #expect(entries.first == .open("Open Charkha"))
    #expect(!entries.contains { if case .destination = $0 { true } else { false } })
}

@Test func aMenuBarOnlyAppKeepsTheAccountInItsMenu() {
    let user = Navree.User(menu: [
        .link(Navree.Link(action: .navigate(.init(href: "/profile")), label: "Profile")),
        .link(Navree.Link(action: .navigate(.init(href: "/settings")), label: "Settings")),
        .button(Navree.Button(label: "Sign out", role: .destructive)),
    ], name: "Harpreet Kaur")
    let alone = StatusRules.menu(destinations: destinations, appName: "Charkha", place: .menuBar, user: user)
    #expect(alone.contains(.account("Harpreet Kaur")))
    #expect(alone.contains(.accountItem(label: "Profile", index: 0, destructive: false)))
    #expect(alone.contains(.accountItem(label: "Sign Out", index: 2, destructive: true)))
    // Its settings are Settings… itself.
    #expect(!alone.contains { if case .accountItem(_, 1, _) = $0 { true } else { false } })
    // In the Dock the App menu has the account.
    let docked = StatusRules.menu(destinations: destinations, appName: "Charkha", place: .both, user: user)
    #expect(!docked.contains(.account("Harpreet Kaur")))
}

@Test func quitNamesTheApp() {
    #expect(StatusRules.menu(destinations: [], appName: "Charkha Bar", place: .menuBar, user: nil).last == .quit("Quit Charkha Bar"))
}

@Test func theCountStandsBesideTheImageOnlyWhileTheAppIsNotInTheDock() {
    #expect(StatusRules.label(item: menu, count: 4, place: .menuBar, appName: "Charkha").count == "4")
    #expect(StatusRules.label(item: menu, count: 4, place: .both, appName: "Charkha").count == nil)
    let quiet = StatusItem(image: .symbol("shippingbox"), showsCount: false)
    #expect(StatusRules.label(item: quiet, count: 4, place: .menuBar, appName: "Charkha").count == nil)
}

@Test func zeroShowsNoCount() {
    #expect(StatusRules.label(item: menu, count: 0, place: .menuBar, appName: "Charkha").count == nil)
    #expect(StatusRules.label(item: menu, count: nil, place: .menuBar, appName: "Charkha").count == nil)
}

@Test func theItemIsNamedForVoiceOverByItsTitleOrTheApp() {
    #expect(StatusRules.label(item: menu, count: nil, place: .menuBar, appName: "Charkha").accessibilityLabel == "Charkha")
    let titled = StatusItem(image: .asset("Charkha"), title: "Orders")
    let parts = StatusRules.label(item: titled, count: nil, place: .menuBar, appName: "Charkha")
    #expect(parts.accessibilityLabel == "Orders")
    #expect(parts.title == "Orders")
    #expect(parts.image == .asset("Charkha"))
}
