import CoreGraphics
import NavreeProtocol
import NavreeRuntime
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

private let held = StatusItem(image: .symbol("shippingbox"), content: .app())

@Test func anItemHoldingTheAppOpensAWindowInTheMenuBar() {
    #expect(StatusRules.style(held) == .window)
    #expect(StatusRules.holdsApp(held))
    #expect(StatusRules.inserted(style: .window, item: held, place: .menuBar))
    #expect(!StatusRules.inserted(style: .menu, item: held, place: .menuBar))
    #expect(!StatusRules.holdsApp(menu))
}

@Test func anItemHoldingTheAppStandsInTheMenuBarOnlyNeverBesideTheDocksWindows() {
    // Beside a Dock app's own windows it would be a second session.
    #expect(PresenceRules.place(Presence(place: .both, statusItem: held), chosen: nil) == .menuBar)
    #expect(PresenceRules.choices(Presence(choices: [.dock, .menuBar, .both], statusItem: held)) == [.dock, .menuBar])
    #expect(PresenceRules.place(Presence(place: .menuBar, choices: [.dock, .menuBar], statusItem: held), chosen: .dock) == .dock)
}

@Test func theWindowOpensUnderTheItemCentredOnItAndKeptOnTheScreen() {
    let screen = CGRect(x: 0, y: 0, width: 1440, height: 870)
    let middle = StatusRules.windowFrame(item: CGRect(x: 900, y: 870, width: 32, height: 30), size: CGSize(width: 400, height: 560), screen: screen)
    #expect(middle == CGRect(x: 716, y: 306, width: 400, height: 560))
    // At the screen's edge it stays on the screen.
    let edge = StatusRules.windowFrame(item: CGRect(x: 1400, y: 870, width: 32, height: 30), size: CGSize(width: 400, height: 560), screen: screen)
    #expect(edge.maxX == 1432)
    // A screen too short for it keeps it on the screen.
    let short = StatusRules.windowFrame(item: CGRect(x: 900, y: 600, width: 32, height: 30), size: CGSize(width: 400, height: 900), screen: CGRect(x: 0, y: 0, width: 1440, height: 600))
    #expect(short.minY == 8)
}

@Test func theWindowOpensOnTheScreenThatHoldsTheItem() {
    // A second screen to the right of the first, its menu bar with the item.
    let second = CGRect(x: 1440, y: 0, width: 1920, height: 1050)
    let frame = StatusRules.windowFrame(item: CGRect(x: 3300, y: 1050, width: 32, height: 30), size: CGSize(width: 400, height: 560), screen: second)
    #expect(second.contains(frame))
    #expect(frame.maxX == second.maxX - 8)
    #expect(frame.maxY == 1046)
    // A screen above the first, with its own menu bar.
    let above = CGRect(x: 0, y: 870, width: 1440, height: 870)
    let high = StatusRules.windowFrame(item: CGRect(x: 100, y: 1740, width: 32, height: 30), size: CGSize(width: 400, height: 560), screen: above)
    #expect(above.contains(high))
    #expect(high.minX == 8)
}

@MainActor
@Test func theWindowUnderTheItemCountsTheSystemsSheetsStandingOverIt() {
    let model = PresenceModel()
    #expect(model.systemSheets == 0)
    model.systemSheet(opened: true)
    model.systemSheet(opened: true)
    #expect(model.systemSheets == 2)
    model.systemSheet(opened: false)
    model.systemSheet(opened: false)
    model.systemSheet(opened: false)
    // Never fewer than none, however a sheet ended.
    #expect(model.systemSheets == 0)
}

@Test func aRightClickOffersTheAppThenOpenInWindowSettingsAboutAndQuit() {
    let entries = StatusRules.contextMenu(destinations: destinations, appName: "Charkha Bar", user: nil)
    #expect(entries == [
        .destination(destinations[0]), .destination(destinations[1]), .divider, .openInWindow, .settings,
        .about("About Charkha Bar"), .divider, .quit("Quit Charkha Bar"),
    ])
    // Before the shell has come: no destinations, never "Open".
    #expect(StatusRules.contextMenu(destinations: [], appName: "Charkha Bar", user: nil).first == .openInWindow)
}

@Test func theWindowUnderTheItemIsLaidOutAsACompactWindow() {
    #expect(StatusRules.width(compactWindow: true, sizeClass: nil, points: 400) == .compact)
    #expect(StatusRules.width(compactWindow: true, sizeClass: nil, points: 1200) == .compact)
    #expect(StatusRules.width(compactWindow: false, sizeClass: nil, points: 1200) == .expanded)
}

@Test func theWindowUnderTheItemHasTheCompactBarOfANarrowUtilityWindow() {
    // 24-point controls in a 40-point bar, not the unified bar's 36-point circles.
    #expect(StatusRules.windowBar == .compact)
}

@Test func theFullWindowIsNeverNarrowerThanItsColumnsNeed() {
    // A sidebar at its least (180) beside a detail at its least (400).
    #expect(StatusRules.fullWindowMinimum(configured: CGSize(width: 480, height: 400), columns: CGSize(width: 579.5, height: 120))
        == CGSize(width: 580, height: 400))
    // The app's own least size where it is wider.
    #expect(StatusRules.fullWindowMinimum(configured: CGSize(width: 900, height: 600), columns: CGSize(width: 580, height: 120))
        == CGSize(width: 900, height: 600))
}
