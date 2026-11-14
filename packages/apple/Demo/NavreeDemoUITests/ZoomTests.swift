import UIKit
import XCTest

/// A card that grows into the screen it opens, and the screen that shrinks
/// back into the card when the person goes back, with Reduce Motion off and
/// on (then the screen comes in as any pushed screen does). The answers are
/// the repository's own examples, a designed Today whose pillars are cards
/// that zoom, and the Sleep screen one of them opens, served from a folder the
/// test writes, so no server is needed.
///
/// Runs only when asked, in the test's environment (for `xcodebuild`, each
/// with the prefix `TEST_RUNNER_`):
///
/// - `NAVREE_ZOOM=1`: run at all. Reduce Motion is the simulator's own
///   setting: the run sets it before, and the test notes what it found.
/// - `NAVREE_ZOOM_NAME`: the pictures' name (`zoom`).
/// - `NAVREE_ZOOM_SLOW`: how many times slower the system's animations run
///   (UIKit's drag coefficient, 1 by default), so the pictures taken during
///   the transition catch it halfway.
///
/// Pictures go to the folder `NAVREE_SHOTS` names, as the other tests'.
final class ZoomTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = true
        try XCTSkipUnless(environment["NAVREE_ZOOM"] == "1", "Only when NAVREE_ZOOM=1")
    }

    private var environment: [String: String] {
        ProcessInfo.processInfo.environment
    }

    /// The Sleep card opens Sleep, and back shows Today with the card where it was.
    @MainActor
    func test01ACardOpensItsScreenAndBackShowsTheCard() throws {
        let name = environment["NAVREE_ZOOM_NAME"] ?? "zoom"
        let reduced = UIAccessibility.isReduceMotionEnabled
        let app = XCUIApplication()
        app.launchEnvironment["NAVREE_ANSWERS"] = try Self.answers().path
        if let slow = environment["NAVREE_ZOOM_SLOW"], !slow.isEmpty {
            app.launchArguments += ["-UIAnimationDragCoefficient", slow]
        }
        app.launch()
        let card = app.buttons["Sleep, 25% of score"].firstMatch
        XCTAssertTrue(card.waitForExistence(timeout: 90), "The Sleep card did not appear")
        settle(3)
        let before = card.frame
        Shots.save("\(name)-0-today", in: self)
        var notes = "Reduce Motion: \(reduced ? "on" : "off")\ncard before: \(before)\n"

        card.tap()
        for shot in 1...3 {
            Shots.save("\(name)-1-opening-\(shot)", in: self)
        }
        let sleep = app.navigationBars["Sleep"].firstMatch
        XCTAssertTrue(sleep.waitForExistence(timeout: 30), "Sleep did not open from its card")
        settle(2)
        Shots.save("\(name)-2-sleep", in: self)
        notes += "Sleep's bar: \(sleep.frame)\n"

        let back = sleep.buttons.firstMatch
        XCTAssertTrue(back.exists, "Sleep has no back button")
        back.tap()
        for shot in 1...3 {
            Shots.save("\(name)-3-closing-\(shot)", in: self)
        }
        XCTAssertTrue(card.waitForExistence(timeout: 30), "Back did not show Today's Sleep card")
        settle(2)
        Shots.save("\(name)-4-back", in: self)
        notes += "card after: \(card.frame)\nSleep's bar after: \(sleep.exists ? "still there" : "gone")\n"
        Shots.note("\(name)-frames", notes)
        XCTAssertTrue(card.isHittable, "The Sleep card is not back where a finger can tap it")
        XCTAssertEqual(card.frame, before, "The Sleep card did not come back to where it was")
        XCTAssertFalse(sleep.exists, "Sleep's bar is still there after going back")
    }

    /// The folder of answers: the designed Today at `/` and Sleep at
    /// `/sleep`, from the repository's examples, with the manifest that
    /// says which address each answers.
    static func answers() throws -> URL {
        let repository = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // NavreeDemoUITests
            .deletingLastPathComponent()  // Demo
            .deletingLastPathComponent()  // apple
            .deletingLastPathComponent()  // packages
            .deletingLastPathComponent()
        let made = "crates/navree-native-core/transcripts/fixtures/made/"
        let rows = [
            ("shell.json", made + "shell-health.json", "/_navree/native/shell"),
            ("today.json", "crates/navree-native/corpus/screen-designed-health.json", "/"),
            ("sleep.json", made + "designed-sleep.json", "/sleep"),
        ]
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-zoom-answers")
        try? FileManager.default.removeItem(at: folder)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var manifest = "file\tstatus\turl\n"
        for (file, source, address) in rows {
            let data = try Data(contentsOf: repository.appendingPathComponent(source))
            try data.write(to: folder.appendingPathComponent(file))
            manifest += "\(file)\t200\t\(address)\n"
        }
        try Data(manifest.utf8).write(to: folder.appendingPathComponent("manifest.tsv"))
        return folder
    }

    @MainActor
    private func settle(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }
}
