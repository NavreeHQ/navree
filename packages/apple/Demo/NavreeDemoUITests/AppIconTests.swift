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

        // A run that ended early can leave Indigo chosen on this simulator:
        // begin from the app's own icon.
        if chosen(in: app) == "Indigo" {
            choose("Saffron", in: app, shots: false)
        }

        choose("Indigo", in: app, shots: true)
        showOnHomeScreen("icon-05-home-indigo")

        app.activate()
        XCTAssertTrue(app.navigationBars["Orders"].waitForExistence(timeout: 20), "The app did not come back")
        choose("Saffron", in: app, shots: false)
        showOnHomeScreen("icon-06-home-saffron")
    }

    /// Opens the account menu, then the choice of icons.
    @MainActor
    private func openChoice(in app: XCUIApplication, shots: Bool) {
        let account = app.buttons["Ravneet Kaur"].firstMatch
        XCTAssertTrue(account.waitForExistence(timeout: 10), "The account menu did not appear")
        account.tap()
        let row = app.buttons["App Icon"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5), "The account menu has no App Icon")
        if shots { Shots.save("icon-01-account-menu", in: self) }
        row.tap()
    }

    /// Closes the choice of icons, and waits until it has gone.
    @MainActor
    private func closeChoice(in app: XCUIApplication) {
        let close = app.buttons["Close"].firstMatch
        guard close.waitForExistence(timeout: 3) else { return }
        close.tap()
        let gone = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: close)
        _ = XCTWaiter().wait(for: [gone], timeout: 5)
    }

    /// The icon checked in the choice of icons now, by its label.
    @MainActor
    private func chosen(in app: XCUIApplication) -> String? {
        openChoice(in: app, shots: false)
        XCTAssertTrue(app.buttons["Saffron"].firstMatch.waitForExistence(timeout: 5), "The choice of icons did not open")
        var label: String?
        for _ in 0..<6 where label == nil {
            label = ["Saffron", "Indigo"].first { app.buttons[$0].firstMatch.isSelected }
            if label == nil { Thread.sleep(forTimeInterval: 0.5) }
        }
        closeChoice(in: app)
        return label
    }

    /// Opens the choice of icons and chooses one.
    @MainActor
    private func choose(_ label: String, in app: XCUIApplication, shots: Bool) {
        openChoice(in: app, shots: shots)
        let choice = app.buttons[label].firstMatch
        XCTAssertTrue(choice.waitForExistence(timeout: 5), "\(label) is not among the icons")
        if shots { Shots.save("icon-02-choice", in: self) }
        choice.tap()
        // The system says the icon changed, in an alert of its own.
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let ok = { [app.alerts.buttons["OK"], springboard.alerts.buttons["OK"]].first(where: { $0.exists }) }
        var said = false
        for _ in 0..<20 where !said {
            said = ok() != nil
            if !said { Thread.sleep(forTimeInterval: 0.5) }
        }
        XCTAssertTrue(said, "The system did not say the icon changed")
        if shots { Shots.save("icon-03-system-says", in: self) }
        // The system finishes the change only once its alert is dismissed,
        // and a tap while the alert still comes in is lost: OK is tapped
        // until the alert has gone, the first time once it has come in.
        Thread.sleep(forTimeInterval: 1)
        for _ in 0..<10 {
            guard let button = ok() else { break }
            button.tap()
            Thread.sleep(forTimeInterval: 1)
        }
        XCTAssertNil(ok(), "The system's alert did not go")
        // The check mark moves to the icon chosen once the system has changed it.
        let chosen = NSPredicate(format: "isSelected == true")
        let moved = XCTNSPredicateExpectation(predicate: chosen, object: choice)
        XCTAssertEqual(XCTWaiter().wait(for: [moved], timeout: 10), .completed, "\(label) is not checked")
        if shots { Shots.save("icon-04-choice-made", in: self) }
        closeChoice(in: app)
    }

    /// Goes to the Home Screen and turns its pages until the app's icon
    /// shows. Just after a change the system draws the icon anew, so it is
    /// waited for on the first page before a page is turned; where the
    /// pages end in the App Library, the picture is of the App Library,
    /// which shows the icon too.
    @MainActor
    private func showOnHomeScreen(_ name: String) {
        XCUIDevice.shared.press(.home)
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let icon = springboard.icons["Charkha"].firstMatch
        XCTAssertTrue(icon.waitForExistence(timeout: 10), "The app's icon is nowhere on the Home Screen")
        let hittable = XCTNSPredicateExpectation(predicate: NSPredicate(format: "isHittable == true"), object: icon)
        _ = XCTWaiter().wait(for: [hittable], timeout: 5)
        for _ in 0..<4 where !icon.isHittable && !springboard.searchFields["App Library"].exists {
            springboard.swipeLeft()
            Thread.sleep(forTimeInterval: 1)
        }
        Thread.sleep(forTimeInterval: 2)
        Shots.save(name, in: self)
    }
}
