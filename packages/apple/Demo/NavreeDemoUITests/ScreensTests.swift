import XCTest

final class ScreensTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    @MainActor
    func testTheFirstScreenDraws() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.navigationBars["Orders"].waitForExistence(timeout: 20))
        Shots.save("00-first-screen", in: self)
    }
}
