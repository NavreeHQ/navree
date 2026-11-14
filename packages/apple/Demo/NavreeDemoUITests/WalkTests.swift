import XCTest

/// A finger's walk through the demo, where a launch argument cannot go: a
/// screen pictured a swipe at a time, and a conversation opened by a tap on
/// its row.
///
/// Runs only when asked, in the test's environment (for `xcodebuild`, each
/// with the prefix `TEST_RUNNER_`):
///
/// - `NAVREE_WALK=1`: run at all.
/// - `NAVREE_WALK_ARGUMENTS`: the demo's launch arguments for the swipes,
///   separated by `|` (`-NavreeSelect|/`); `NAVREE_WALK_ORIGIN`, the server
///   (`http://127.0.0.1:8095`); `NAVREE_WALK_NAME`, the pictures' name
///   (`walk`); `NAVREE_WALK_SWIPES`, how many swipes (4).
/// - `NAVREE_INBOX_ORIGIN`: the inbox demo, for the conversation.
///
/// Pictures go to the folder `NAVREE_SHOTS` names, as the other tests'.
final class WalkTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = true
        try XCTSkipUnless(environment["NAVREE_WALK"] == "1", "Only when NAVREE_WALK=1")
    }

    private var environment: [String: String] {
        ProcessInfo.processInfo.environment
    }

    /// The screen the arguments open, pictured, then a swipe further down at a time.
    @MainActor
    func test01Swipes() throws {
        let arguments = (environment["NAVREE_WALK_ARGUMENTS"] ?? "").split(separator: "|").map(String.init)
        try XCTSkipIf(arguments.isEmpty, "Only with NAVREE_WALK_ARGUMENTS")
        let name = environment["NAVREE_WALK_NAME"] ?? "walk"
        let app = XCUIApplication()
        if let origin = environment["NAVREE_WALK_ORIGIN"], !origin.isEmpty {
            app.launchEnvironment["NAVREE_ORIGIN"] = origin
        }
        app.launchArguments = arguments
        app.launch()
        XCTAssertTrue(app.navigationBars.firstMatch.waitForExistence(timeout: 60), "The screen did not appear")
        settle(5)
        Shots.save("\(name)-0", in: self)
        for swipe in 1...(Int(environment["NAVREE_WALK_SWIPES"] ?? "") ?? 4) {
            app.swipeUp(velocity: .slow)
            settle(2)
            Shots.save("\(name)-\(swipe)", in: self)
        }
    }

    /// A conversation opened by a tap on its row, on whatever width the
    /// device has: on a compact one the thread takes the screen, its reply
    /// resting above the tab bar.
    @MainActor
    func test02ConversationByTap() throws {
        guard let origin = environment["NAVREE_INBOX_ORIGIN"], !origin.isEmpty else {
            throw XCTSkip("Only with NAVREE_INBOX_ORIGIN")
        }
        let app = XCUIApplication()
        app.launchEnvironment["NAVREE_ORIGIN"] = origin
        app.launchArguments = ["-NavreeSelect", "/inbox"]
        app.launch()
        let row = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", "Sukhpreet Chahal")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 90), "The conversation's row did not appear")
        Shots.save("inbox-list", in: self)
        row.tap()
        let reply = app.descendants(matching: .any)
            .matching(NSPredicate(format: "placeholderValue BEGINSWITH %@", "Write a reply")).firstMatch
        XCTAssertTrue(reply.waitForExistence(timeout: 30), "The conversation's reply did not appear")
        settle(3)
        Shots.save("inbox-thread", in: self)
        let tabs = app.tabBars.firstMatch
        Shots.note("inbox-thread", "reply \(reply.frame)\ntab bar \(tabs.exists ? "\(tabs.frame)" : "none")\nwindow \(app.frame)\n")
        if tabs.exists, tabs.frame.minY > app.frame.midY {
            XCTAssertLessThanOrEqual(reply.frame.maxY, tabs.frame.minY, "The reply runs under the tab bar")
        }
    }

    @MainActor
    private func settle(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }
}
