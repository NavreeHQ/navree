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
        let row = try firstConversation(in: app)
        Shots.save("inbox-list", in: self)
        row.tap()
        let reply = app.descendants(matching: .any)
            .matching(NSPredicate(format: "placeholderValue BEGINSWITH %@", "Write a reply")).firstMatch
        XCTAssertTrue(reply.waitForExistence(timeout: 30), "The conversation's reply did not appear")
        settle(3)
        Shots.save("inbox-thread", in: self)
        let tabs = app.tabBars.firstMatch
        // What a finger can hit: each control's frame, in points.
        var frames = "reply \(reply.frame)\ntab bar \(tabs.exists ? "\(tabs.frame)" : "none")\nwindow \(app.frame)\n"
        for name in ["Snooze", "Close", "Mark as read", "Reply", "Internal note", "Saved replies", "Send"] {
            let control = app.buttons[name].firstMatch
            frames += "\(name): \(control.exists ? "\(control.frame)" : "none")\n"
        }
        Shots.note("inbox-thread", frames)
        if tabs.exists, tabs.frame.minY > app.frame.midY {
            XCTAssertLessThanOrEqual(reply.frame.maxY, tabs.frame.minY, "The reply runs under the tab bar")
        }
    }

    /// The reply's small controls take a finger's tap across their whole
    /// target: "Saved replies", drawn as its words alone, is 44 points tall
    /// and opens its replies from a tap well above its words; the field and
    /// its send button stand as one, the button no taller than the field.
    /// Their frames, and what the system's hit-region audit finds too small,
    /// are noted beside the pictures.
    @MainActor
    func test03ReplyControlsTakeATapAcrossTheirTarget() throws {
        guard let origin = environment["NAVREE_INBOX_ORIGIN"], !origin.isEmpty else {
            throw XCTSkip("Only with NAVREE_INBOX_ORIGIN")
        }
        let app = XCUIApplication()
        app.launchEnvironment["NAVREE_ORIGIN"] = origin
        app.launchArguments = ["-NavreeSelect", "/inbox"]
        app.launch()
        try firstConversation(in: app).tap()
        let reply = app.descendants(matching: .any)
            .matching(NSPredicate(format: "placeholderValue BEGINSWITH %@", "Write a reply")).firstMatch
        XCTAssertTrue(reply.waitForExistence(timeout: 30), "The conversation's reply did not appear")
        settle(3)
        Shots.save("reply-controls", in: self)
        let saved = app.buttons["Saved replies"].firstMatch
        let send = app.buttons["Send"].firstMatch
        XCTAssertTrue(saved.exists, "Saved replies is not there")
        var frames = "reply \(reply.frame)\nSaved replies \(saved.frame)\nSend \(send.exists ? "\(send.frame)" : "none")\n"
        for name in ["Reply", "Internal note", "Snooze", "Close", "Mark as read"] {
            let control = app.buttons[name].firstMatch
            frames += "\(name): \(control.exists ? "\(control.frame)" : "none")\n"
        }
        // The system's own audit of hit regions: each element it finds too small.
        var small: [String] = []
        try app.performAccessibilityAudit(for: .hitRegion) { issue in
            small.append("\(issue.element?.label ?? "?") \(issue.element.map { "\($0.frame)" } ?? ""): \(issue.compactDescription)")
            return true
        }
        frames += "hit region audit: \(small.isEmpty ? "nothing too small" : small.joined(separator: "; "))\n"
        Shots.note("reply-controls", frames)
        XCTAssertGreaterThanOrEqual(saved.frame.height, 44, "Saved replies' target is under 44 points tall")
        if send.exists {
            XCTAssertGreaterThanOrEqual(send.frame.height, 44, "Send's target is under 44 points tall")
            XCTAssertGreaterThanOrEqual(send.frame.width, 44, "Send's target is under 44 points wide")
        }
        // Words written: the send button is the tint's, as Messages' is.
        reply.tap()
        reply.typeText("Thank you for writing to us.")
        settle(2)
        let written = app.buttons["Send"].firstMatch
        let state = written.waitForExistence(timeout: 2) ? "\(written.frame), enabled \(written.isEnabled)" : "none"
        // The field is found by its words now: its placeholder has gone.
        let field = app.descendants(matching: .any).matching(NSPredicate(format: "value BEGINSWITH %@", "Thank you")).firstMatch
        frames += "after writing: reply \(field.exists ? "\(field.frame)" : "none"), Send \(state)\n"
        Shots.note("reply-controls", frames)
        Shots.save("reply-written", in: self)
        // Eighteen points above the middle of the words, which stand 20.5
        // points tall: inside a 44-point target, outside the words alone.
        let edge = saved.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            .withOffset(CGVector(dx: 0, dy: -18))
        frames += "tapped at \(edge.screenPoint)\n"
        edge.tap()
        let first = app.buttons["Looking into it"].firstMatch
        let shown = first.waitForExistence(timeout: 10)
        frames += "saved replies opened: \(shown)\n"
        Shots.note("reply-controls", frames)
        Shots.save("reply-saved-replies-opened", in: self)
        XCTAssertTrue(shown, "A tap above the words of Saved replies, inside a 44-point target, did not open the saved replies")
    }

    /// A bordered button in the content is the system's regular size, as
    /// Mail's are, and still takes a tap across a target of 44 points: its
    /// frame, what VoiceOver outlines, is the 44-point target round a capsule
    /// about 34 points tall (the large capsule was 50.33), and a tap 20
    /// points above the middle of the conversation's "Mark as read" (or,
    /// once it is read, its "Close"), outside the capsule and inside the
    /// target, does what the button does. The frames of the conversation's
    /// bordered buttons are noted beside the pictures.
    @MainActor
    func test04BorderedButtonsAreTheRegularSizeAndTakeATapAcrossTheirTarget() throws {
        guard let origin = environment["NAVREE_INBOX_ORIGIN"], !origin.isEmpty else {
            throw XCTSkip("Only with NAVREE_INBOX_ORIGIN")
        }
        let app = XCUIApplication()
        app.launchEnvironment["NAVREE_ORIGIN"] = origin
        app.launchArguments = ["-NavreeSelect", "/inbox"]
        app.launch()
        try firstConversation(in: app).tap()
        let close = app.buttons["Close"].firstMatch
        XCTAssertTrue(close.waitForExistence(timeout: 30), "The conversation's buttons did not appear")
        settle(3)
        Shots.save("bordered-buttons", in: self)
        var frames = ""
        for name in ["Snooze", "Close", "Mark as read"] {
            let control = app.buttons[name].firstMatch
            frames += "\(name): \(control.exists ? "\(control.frame)" : "none")\n"
        }
        Shots.note("bordered-buttons", frames)
        let read = app.buttons["Mark as read"].firstMatch
        let name = read.exists ? "Mark as read" : "Close"
        let button = read.exists ? read : close
        XCTAssertGreaterThanOrEqual(button.frame.height, 44, "A bordered button's target is under 44 points tall")
        XCTAssertLessThan(button.frame.height, 50, "A bordered button is still at the large size")
        let above = button.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).withOffset(CGVector(dx: 0, dy: -20))
        frames += "tapped \(name) at \(above.screenPoint), its target from y \(button.frame.minY)\n"
        above.tap()
        // What the button does: it goes (the conversation is read or closed),
        // or the question it asks first comes up.
        var acted = false
        for _ in 0..<30 where !acted {
            acted = !button.exists || app.alerts.count > 0 || app.sheets.count > 0
            if !acted { settle(0.5) }
        }
        frames += "\(name) acted: \(acted) (gone \(!button.exists), a question \(app.alerts.count + app.sheets.count > 0))\n"
        Shots.note("bordered-buttons", frames)
        Shots.save("bordered-buttons-tapped", in: self)
        XCTAssertTrue(acted, "A tap 20 points above the middle of a bordered button, inside its 44-point target, did nothing")
    }

    /// The first conversation the list shows, whichever it is now: the
    /// first wide button under the list's filters.
    @MainActor
    private func firstConversation(in app: XCUIApplication) throws -> XCUIElement {
        let filter = app.buttons["Filter"].firstMatch
        XCTAssertTrue(filter.waitForExistence(timeout: 90), "The conversations did not appear")
        settle(2)
        let row = app.buttons.allElementsBoundByIndex.first { button in
            button.frame.minY > filter.frame.maxY + 40 && button.frame.width > 200 && button.isHittable
        }
        return try XCTUnwrap(row, "No conversation's row under the filters")
    }

    @MainActor
    private func settle(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }
}
