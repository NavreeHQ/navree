import XCTest

/// A conversation opened from the inbox's list shows its own title in the
/// bar, "Conversation", never the title of its inspector ("Customer"), launch
/// after launch, in light and in dark, and the inspector opens from its
/// button in the bar. Each launch's bars and their frames are noted beside
/// its picture.
///
/// Runs only when asked, in the test's environment (for `xcodebuild`, each
/// with the prefix `TEST_RUNNER_`):
///
/// - `NAVREE_INBOX_ORIGIN`: the inbox demo.
/// - `NAVREE_LAUNCHES`: how many launches (6), every other one in dark.
/// - `NAVREE_TITLE_ROW`: the words of a row to open with the demo's launch
///   tap (`-NavreeTap`), as the pictures open it; without it, a finger taps
///   the first conversation.
final class ConversationTitleTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = true
        try XCTSkipIf((environment["NAVREE_INBOX_ORIGIN"] ?? "").isEmpty, "Only with NAVREE_INBOX_ORIGIN")
    }

    private var environment: [String: String] {
        ProcessInfo.processInfo.environment
    }

    @MainActor
    func test01AConversationsBarSaysConversation() throws {
        let launches = Int(environment["NAVREE_LAUNCHES"] ?? "") ?? 6
        let row = environment["NAVREE_TITLE_ROW"] ?? ""
        var notes = ""
        var wrong: [Int] = []
        for launch in 1...launches {
            let dark = launch.isMultiple(of: 2)
            XCUIDevice.shared.appearance = dark ? .dark : .light
            let app = XCUIApplication()
            app.launchEnvironment["NAVREE_ORIGIN"] = environment["NAVREE_INBOX_ORIGIN"]
            app.launchArguments = ["-NavreeSelect", "/inbox"] + (row.isEmpty ? [] : ["-NavreeTap", row])
            app.launch()
            if row.isEmpty {
                let filter = app.buttons["Filter"].firstMatch
                XCTAssertTrue(filter.waitForExistence(timeout: 90), "Launch \(launch): the conversations did not appear")
                settle(2)
                let first = app.buttons.allElementsBoundByIndex.first { button in
                    button.frame.minY > filter.frame.maxY + 40 && button.frame.width > 200 && button.isHittable
                }
                first?.tap()
            }
            let saved = app.buttons["Saved replies"].firstMatch
            XCTAssertTrue(saved.waitForExistence(timeout: 90), "Launch \(launch): the conversation did not open")
            settle(3)
            let bars = app.navigationBars.allElementsBoundByIndex.filter(\.exists)
            let titles = bars.map(\.identifier)
            notes += "launch \(launch) (\(dark ? "dark" : "light")): " + bars.map { "\($0.identifier) \($0.frame)" }.joined(separator: "; ") + "\n"
            Shots.save("title-\(launch)-\(dark ? "dark" : "light")", in: self)
            if titles.contains("Customer") || !titles.contains("Conversation") { wrong.append(launch) }
            if launch == launches {
                // The inspector still opens from its button in the bar.
                let inspector = app.navigationBars.buttons["Customer"].firstMatch
                if inspector.exists {
                    inspector.tap()
                    let opened = app.navigationBars["Customer"].firstMatch.waitForExistence(timeout: 15)
                    settle(2)
                    Shots.save("title-inspector-opened", in: self)
                    notes += "the inspector's button opened Customer: \(opened)\n"
                    XCTAssertTrue(opened, "The inspector's button did not open the customer")
                } else {
                    notes += "no inspector button in the bar\n"
                }
            }
            app.terminate()
        }
        XCUIDevice.shared.appearance = .light
        notes += "wrong title in launches: \(wrong)\n"
        Shots.note("title", notes)
        XCTAssertEqual(wrong, [], "The conversation's bar did not say Conversation in launches \(wrong)")
    }

    @MainActor
    private func settle(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }
}
