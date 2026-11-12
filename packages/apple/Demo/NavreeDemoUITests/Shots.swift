import XCTest

/// Saves what the simulator shows: as an attachment of the test, and as a PNG
/// in the folder named by `NAVREE_SHOTS` when the run sets it (`xcodebuild`
/// passes `TEST_RUNNER_NAVREE_SHOTS` to the test as `NAVREE_SHOTS`).
@MainActor
enum Shots {
    static func save(_ name: String, in test: XCTestCase) {
        let shot = XCUIScreen.main.screenshot()
        let attachment = XCTAttachment(screenshot: shot)
        attachment.name = name
        attachment.lifetime = .keepAlways
        test.add(attachment)
        guard let folder = ProcessInfo.processInfo.environment["NAVREE_SHOTS"], !folder.isEmpty else {
            return
        }
        let directory = URL(fileURLWithPath: folder, isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try? shot.pngRepresentation.write(to: directory.appendingPathComponent("\(name).png"))
    }
}
