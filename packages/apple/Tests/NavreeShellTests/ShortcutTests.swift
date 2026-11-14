import Testing

@testable import NavreeShell

private let destinations = [
    PresenceDestination(label: "Orders", href: "/orders", symbol: "shippingbox"),
    PresenceDestination(label: "Inventory", href: "/inventory", symbol: "cube.box"),
    PresenceDestination(label: "Customers", href: "/customers", symbol: "person.2"),
    PresenceDestination(label: "Reports", href: "/reports"),
    PresenceDestination(label: "Settings", href: "/settings", symbol: "gearshape"),
]

@Test func noShortcutsUnlessTheAppTurnsThemOn() {
    #expect(ShortcutRules.entries(source: .off, destinations: destinations).isEmpty)
    #expect(ShortcutRules.entries(source: .on(limit: 0), destinations: destinations).isEmpty)
}

@Test func theFirstDestinationsStandInUpToTheLimit() {
    #expect(ShortcutRules.entries(source: .on(), destinations: destinations).map(\.label) == ["Orders", "Inventory", "Customers", "Reports"])
    #expect(ShortcutRules.entries(source: .on(limit: 2), destinations: destinations).map(\.href) == ["/orders", "/inventory"])
    // A destination that is not an address of the app's own is never a way in.
    let elsewhere = [PresenceDestination(label: "Help", href: "https://help.example/")] + destinations
    #expect(ShortcutRules.entries(source: .on(limit: 1), destinations: elsewhere).map(\.label) == ["Orders"])
}

@Test func aShortcutTakesItsDestinationsSymbol() {
    let entries = ShortcutRules.entries(source: .on(), destinations: destinations)
    #expect(entries[0].symbol == "shippingbox")
    #expect(entries[3].symbol == nil)
}

@Test func aQuickActionCarriesItsAddress() {
    let action = ShortcutRules.item(ShortcutEntry(label: "Orders", href: "/orders", symbol: "shippingbox"))
    #expect(action == QuickAction(type: "navree.open", title: "Orders", symbol: "shippingbox", href: "/orders"))
    #expect(ShortcutRules.href(type: action.type, userInfo: ["href": action.href]) == "/orders")
}

@Test func aQuickActionOfAnotherTypeOpensNothing() {
    #expect(ShortcutRules.href(type: "com.other.compose", userInfo: ["href": "/orders"]) == nil)
    #expect(ShortcutRules.href(type: "navree.open", userInfo: ["href": "https://elsewhere.example/"]) == nil)
    #expect(ShortcutRules.href(type: "navree.open", userInfo: nil) == nil)
}

@Test func aStaticQuickActionOfTheInfoPlistOpensItsAddress() {
    // As `UIApplicationShortcutItemUserInfo` in the Info.plist gives it.
    let userInfo: [String: Any] = ["href": "/orders/new"]
    #expect(ShortcutRules.href(type: ShortcutRules.quickActionType, userInfo: userInfo) == "/orders/new")
}

@Test func theDockMenuOffersANewWindowThenTheShortcuts() {
    let entries = ShortcutRules.entries(source: .on(limit: 2), destinations: destinations)
    #expect(ShortcutRules.dockMenu(entries: entries) == [.newWindow, .shortcut(entries[0]), .shortcut(entries[1])])
}

@Test func withShortcutsOffTheDockMenuHasOnlyTheSystemsItems() {
    #expect(ShortcutRules.dockMenu(entries: ShortcutRules.entries(source: .off, destinations: destinations)).isEmpty)
}
