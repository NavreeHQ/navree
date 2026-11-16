import SwiftUI
import Testing

@testable import NavreeShell

@Test func theContentIsCoveredWhileTheSceneIsNotActiveOnlyWhenTheAppAsks() {
    let plain = ShellConfiguration(origin: "https://shop.example", appVersion: "1")
    #expect(!plain.coversWhenInactive)
    for phase in [ScenePhase.active, .inactive, .background] {
        #expect(!CoverRules.covers(plain, phase: phase))
    }
    let covered = ShellConfiguration(origin: "https://shop.example", appVersion: "1", coversWhenInactive: true)
    #expect(!CoverRules.covers(covered, phase: .active))
    #if os(iOS)
    #expect(CoverRules.covers(covered, phase: .inactive))
    #expect(CoverRules.covers(covered, phase: .background))
    #else
    #expect(!CoverRules.covers(covered, phase: .inactive))
    #endif
}
