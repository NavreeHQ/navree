import Testing

@testable import NavreeShell

private let inbox = PresenceDestination(label: "Inbox", href: "/inbox", badge: "5")
private let board = PresenceDestination(label: "Board", href: "/board", badge: "New")

@Test func noCountOrZeroClearsTheBadge() {
    #expect(BadgeRules.label(nil) == nil)
    #expect(BadgeRules.label(0) == nil)
    #expect(BadgeRules.label(-2) == nil)
}

@Test func aCountIsItsDigits() {
    #expect(BadgeRules.label(3) == "3")
    #expect(BadgeRules.label(1200) == "1200")
}

@Test func aDestinationsBadgeCountsOnlyWhenItIsANumber() {
    #expect(BadgeRules.count(destinationBadge: "12") == 12)
    #expect(BadgeRules.count(destinationBadge: " 4 ") == 4)
    #expect(BadgeRules.count(destinationBadge: "New") == nil)
    #expect(BadgeRules.count(destinationBadge: "4+") == nil)
    #expect(BadgeRules.count(destinationBadge: "-3") == nil)
    #expect(BadgeRules.count(destinationBadge: "٣") == nil)
    #expect(BadgeRules.count(destinationBadge: nil) == nil)
}

@Test func aCallSetsTheBadgeOfAnAppThatFollowsCalls() {
    var state = BadgeRules.called(BadgeRules.State(), count: 3)
    #expect(state.count == 3)
    // The destinations of the window in front do not move it.
    state = BadgeRules.destinationsChanged(state, source: .calls, destinations: [inbox])
    #expect(state.count == 3)
    #expect(BadgeRules.called(state, count: nil).count == nil)
    #expect(BadgeRules.called(state, count: 0).count == nil)
}

@Test func aFollowedDestinationsBadgeWinsWhenItChanges() {
    let source = BadgeSource.destination("/inbox")
    var state = BadgeRules.destinationsChanged(BadgeRules.State(), source: source, destinations: [inbox, board])
    #expect(state.count == 5)
    // A call sets it until the destination's badge changes.
    state = BadgeRules.called(state, count: 9)
    state = BadgeRules.destinationsChanged(state, source: source, destinations: [inbox, board])
    #expect(state.count == 9)
    var read = inbox
    read.badge = "2"
    state = BadgeRules.destinationsChanged(state, source: source, destinations: [read, board])
    #expect(state.count == 2)
    // A word is no count; a destination gone (signed out) takes it away.
    state = BadgeRules.destinationsChanged(state, source: .destination("/board"), destinations: [read, board])
    #expect(state.count == nil)
    state = BadgeRules.destinationsChanged(BadgeRules.called(state, count: 4), source: source, destinations: [])
    #expect(state.count == nil)
}

@Test func aDockAppBadgesItsDockIcon() {
    #expect(BadgeRules.targets(place: .dock, statusShowsCount: true) == [.appIcon])
}

@Test func aMenuBarAppCountsOnItsStatusItem() {
    #expect(BadgeRules.targets(place: .menuBar, statusShowsCount: true) == [.appIcon, .statusItem])
    #expect(BadgeRules.targets(place: .menuBar, statusShowsCount: false) == [.appIcon])
}

@Test func anAppInBothPlacesBadgesItsDockIcon() {
    #expect(BadgeRules.targets(place: .both, statusShowsCount: true) == [.appIcon])
}

@Test func aNumberIsACountAndAWordIsAWord() {
    #expect(BadgeRules.tabBadgeKind("3") == .count(3))
    #expect(BadgeRules.tabBadgeKind("New") == .word("New"))
    #expect(BadgeRules.tabBadgeKind("0") == TabBadge.none)
    #expect(BadgeRules.tabBadgeKind("  ") == TabBadge.none)
    #expect(BadgeRules.tabBadgeKind(nil) == TabBadge.none)
    #expect(BadgeRules.tabBadge(nil) == nil)
    #expect(BadgeRules.tabBadge("New") != nil)
}
