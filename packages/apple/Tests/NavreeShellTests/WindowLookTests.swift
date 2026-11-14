import CoreGraphics
import Foundation
import Testing

@testable import NavreeShell

// The app's windows, as far as the system lets an app choose: sizes, the
// toolbar's labels, tabs. Everything else of the window is the system's.

@Test func anAppThatSaysNothingKeepsTheSystemsOwnWindowsAndOffersNoIcons() {
    let configuration = ShellConfiguration(origin: "https://charkha.example", appVersion: "1.0")
    #expect(configuration.windows == WindowLook())
    #expect(configuration.windows.idealSize == nil)
    #expect(configuration.windows.minimumSize == nil)
    #expect(configuration.windows.toolbarLabels == .automatic)
    #expect(configuration.windows.tabs)
    #expect(configuration.icons.isEmpty)
}

@Test func aWindowWithNoSizesKeepsTheSystemsOwn() {
    #expect(WindowLookRules.firstSize(WindowLook()) == nil)
    #expect(WindowLookRules.firstSize(WindowLook(minimumSize: CGSize(width: 760, height: 480))) == nil)
}

@Test func aNewWindowOpensAtTheIdealSize() {
    let look = WindowLook(idealSize: CGSize(width: 1180, height: 760), minimumSize: CGSize(width: 760, height: 480))
    #expect(WindowLookRules.firstSize(look) == CGSize(width: 1180, height: 760))
}

@Test func theIdealSizeIsNeverBelowTheMinimum() {
    let look = WindowLook(idealSize: CGSize(width: 600, height: 900), minimumSize: CGSize(width: 760, height: 480))
    #expect(WindowLookRules.firstSize(look) == CGSize(width: 760, height: 900))
}

@Test func windowsJoinAsTabsUnlessTheAppSaysNot() {
    #expect(WindowLookRules.allowsTabs(WindowLook()))
    #expect(!WindowLookRules.allowsTabs(WindowLook(tabs: false)))
}

#if os(macOS)
@Test func eachLabelChoiceIsTheToolbarsDisplayModeOfTheSameName() {
    #expect(WindowKeeper.mode(.automatic) == nil)
    #expect(WindowKeeper.mode(.iconOnly) == .iconOnly)
    #expect(WindowKeeper.mode(.titleAndIcon) == .iconAndLabel)
    #expect(WindowKeeper.mode(.titleOnly) == .labelOnly)
}
#endif
