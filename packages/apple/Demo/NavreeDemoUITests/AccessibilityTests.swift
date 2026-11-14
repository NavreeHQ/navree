import XCTest

/// The accessibility tree VoiceOver reads, and Apple's accessibility audit, on
/// the main screens of the store, inbox and board demos and of an app whose
/// answers are read from a folder.
///
/// On each screen the test runs `performAccessibilityAudit()` and writes a
/// listing: the audit's issues; the screen's accessibility elements in the
/// order of the tree XCUITest reads (type, label, value, identifier, state);
/// and, where XCUITest can drive VoiceOver (iOS 27 and later), what VoiceOver
/// speaks for each element as it moves forward through the screen, which
/// carries the traits and hints the tree does not. Each audit issue is a
/// failure of the test, and the walk goes on to the next screen. The listing
/// is an attachment of the test, and a text file beside the screen's picture
/// when `NAVREE_SHOTS` names a folder.
///
/// Runs only when asked, in the test's environment (for `xcodebuild`, each
/// with the prefix `TEST_RUNNER_`):
///
/// - `NAVREE_ACCESSIBILITY=1`: run at all.
/// - `NAVREE_STORE_ORIGIN`: a store demo (`http://127.0.0.1:8095`); without
///   it, the store's recorded answers.
/// - `NAVREE_INBOX_ORIGIN`, `NAVREE_BOARD_ORIGIN`: the inbox and board demos;
///   each walk is skipped without its origin.
/// - `NAVREE_ANSWERS_FOLDER`: a folder of recorded answers (as the demo's
///   `-NavreeAnswers`), and `NAVREE_ANSWERS_SCREENS`, its screens to walk,
///   separated by `;`: a destination's address, or `destination=address` for
///   an address pushed on it. Skipped without the folder.
/// - `NAVREE_VOICEOVER=0`: leave VoiceOver off.
/// - `NAVREE_VOICEOVER_STEPS`: how many elements VoiceOver moves through on
///   one screen at most (80).
/// - `NAVREE_SCROLL_SHOTS`: on the store's day and the folder's screens, how
///   many more pictures to take, each a swipe further down (0).
final class AccessibilityTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = true
        try XCTSkipUnless(environment["NAVREE_ACCESSIBILITY"] == "1", "Only when NAVREE_ACCESSIBILITY=1")
    }

    private var environment: [String: String] {
        ProcessInfo.processInfo.environment
    }

    // MARK: The store

    @MainActor
    func test01StoreOrdersAnOrderAndItsSheet() throws {
        let app = launchStore(["-NavreeSelect", "/orders"])
        wait(app.navigationBars["Orders"], "The orders screen")
        let orders = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "CS-"))
        wait(orders.firstMatch, "An order in the list")
        check(app, "store-01-orders")

        // A launch of its own for the tap: VoiceOver has moved through the list.
        let again = launchStore(["-NavreeSelect", "/orders"])
        let first = again.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "CS-"))
        wait(first.firstMatch, "An order in the list")
        try tapFirst(first, "An order in the list")
        // The sheet's leading control: Close for a glance, Cancel for a task.
        let leading = again.buttons.matching(NSPredicate(format: "label IN %@", ["Close", "Cancel"])).firstMatch
        wait(leading, "The order's sheet")
        settle(again)
        check(again, "store-02-order-sheet")

        let pushed = launchStore(["-NavreeSelect", "/orders", "-NavreeStack", "/orders=\(orderAddress)"])
        wait(pushed.buttons["BackButton"].firstMatch, "The order, pushed")
        check(pushed, "store-03-order")
    }

    @MainActor
    func test02StoreInventoryAndRestock() throws {
        let app = launchStore(["-NavreeSelect", "/inventory"])
        wait(app.navigationBars["Inventory"], "The inventory")
        wait(app.cells.firstMatch, "A product in the list")
        check(app, "store-04-inventory")

        let again = launchStore(["-NavreeSelect", "/inventory"])
        wait(again.cells.firstMatch, "A product in the list")
        try tapFirst(again.cells, "A product in the list")
        wait(again.buttons["Cancel"].firstMatch, "The restock sheet")
        settle(again)
        check(again, "store-05-restock")
    }

    @MainActor
    func test03StoreToday() throws {
        let app = launchStore(["-NavreeSelect", "/"])
        wait(app.navigationBars.firstMatch, "The store's day")
        settle(app)
        check(app, "store-06-today", scrolls: scrolls)
    }

    // MARK: The inbox and the board

    @MainActor
    func test04InboxListAndThread() throws {
        let origin = try origin("NAVREE_INBOX_ORIGIN")
        let app = launch(origin: origin, [])
        wait(app.cells.firstMatch, "A conversation in the list")
        settle(app)
        check(app, "inbox-01-list")

        let again = launch(origin: origin, [])
        wait(again.cells.firstMatch, "A conversation in the list")
        try tapFirst(again.cells, "A conversation in the list")
        wait(again.buttons["BackButton"].firstMatch, "The conversation")
        settle(again)
        check(again, "inbox-02-thread")
    }

    @MainActor
    func test05Board() throws {
        let origin = try origin("NAVREE_BOARD_ORIGIN")
        let app = launch(origin: origin, [])
        wait(app.cells.firstMatch, "A card on the board")
        settle(app)
        check(app, "board-01-board")
    }

    // MARK: An app read from a folder of answers

    @MainActor
    func test06Folder() throws {
        let folder = try XCTUnwrap(
            environment["NAVREE_ANSWERS_FOLDER"].flatMap { $0.isEmpty ? nil : $0 },
            "Skipped"
        )
        let screens = (environment["NAVREE_ANSWERS_SCREENS"] ?? "/").split(separator: ";").map(String.init)
        for (index, screen) in screens.enumerated() {
            let parts = screen.split(separator: "=", maxSplits: 1).map(String.init)
            var arguments = ["-NavreeAnswers", folder, "-NavreeSelect", parts[0]]
            if parts.count == 2 {
                arguments += ["-NavreeStack", screen]
            }
            let app = XCUIApplication()
            app.launchArguments = arguments
            app.launch()
            wait(app.navigationBars.firstMatch, "The screen \(screen)")
            settle(app)
            let name = screen == "/" ? "home" : screen.replacingOccurrences(of: "/", with: "-")
                .replacingOccurrences(of: "=", with: "-").replacingOccurrences(of: "?", with: "-")
                .trimmingCharacters(in: CharacterSet(charactersIn: "-"))
            check(app, String(format: "folder-%02d-%@", index + 1, name), scrolls: scrolls)
        }
    }

    // MARK: Launching

    private var scrolls: Int {
        Int(environment["NAVREE_SCROLL_SHOTS"] ?? "") ?? 0
    }

    /// The order pushed in the store's walk: one the store demo seeds, or the
    /// recorded one.
    private var orderAddress: String {
        environment["NAVREE_STORE_ORIGIN"].map { _ in environment["NAVREE_STORE_ORDER"] ?? "/orders/200000" } ?? "/orders/1"
    }

    @MainActor
    private func launchStore(_ arguments: [String]) -> XCUIApplication {
        if let origin = environment["NAVREE_STORE_ORIGIN"], !origin.isEmpty {
            return launch(origin: origin, arguments)
        }
        let app = XCUIApplication()
        app.launchArguments = ["-NavreeRecorded", "YES"] + arguments
        app.launch()
        return app
    }

    @MainActor
    private func launch(origin: String, _ arguments: [String]) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["NAVREE_ORIGIN"] = origin
        app.launchArguments = arguments
        app.launch()
        return app
    }

    private func origin(_ name: String) throws -> String {
        guard let origin = environment[name], !origin.isEmpty else {
            throw XCTSkip("Only with \(name)")
        }
        return origin
    }

    @MainActor
    private func wait(_ element: XCUIElement, _ what: String, timeout: TimeInterval = 45) {
        XCTAssertTrue(element.waitForExistence(timeout: timeout), "\(what) did not appear")
    }

    /// Taps the first of `query` a finger could reach: VoiceOver moving
    /// through a list scrolls it, so the first may be under the bar.
    @MainActor
    private func tapFirst(_ query: XCUIElementQuery, _ what: String) throws {
        let end = Date().addingTimeInterval(20)
        repeat {
            for index in 0..<min(query.count, 40) {
                let element = query.element(boundBy: index)
                if element.exists, element.isHittable {
                    return element.tap()
                }
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.5))
        } while Date() < end
        throw XCTSkip("\(what): none could be tapped")
    }

    /// Waits for what arrives after the screen's first part: charts, words
    /// that roll as they change.
    @MainActor
    private func settle(_ app: XCUIApplication, seconds: TimeInterval = 3) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }

    // MARK: The audit and the listing

    /// Runs Apple's audit on the screen in front, lists what VoiceOver reads
    /// there, keeps both with a picture, and fails once for each issue.
    @MainActor
    private func check(_ app: XCUIApplication, _ name: String, scrolls: Int = 0) {
        Shots.save(name, in: self)
        var text = "# \(name)\n\n## Apple's accessibility audit (performAccessibilityAudit)\n\n"
        var issues: [XCUIAccessibilityAuditIssue] = []
        // All types at once; when that does not finish in time, each type
        // on its own, so a slow check costs only itself.
        var unfinished: [XCUIAccessibilityAuditType] = []
        if audit(app, .all, into: &issues) != nil {
            for type in Self.types where audit(app, type, into: &issues) != nil {
                unfinished.append(type)
            }
            text += "All types at once did not finish in time; each type was run on its own.\n"
        }
        for type in unfinished {
            text += "- \(Self.name(of: type)): did not finish in time\n"
        }
        if issues.isEmpty, unfinished.count < Self.types.count {
            text += "No issues.\n"
        }
        for issue in issues {
            let line = "\(Self.name(of: issue.auditType)): \(issue.compactDescription) [\(Self.describe(issue.element))]"
            text += "- \(line)\n  \(issue.detailedDescription.replacingOccurrences(of: "\n", with: " "))\n"
            XCTFail("\(name): \(line)")
        }

        text += "\n## The accessibility elements, in the order of the tree XCUITest reads\n\n"
        if let snapshot = try? app.snapshot() {
            var lines: [String] = []
            Self.list(snapshot, into: &lines)
            text += lines.joined(separator: "\n") + "\n"
        } else {
            text += "The tree could not be read.\n"
        }

        // The rest of a long screen, a swipe at a time, and back to its top.
        if scrolls > 0 {
            for index in 1...scrolls {
                app.swipeUp()
                settle(app, seconds: 1.5)
                Shots.save("\(name)-scrolled-\(index)", in: self)
            }
            for _ in 1...scrolls {
                app.swipeDown()
            }
            settle(app, seconds: 1)
        }

        text += "\n## What VoiceOver speaks, moving forward from the first element\n\n"
        text += voiceOver(app)

        let attachment = XCTAttachment(string: text)
        attachment.name = "\(name).txt"
        attachment.lifetime = .keepAlways
        add(attachment)
        Shots.note(name, text)
    }

    /// One audit; its error when it did not run.
    @MainActor
    private func audit(
        _ app: XCUIApplication, _ types: XCUIAccessibilityAuditType, into issues: inout [XCUIAccessibilityAuditIssue]
    ) -> Error? {
        var found: [XCUIAccessibilityAuditIssue] = []
        do {
            try app.performAccessibilityAudit(for: types) { issue in
                found.append(issue)
                return true
            }
        } catch {
            return error
        }
        issues += found
        return nil
    }

    private static let types: [XCUIAccessibilityAuditType] = [
        .contrast, .elementDetection, .hitRegion, .sufficientElementDescription, .dynamicType, .textClipped, .trait,
    ]

    /// VoiceOver's words for each element in turn, through XCUITest's own
    /// VoiceOver service; the reason when it cannot run.
    @MainActor
    private func voiceOver(_ app: XCUIApplication) -> String {
        guard environment["NAVREE_VOICEOVER"] != "0" else { return "Left off (NAVREE_VOICEOVER=0).\n" }
        #if compiler(>=6.4)
        if #available(iOS 27.0, *) {
            let service = XCUIDevice.shared.voiceOverService
            do {
                try service.enable()
            } catch {
                return "VoiceOver did not start: \(error)\n"
            }
            defer { try? service.disable() }
            let limit = Int(environment["NAVREE_VOICEOVER_STEPS"] ?? "") ?? 80
            var lines: [String] = []
            // VoiceOver's own "VoiceOver on" is not the screen's.
            let own = ["VoiceOver on"]
            var last = ""
            if let first = try? service.currentSpeech(), !own.contains(first.utterance) {
                lines.append("1. \(first.utterance)")
                last = first.utterance
            }
            var same = 0
            for _ in 0..<limit {
                let output: XCUIVoiceOverService.Output
                do {
                    output = try service.moveForward()
                } catch {
                    lines.append("(stopped: \(error.localizedDescription))")
                    break
                }
                if own.contains(output.utterance) { continue }
                if output.utterance == last {
                    same += 1
                    if same >= 2 { break }
                    continue
                }
                same = 0
                last = output.utterance
                lines.append("\(lines.count + 1). \(output.utterance)")
            }
            return lines.joined(separator: "\n") + "\n"
        }
        #endif
        return "Not available: XCUITest drives VoiceOver from iOS 27 with Xcode 27.\n"
    }

    /// The elements VoiceOver would stop at, in tree order: a node with words
    /// or a control, without the words of the nodes it already speaks for.
    @MainActor
    private static func list(_ node: XCUIElementSnapshot, depth: Int = 0, into lines: inout [String]) {
        let containers: Set<XCUIElement.ElementType> = [
            .application, .window, .other, .cell, .collectionView, .table, .scrollView, .navigationBar, .tabBar,
            .toolbar, .group, .layoutArea, .layoutItem, .sheet, .webView, .outline, .outlineRow,
        ]
        let words = !node.label.isEmpty || !(node.value.map { "\($0)" } ?? "").isEmpty
        let control: Set<XCUIElement.ElementType> = [.button, .textField, .secureTextField, .switch, .slider, .segmentedControl, .link, .searchField, .picker, .datePicker, .textView, .menuButton, .toggle, .stepper]
        if containers.contains(node.elementType) {
            if words || !node.identifier.isEmpty {
                lines.append(String(repeating: "  ", count: min(depth, 1)) + "[\(describe(node))]")
            }
            for child in node.children {
                list(child, depth: depth + 1, into: &lines)
            }
            return
        }
        if words || control.contains(node.elementType) {
            lines.append("- " + describe(node))
            return
        }
        for child in node.children {
            list(child, depth: depth + 1, into: &lines)
        }
    }

    @MainActor
    private static func describe(_ node: XCUIElementSnapshot) -> String {
        var parts = [type(node.elementType)]
        if !node.label.isEmpty { parts.append("label \"\(node.label)\"") }
        if let value = node.value.map({ "\($0)" }), !value.isEmpty { parts.append("value \"\(value)\"") }
        if !node.identifier.isEmpty { parts.append("id \(node.identifier)") }
        if let placeholder = node.placeholderValue, !placeholder.isEmpty { parts.append("placeholder \"\(placeholder)\"") }
        if node.isSelected { parts.append("selected") }
        if !node.isEnabled { parts.append("dimmed") }
        let frame = node.frame
        parts.append(String(format: "at %.0f,%.0f size %.0f×%.0f", frame.minX, frame.minY, frame.width, frame.height))
        return parts.joined(separator: ", ")
    }

    @MainActor
    private static func describe(_ element: XCUIElement?) -> String {
        guard let element, element.exists else { return "no element" }
        var parts = [type(element.elementType)]
        if !element.label.isEmpty { parts.append("\"\(element.label)\"") }
        if !element.identifier.isEmpty { parts.append("id \(element.identifier)") }
        let frame = element.frame
        parts.append(String(format: "at %.0f,%.0f size %.0f×%.0f", frame.minX, frame.minY, frame.width, frame.height))
        return parts.joined(separator: ", ")
    }

    private static func type(_ type: XCUIElement.ElementType) -> String {
        switch type {
        case .button: "button"
        case .staticText: "text"
        case .image: "image"
        case .cell: "cell"
        case .textField: "text field"
        case .searchField: "search field"
        case .switch, .toggle: "switch"
        case .link: "link"
        case .navigationBar: "navigation bar"
        case .tabBar: "tab bar"
        case .toolbar: "toolbar"
        case .segmentedControl: "segmented control"
        case .slider: "slider"
        case .progressIndicator: "progress"
        case .activityIndicator: "spinner"
        case .other: "group"
        case .collectionView: "collection"
        case .table: "table"
        case .scrollView: "scroll view"
        case .webView: "web view"
        case .menuButton: "menu button"
        case .picker, .datePicker: "picker"
        case .stepper: "stepper"
        case .sheet: "sheet"
        case .window: "window"
        case .application: "application"
        default: "element \(type.rawValue)"
        }
    }

    private static func name(of type: XCUIAccessibilityAuditType) -> String {
        switch type {
        case .contrast: "Contrast"
        case .elementDetection: "Element detection"
        case .hitRegion: "Hit region"
        case .sufficientElementDescription: "Element description"
        case .dynamicType: "Dynamic Type"
        case .textClipped: "Text clipped"
        case .trait: "Trait"
        default: "Audit \(type.rawValue)"
        }
    }
}
