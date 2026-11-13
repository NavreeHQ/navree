import XCTest

/// Signing in on a live server through the system's sign-in session, handed
/// over to the app, and still signed in after the app is quit and opened
/// again (the session's cookie kept in the Keychain, this device only).
///
/// Runs only against a server that hands its sign-in over to this app on the
/// app's own scheme (`auth.apps.apple.<name>` with this bundle id,
/// `callback = "charkha:/signed-in"` and `custom_scheme = true`), named with
/// its account in the test's environment (for `xcodebuild`, each with the
/// prefix `TEST_RUNNER_`):
///
/// - `NAVREE_SIGN_IN_ORIGIN`: `http://127.0.0.1:8099`
/// - `NAVREE_SIGN_IN_EMAIL`, `NAVREE_SIGN_IN_PASSWORD`: an account of the server
/// - `NAVREE_SIGNED_IN_PAGE`, `NAVREE_SIGNED_IN_TITLE`: a page only a signed-in
///   person sees, drawn natively, and its title
///
/// Start it with the app signed out (`xcrun simctl keychain <device> reset`),
/// from a build signed to run locally (`CODE_SIGN_IDENTITY=-`): a build with
/// no signature at all has no Keychain, so nothing outlives a launch. The
/// site's sign-in form must lead to the hand-over's finish address by the
/// browser's own navigation: a page script that fetches that address spends
/// the hand-over before the session sees where it returns.
final class SignInTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private var environment: [String: String] {
        ProcessInfo.processInfo.environment
    }

    @MainActor
    private func launch(_ arguments: [String]) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["NAVREE_ORIGIN"] = environment["NAVREE_SIGN_IN_ORIGIN"]
        app.launchArguments = ["-NavreeSignInPaths", "/sign-in"] + arguments
        app.launch()
        return app
    }

    /// The first of `queries` that shows up within `timeout`. A query of
    /// another app is asked only while that app runs: asking one that does
    /// not fails the test.
    @MainActor
    private func first(_ queries: [(XCUIApplication, XCUIElement)], timeout: TimeInterval = 60) -> XCUIElement? {
        let end = Date().addingTimeInterval(timeout)
        repeat {
            for (app, query) in queries where app.state != .notRunning && app.state != .unknown {
                if query.exists { return query }
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.5))
        } while Date() < end
        return nil
    }

    /// Types into a field of a web page, and again if the keys went nowhere:
    /// the page moves while the keyboard and its bar come up, and the first
    /// keys after a field takes the focus can be lost.
    @MainActor
    private func type(_ text: String, into field: XCUIElement) {
        for _ in 0..<4 {
            field.tap()
            settle(2)
            field.typeText(text)
            settle(1)
            let value = field.value as? String ?? ""
            if value == text || (value.count == text.count && !value.isEmpty) { return }
            // Some of it went in: take it out before the next try.
            if !value.isEmpty, value != field.placeholderValue {
                field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: value.count))
            }
        }
    }

    /// Lets the screen settle before it is photographed.
    private func settle(_ seconds: TimeInterval = 3) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }

    @MainActor
    func testASignInIsHandedOverToTheAppAndKeptAcrossLaunches() throws {
        guard let origin = environment["NAVREE_SIGN_IN_ORIGIN"], !origin.isEmpty,
            let email = environment["NAVREE_SIGN_IN_EMAIL"], let password = environment["NAVREE_SIGN_IN_PASSWORD"],
            let page = environment["NAVREE_SIGNED_IN_PAGE"], let title = environment["NAVREE_SIGNED_IN_TITLE"]
        else {
            throw XCTSkip("Needs a server that hands its sign-in over to this app")
        }
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let browser = XCUIApplication(bundleIdentifier: "com.apple.SafariViewService")

        // Signed out: the page a signed-in person sees asks for a sign-in instead.
        var app = launch(["-NavreeStack", "/=\(page)"])
        XCTAssertNotNil(first([(app, app.staticTexts["Sign in"]), (app, app.webViews.staticTexts["Sign in"])]), "The sign-in page")
        settle(5)
        Shots.save("01-signed-out", in: self)
        app.terminate()

        // Sign in: the core opens the system's session at the hand-over's start,
        // and the system asks first.
        app = launch(["-NavreeSheet", "/sign-in"])
        let proceed = first([
            (app, app.alerts.buttons["Continue"]), (springboard, springboard.alerts.buttons["Continue"]),
            (app, app.buttons["Continue"]), (springboard, springboard.buttons["Continue"]),
        ])
        guard let proceed else {
            Shots.save("02-no-question", in: self)
            Shots.note("02-app", app.debugDescription)
            Shots.note("02-springboard", springboard.debugDescription)
            return XCTFail("The system's question before the session")
        }
        Shots.save("02-the-system-asks-first", in: self)
        proceed.tap()
        // The session's page is drawn by the system's browser service: its
        // own fields take the typing, the app's view of them only the taps.
        let emailQueries = [
            (browser, browser.webViews.textFields["Email"]), (browser, browser.textFields["Email"]),
            (app, app.webViews.textFields["Email"]), (app, app.textFields["Email"]),
        ]
        let emailField = try XCTUnwrap(first(emailQueries), "The email field in the system's session")
        settle()
        Shots.save("03-the-sites-sign-in-page-in-the-session", in: self)
        type(email, into: emailField)
        // The rest by the keyboard alone, as a person with one would: the tab
        // key to the password, its characters, the return key to send the form.
        let keyboard = emailField.value(forKey: "hasKeyboardFocus") as? Bool == true ? emailField : browser
        keyboard.typeText("\t")
        settle(2)
        let typing = browser.state == .notRunning ? app : browser
        typing.typeText(password)
        settle(1)
        Shots.save("04-signing-in-in-the-session", in: self)
        typing.typeText("\n")

        // Back in the app: the session has gone, the code was exchanged.
        let end = Date().addingTimeInterval(90)
        while Date() < end, first(emailQueries, timeout: 0) != nil {
            settle(1)
        }
        settle(5)
        Shots.save("05-back-in-the-app", in: self)
        app.terminate()

        // Quit and opened again: still signed in, the page drawn natively.
        app = launch(["-NavreeStack", "/=\(page)"])
        XCTAssertTrue(app.navigationBars[title].waitForExistence(timeout: 90), "The signed-in page")
        settle(5)
        Shots.save("06-signed-in-after-a-relaunch", in: self)
        Shots.note("06-signed-in-after-a-relaunch", app.debugDescription)
    }
}
