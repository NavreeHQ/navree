import Testing

@testable import NavreeShell

private let item = StatusItem(image: .symbol("shippingbox"))

@Test func anAppWithNoStatusItemStandsInTheDockWhateverWasChosen() {
    let presence = Presence(place: .menuBar, choices: [.dock, .menuBar, .both])
    #expect(PresenceRules.place(presence, chosen: nil) == .dock)
    #expect(PresenceRules.place(presence, chosen: .menuBar) == .dock)
    #expect(PresenceRules.place(presence, chosen: .both) == .dock)
}

@Test func anAppWhoseItemIsAWindowNotYetBuiltStandsInTheDock() {
    let presence = Presence(place: .menuBar, statusItem: StatusItem(image: .symbol("sun.max"), content: .screen("/today")))
    #expect(PresenceRules.place(presence, chosen: nil) == .dock)
    #expect(PresenceRules.choices(Presence(choices: [.dock, .menuBar], statusItem: presence.statusItem)).isEmpty)
}

@Test func thePersonsChoiceHoldsOnlyAmongTheOfferedPlaces() {
    let presence = Presence(place: .dock, choices: [.dock, .both], statusItem: item)
    #expect(PresenceRules.place(presence, chosen: .both) == .both)
    #expect(PresenceRules.place(presence, chosen: .menuBar) == .dock)
}

@Test func withNoChoiceTheAppStandsWhereItsBuildSays() {
    #expect(PresenceRules.place(Presence(place: .menuBar, statusItem: item), chosen: nil) == .menuBar)
    #expect(PresenceRules.place(Presence(place: .both, statusItem: item), chosen: nil) == .both)
    #expect(PresenceRules.place(Presence(), chosen: nil) == .dock)
}

@Test func settingsOffersNoChoiceOfOnePlace() {
    #expect(PresenceRules.choices(Presence(choices: [.both], statusItem: item)).isEmpty)
    #expect(PresenceRules.choices(Presence(choices: [.dock, .dock], statusItem: item)).isEmpty)
    #expect(PresenceRules.choices(Presence(choices: [.dock, .menuBar, .both], statusItem: item)) == [.dock, .menuBar, .both])
}

@Test func settingsNeverOffersTheMenuBarWithoutAStatusItem() {
    #expect(PresenceRules.choices(Presence(choices: [.dock, .menuBar, .both])).isEmpty)
}

@Test func aDockAppIsAlwaysRegular() {
    for windows in [0, 1, 3] {
        #expect(PresenceRules.policy(place: .dock, mainWindows: windows, dockWhileWindowOpen: false) == .regular)
        #expect(PresenceRules.policy(place: .both, mainWindows: windows, dockWhileWindowOpen: false) == .regular)
    }
}

@Test func aMenuBarAppWithNoWindowIsAnAccessory() {
    #expect(PresenceRules.policy(place: .menuBar, mainWindows: 0, dockWhileWindowOpen: true) == .accessory)
}

@Test func aMenuBarAppBringsItsDockIconWhileAWindowIsOpenWhenAsked() {
    #expect(PresenceRules.policy(place: .menuBar, mainWindows: 1, dockWhileWindowOpen: true) == .regular)
    #expect(PresenceRules.policy(place: .menuBar, mainWindows: 2, dockWhileWindowOpen: true) == .regular)
}

@Test func aMenuBarAppStaysOutOfTheDockWhenNotAsked() {
    #expect(PresenceRules.policy(place: .menuBar, mainWindows: 1, dockWhileWindowOpen: false) == .accessory)
}

@Test func aDockAppOpensItsWindowAtLaunchAsToday() {
    #expect(PresenceRules.launch(place: .dock) == .automatic)
    #expect(PresenceRules.launch(place: .both) == .automatic)
}

@Test func aMenuBarAppOpensNoWindowAtLaunch() {
    #expect(PresenceRules.launch(place: .menuBar) == .suppressed)
}

@Test func takingTheItemOutOfTheMenuBarLeavesAnAppOfBothInTheDock() {
    #expect(PresenceRules.placeAfterRemoval(.both) == .dock)
}

@Test func aMenuBarOnlyAppIsEndedByTheSystemWhenItsItemIsTakenOut() {
    #expect(PresenceRules.placeAfterRemoval(.menuBar) == nil)
}

@Test func anAppThatMayStartInTheMenuBarSaysLSUIElement() {
    #expect(PresenceRules.needsUIElement(Presence(place: .menuBar, statusItem: item)))
    #expect(PresenceRules.needsUIElement(Presence(place: .dock, choices: [.dock, .menuBar], statusItem: item)))
}

@Test func aDockOnlyAppDoesNotSayLSUIElement() {
    #expect(!PresenceRules.needsUIElement(Presence()))
    #expect(!PresenceRules.needsUIElement(Presence(place: .both, choices: [.dock, .both], statusItem: item)))
    // The menu bar offered without an item to stand there is no menu bar at all.
    #expect(!PresenceRules.needsUIElement(Presence(place: .menuBar, choices: [.dock, .menuBar])))
}

@Test func aDockOnlyAppSayingLSUIElementIsReported() {
    #expect(PresenceRules.infoPlistFault(presence: Presence(), uiElement: true) != nil)
    #expect(PresenceRules.infoPlistFault(presence: Presence(), uiElement: false) == nil)
}

@Test func aMenuBarAppNotSayingLSUIElementIsReported() {
    let presence = Presence(place: .menuBar, statusItem: item)
    #expect(PresenceRules.infoPlistFault(presence: presence, uiElement: false) != nil)
    #expect(PresenceRules.infoPlistFault(presence: presence, uiElement: true) == nil)
}

@Test func reopeningAMenuBarAppWithNoWindowOpensOne() {
    #expect(PresenceRules.reopenOpensWindow(place: .menuBar, visibleWindows: false))
    #expect(!PresenceRules.reopenOpensWindow(place: .menuBar, visibleWindows: true))
}

@Test func reopeningADockAppIsLeftToTheSystem() {
    #expect(!PresenceRules.reopenOpensWindow(place: .dock, visibleWindows: false))
    #expect(!PresenceRules.reopenOpensWindow(place: .both, visibleWindows: false))
}
