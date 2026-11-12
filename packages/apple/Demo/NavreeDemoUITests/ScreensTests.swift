import XCTest

/// The store demo's screens, from its recorded answers, each saved as a
/// picture: what a review against Apple's guidelines looks at.
///
/// Every pass runs the screens marked as main; the light and dark passes run
/// all of them.
final class ScreensTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private var pass: String {
        ProcessInfo.processInfo.environment["NAVREE_PASS"] ?? "light"
    }

    /// Screens beyond the main ones only in the plain light and dark passes.
    private func onlyInFullPasses() throws {
        try XCTSkipUnless(pass == "light" || pass == "dark", "Only in the light and dark passes")
    }

    @MainActor
    private func launch(select: String = "/orders", stack: String? = nil) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["-NavreeRecorded", "YES", "-NavreeSelect", select]
        if let stack {
            app.launchArguments += ["-NavreeStack", stack]
        }
        if ProcessInfo.processInfo.environment["NAVREE_RTL"] == "1" {
            app.launchArguments += ["-AppleTextDirection", "YES", "-NSForceRightToLeftWritingDirection", "YES"]
        }
        app.launch()
        return app
    }

    @MainActor
    private func wait(_ element: XCUIElement, _ what: String, timeout: TimeInterval = 20) {
        XCTAssertTrue(element.waitForExistence(timeout: timeout), "\(what) did not appear")
    }

    @MainActor
    func test01Orders() {
        let app = launch()
        wait(app.navigationBars["Orders"], "The orders screen")
        wait(app.descendants(matching: .any)["CS-220480"].firstMatch, "The first order")
        Shots.save("01-orders", in: self)
    }

    @MainActor
    func test02Order() {
        let app = launch(stack: "/orders=/orders/1")
        wait(app.navigationBars["Order CS-20481"], "The order")
        Shots.save("02-order", in: self)
        app.swipeUp()
        Shots.save("03-order-scrolled", in: self)
    }

    @MainActor
    func test04RefundWithErrors() {
        let app = launch(stack: "/orders=/orders/1")
        wait(app.navigationBars["Order CS-20481"], "The order")
        app.buttons["Refund"].firstMatch.tap()
        let sheet = app.navigationBars["Refund CS-20481"]
        wait(sheet, "The refund sheet")
        Shots.save("04-refund", in: self)
        let amount = app.textFields["Amount"].firstMatch
        wait(amount, "The amount field")
        let rtl = ProcessInfo.processInfo.environment["NAVREE_RTL"] == "1"
        amount.coordinate(withNormalizedOffset: CGVector(dx: rtl ? 0.03 : 0.97, dy: 0.75)).tap()
        amount.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 12) + "0")
        sheet.buttons["Refund"].tap()
        let error = app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "Enter an amount above zero")).firstMatch
        let shown = error.waitForExistence(timeout: 20)
        Shots.save("05-refund-errors", in: self)
        XCTAssertTrue(shown, "The amount's error did not appear")
    }

    @MainActor
    func test06Inventory() {
        let app = launch(select: "/inventory")
        wait(app.navigationBars["Inventory"], "The inventory")
        wait(app.descendants(matching: .any)["Chakla belan, Amritsar"].firstMatch, "The first product")
        Shots.save("06-inventory", in: self)
    }

    @MainActor
    func test07RestockFromASwipe() throws {
        try onlyInFullPasses()
        let app = launch(select: "/inventory")
        let product = app.staticTexts["Chakla belan, Amritsar"]
        wait(product, "The first product")
        product.swipeLeft()
        Shots.save("07-inventory-swipe", in: self)
        app.buttons["Restock"].firstMatch.tap()
        wait(app.navigationBars["Restock"], "The restock sheet")
        Shots.save("08-restock", in: self)
        app.navigationBars["Restock"].buttons["Restock"].tap()
        wait(app.staticTexts["Restocked"], "The restock's status line")
        Shots.save("09-restocked", in: self)
    }

    @MainActor
    func test10ResendAsksFirst() throws {
        try onlyInFullPasses()
        let app = launch(stack: "/orders=/orders/1")
        wait(app.navigationBars["Order CS-20481"], "The order")
        app.buttons["Resend confirmation"].firstMatch.tap()
        wait(app.alerts.firstMatch, "The question")
        Shots.save("10-resend-alert", in: self)
        app.alerts.firstMatch.buttons["Send it"].tap()
        wait(app.staticTexts["Confirmation sent"], "The resend's status line")
        Shots.save("11-resent", in: self)
    }

    @MainActor
    func test12PeekAnOrder() throws {
        try onlyInFullPasses()
        let app = launch()
        let first = app.staticTexts["CS-220480"]
        wait(first, "The first order")
        first.tap()
        wait(app.navigationBars["Order CS-20481"], "The order's sheet")
        Shots.save("12-order-sheet", in: self)
    }

    @MainActor
    func test13Filters() throws {
        try onlyInFullPasses()
        let app = launch()
        wait(app.navigationBars["Orders"], "The orders screen")
        app.navigationBars["Orders"].buttons["Filter"].tap()
        wait(app.navigationBars["Filters"], "The filters")
        Shots.save("13-filters", in: self)
    }

    @MainActor
    func test14DashboardInTheWebView() throws {
        try onlyInFullPasses()
        let app = launch(select: "/")
        wait(app.webViews.firstMatch, "The dashboard's web view")
        _ = app.webViews.firstMatch.staticTexts.firstMatch.waitForExistence(timeout: 10)
        Shots.save("14-dashboard-web", in: self)
        Shots.note("14-dashboard-web", app.webViews.firstMatch.debugDescription)
    }

    @MainActor
    func test15MissingOrder() throws {
        try onlyInFullPasses()
        let app = launch(stack: "/orders=/orders/999999")
        wait(app.staticTexts["That order isn't in the shop"], "The missing order's screen")
        Shots.save("15-missing-order", in: self)
    }
}
