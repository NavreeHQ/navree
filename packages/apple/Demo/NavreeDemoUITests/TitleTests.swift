import XCTest

/// A destination whose list stands under a search field opens with its title
/// large, launch after launch: also when its list is kept from the last
/// launch and is there from the bar's very first layout.
///
/// Runs only when asked, in the test's environment (for `xcodebuild`, each
/// with the prefix `TEST_RUNNER_`):
///
/// - `NAVREE_LAUNCHES`: how many launches for each screen (12).
/// - `NAVREE_LAUNCH_ORIGIN`: the server (`http://127.0.0.1:8095`); the
///   recorded answers when it is not given.
///
/// Each launch is pictured and its title's frame noted, in the folder
/// `NAVREE_SHOTS` names, as the other tests' pictures.
final class TitleTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = true
        try XCTSkipUnless(Int(environment["NAVREE_LAUNCHES"] ?? "") != nil, "Only with NAVREE_LAUNCHES")
    }

    private var environment: [String: String] {
        ProcessInfo.processInfo.environment
    }

    @MainActor
    func testOrdersOpensWithItsTitleLargeEveryLaunch() {
        launches(of: "/orders", titled: "Orders", name: "orders")
    }

    @MainActor
    func testInventoryOpensWithItsTitleLargeEveryLaunch() {
        launches(of: "/inventory", titled: "Inventory", name: "inventory")
    }

    /// Launches the app on the destination again and again, and finds its
    /// title large each time: as tall as the large title's type, under the
    /// bar's buttons, never the inline title beside them.
    @MainActor
    private func launches(of destination: String, titled title: String, name: String) {
        let count = Int(environment["NAVREE_LAUNCHES"] ?? "") ?? 12
        var notes: [String] = []
        var large = 0
        for launch in 1...count {
            let app = XCUIApplication()
            if let origin = environment["NAVREE_LAUNCH_ORIGIN"], !origin.isEmpty {
                app.launchEnvironment["NAVREE_ORIGIN"] = origin
            } else {
                app.launchArguments = ["-NavreeRecorded", "YES"]
            }
            app.launchArguments += ["-NavreeSelect", destination]
            app.launch()
            let bar = app.navigationBars[title]
            XCTAssertTrue(bar.waitForExistence(timeout: 60), "\(title) did not appear at launch \(launch)")
            XCTAssertTrue(app.cells.firstMatch.waitForExistence(timeout: 60), "\(title)'s list did not appear at launch \(launch)")
            RunLoop.current.run(until: Date().addingTimeInterval(2))
            let frames = bar.staticTexts.matching(identifier: title).allElementsBoundByIndex.map(\.frame)
            // The large title's type is 34 points (41 tall); the inline title's 17.
            let isLarge = frames.contains { $0.height >= 30 }
            if isLarge { large += 1 }
            notes.append("launch \(launch): title \(frames), bar \(bar.frame), \(isLarge ? "large" : "inline")")
            Shots.save("\(name)-launch-\(launch)", in: self)
        }
        notes.append("\(large) of \(count) large")
        Shots.note("\(name)-launches", notes.joined(separator: "\n"))
        XCTAssertEqual(large, count, "\(title) opened with its title folded: \(notes.joined(separator: "; "))")
    }
}
