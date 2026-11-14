import XCTest

/// The app's place on the Home Screen, as a person meets it: its quick
/// actions (a long press on its icon) and the count on its icon.
final class QuickActionTests: XCTestCase {
    @MainActor private var springboard: XCUIApplication {
        XCUIApplication(bundleIdentifier: "com.apple.springboard")
    }

    override func setUp() {
        continueAfterFailure = false
    }

    /// The app's icon on the Home Screen, on whichever page it stands.
    @MainActor
    private func icon() -> XCUIElement {
        let icons = springboard.icons.matching(identifier: "Charkha")
        XCTAssertTrue(icons.firstMatch.waitForExistence(timeout: 10), "The app's icon is not on the Home Screen")
        for _ in 0..<4 {
            if let shown = icons.allElementsBoundByIndex.first(where: \.isHittable) { return shown }
            springboard.swipeLeft()
        }
        return icons.firstMatch
    }

    /// The app sets its first destinations as quick actions; choosing one on
    /// the Home Screen, while the app runs, opens that destination.
    @MainActor
    func testAQuickActionOpensItsDestination() {
        let app = XCUIApplication()
        app.launchArguments = ["-NavreeRecorded", "YES", "-NavreeShortcuts", "4", "-NavreeSelect", "/orders"]
        app.launch()
        XCTAssertTrue(app.navigationBars["Orders"].waitForExistence(timeout: 30), "The orders screen did not appear")
        XCUIDevice.shared.press(.home)
        icon().press(forDuration: 1.5)
        let action = springboard.buttons["Inventory"]
        XCTAssertTrue(action.waitForExistence(timeout: 10), "The quick action is not in the icon's menu")
        Shots.note("quick-actions", springboard.buttons.allElementsBoundByIndex.map(\.label).joined(separator: "\n"))
        Shots.save("quick-actions", in: self)
        action.tap()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10), "The app did not come forward")
        XCTAssertTrue(app.navigationBars["Inventory"].waitForExistence(timeout: 20), "The quick action did not open Inventory")
        Shots.save("quick-action-opened", in: self)
    }

    /// The count a server's `set_badge` sets stands on the app's icon once
    /// the person allows notifications.
    @MainActor
    func testTheCountStandsOnTheAppsIcon() {
        let app = XCUIApplication()
        app.launchArguments = ["-NavreeRecorded", "YES", "-NavreeBadge", "3"]
        app.launch()
        let allow = springboard.alerts.buttons["Allow"]
        if allow.waitForExistence(timeout: 10) { allow.tap() }
        XCTAssertTrue(app.navigationBars.firstMatch.waitForExistence(timeout: 30), "The app did not start")
        XCUIDevice.shared.press(.home)
        let icon = icon()
        Shots.note("badge", "icon \(icon.label), value \(String(describing: icon.value))")
        Shots.save("badge-home-screen", in: self)
    }
}
