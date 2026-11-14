import NavreeDesign
import Testing

@testable import NavreeShell

private let item = StatusItem(image: .symbol("shippingbox"))

@Test func withNothingOfItsOwnTheSettingsWindowShowsOnlyTheServersScreen() {
    #expect(SettingsRules.panes(presence: Presence(), hasServerSettings: true) == [.server])
    #expect(SettingsRules.panes(presence: Presence(), hasServerSettings: false) == [.server])
    // A choice of one place is no choice.
    #expect(SettingsRules.panes(presence: Presence(choices: [.both], statusItem: item), hasServerSettings: true) == [.server])
}

@Test func theGeneralPaneComesFirstWhenTheAppOffersAChoice() {
    let choice = Presence(choices: [.dock, .menuBar, .both], statusItem: item)
    #expect(SettingsRules.panes(presence: choice, hasServerSettings: true) == [.general, .server])
    #expect(SettingsRules.panes(presence: Presence(offersOpenAtLogin: true), hasServerSettings: true) == [.general, .server])
}

@Test func withNoSettingsOfTheServersTheGeneralPaneStandsAlone() {
    #expect(SettingsRules.panes(presence: Presence(offersOpenAtLogin: true), hasServerSettings: false) == [.general])
}
