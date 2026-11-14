import XCTest

/// The app's icon on iPhone and iPad: "App Icon" in the account menu, the
/// choice of icons it opens, the system's word that the icon changed, and the
/// Home Screen with the icon chosen. It ends with the app's own icon back.
final class AppIconTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    @MainActor
    func testChoosingAnotherIconAndBack() {
        let app = XCUIApplication()
        app.launchArguments = ["-NavreeRecorded", "YES", "-NavreeSelect", "/orders"]
        app.launch()
        XCTAssertTrue(app.navigationBars["Orders"].waitForExistence(timeout: 20), "The orders screen did not appear")

        choose("Indigo", in: app, shots: true)
        showOnHomeScreen("icon-05-home-indigo")

        app.activate()
        XCTAssertTrue(app.navigationBars["Orders"].waitForExistence(timeout: 20), "The app did not come back")
        choose("Saffron", in: app, shots: false)
        showOnHomeScreen("icon-06-home-saffron")
    }

    /// Opens the account menu, then the choice of icons, and chooses one.
    @MainActor
    private func choose(_ label: String, in app: XCUIApplication, shots: Bool) {
        let account = app.buttons["Ravneet Kaur"].firstMatch
        XCTAssertTrue(account.waitForExistence(timeout: 10), "The account menu did not appear")
        account.tap()
        let row = app.buttons["App Icon"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5), "The account menu has no App Icon")
        if shots { Shots.save("icon-01-account-menu", in: self) }
        row.tap()
        let choice = app.buttons[label].firstMatch
        XCTAssertTrue(choice.waitForExistence(timeout: 5), "\(label) is not among the icons")
        if shots { Shots.save("icon-02-choice", in: self) }
        choice.tap()
        // The system says the icon changed, in an alert of its own.
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let ok = [app.alerts.buttons["OK"], springboard.alerts.buttons["OK"]]
        var said = false
        for _ in 0..<20 where !said {
            if let button = ok.first(where: { $0.exists }) {
                if shots { Shots.save("icon-03-system-says", in: self) }
                button.tap()
                said = true
            } else {
                Thread.sleep(forTimeInterval: 0.5)
            }
        }
        XCTAssertTrue(said, "The system did not say the icon changed")
        // The check mark moves to the icon chosen once the system has changed it.
        let chosen = NSPredicate(format: "isSelected == true")
        let moved = XCTNSPredicateExpectation(predicate: chosen, object: choice)
        XCTAssertEqual(XCTWaiter().wait(for: [moved], timeout: 10), .completed, "\(label) is not checked")
        if shots { Shots.save("icon-04-choice-made", in: self) }
        let close = app.buttons["Close"].firstMatch
        if close.waitForExistence(timeout: 3) { close.tap() }
    }

    /// Goes to the Home Screen and turns its pages until the app's icon shows;
    /// an app found only in the App Library is put on the Home Screen first,
    /// as a person would.
    @MainActor
    private func showOnHomeScreen(_ name: String) {
        XCUIDevice.shared.press(.home)
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let icon = springboard.icons["Charkha"].firstMatch
        XCTAssertTrue(icon.waitForExistence(timeout: 10), "The app's icon is nowhere on the Home Screen")
        turnPages(to: icon, in: springboard)
        if springboard.searchFields["App Library"].exists {
            icon.press(forDuration: 1.5)
            let add = springboard.buttons["Add to Home Screen"].firstMatch
            if add.waitForExistence(timeout: 3) {
                add.tap()
                Thread.sleep(forTimeInterval: 2)
                XCUIDevice.shared.press(.home)
                Thread.sleep(forTimeInterval: 1)
                XCUIDevice.shared.press(.home)
                turnPages(to: icon, in: springboard)
            } else {
                XCUIDevice.shared.press(.home)
            }
        }
        Thread.sleep(forTimeInterval: 2)
        Shots.save(name, in: self)
    }

    @MainActor
    private func turnPages(to icon: XCUIElement, in springboard: XCUIApplication) {
        for _ in 0..<4 where !icon.isHittable {
            springboard.swipeLeft()
            Thread.sleep(forTimeInterval: 1)
        }
    }
}
