import NavreeDesign
import NavreeRuntime
@testable import NavreeShell
import Foundation
import Testing

// The window's width as the core is told it, and as the drawing lays out by:
// from the system's size class and the window's width, never the device.

@Test func aNarrowStageManagerWindowIsCompactWhateverTheSystemSays() {
    // An iPad window of 507 points: the compact layout, as a phone's.
    #expect(WindowRules.width(sizeClass: .compact, points: 507) == .compact)
    #expect(WindowRules.width(sizeClass: .regular, points: 507) == .compact)
    #expect(WindowRules.width(sizeClass: .regular, points: 599) == .compact)
    #expect(WindowRules.width(sizeClass: .regular, points: 600) == .medium)
}

@Test func aTabletUprightIsMediumAndAcrossIsExpanded() {
    #expect(WindowRules.width(sizeClass: .regular, points: 834) == .medium)
    #expect(WindowRules.width(sizeClass: .regular, points: 1024) == .expanded)
    #expect(WindowRules.width(sizeClass: .regular, points: 1366) == .expanded)
}

@Test func aWindowWithNoSizeClassIsNeverCompact() {
    // A Mac's window: its sidebar folds away by itself; no phone's tab bar.
    #expect(WindowRules.width(sizeClass: nil, points: 480) == .medium)
    #expect(WindowRules.width(sizeClass: nil, points: 999) == .medium)
    #expect(WindowRules.width(sizeClass: nil, points: 1280) == .expanded)
}

@Test func theDrawingLaysOutByTheWidthTheCoreWasTold() {
    #expect(WindowRules.window(.compact) == .compact)
    #expect(WindowRules.window(.medium) == .medium)
    #expect(WindowRules.window(.expanded) == .expanded)
    #expect(WindowRules.window(.medium).widthClass == .regular)
}

@Test func twoAddressesNameTheSameScreenByTheirPath() {
    #expect(ColumnRules.same("/orders/1042", "https://charkha.example/orders/1042"))
    #expect(ColumnRules.same("/orders/1042?tab=items", "https://charkha.example/orders/1042"))
    #expect(!ColumnRules.same("/orders/1043", "https://charkha.example/orders/1042"))
}

@Test func onAMacWithASidebarTheAccountIsAtItsFootNotInTheToolbar() {
    #expect(NavigationRules.accountPlace(bar: .window, destinations: 3) == .sidebarFoot)
    #expect(NavigationRules.accountPlace(bar: .navigation, destinations: 3) == .toolbar)
}

@Test func eachOriginKeepsItsCacheApart() {
    #expect(Session.cacheFolder("https://charkha.example") == "navree-cache-https---charkha-example")
    #expect(Session.cacheFolder("https://charkha.example") != Session.cacheFolder("https://answers.example"))
    #expect(Session.cacheFolder("http://127.0.0.1:8095") == "navree-cache-http---127-0-0-1-8095")
}

@Test func aFolderOfAnswersMayBeNamedByItsManifest() {
    let named = AnswerFolder.place(URL(fileURLWithPath: "/answers/recorded.tsv"))
    #expect(named.folder.path == "/answers")
    #expect(named.manifest == "recorded.tsv")
    let folder = AnswerFolder.place(URL(fileURLWithPath: "/answers", isDirectory: true))
    #expect(folder.folder.path == "/answers")
    #expect(folder.manifest == "manifest.tsv")
}
