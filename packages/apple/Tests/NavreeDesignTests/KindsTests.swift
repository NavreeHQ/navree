import Foundation
import NavreeDesign
import NavreeProtocol
import Testing
import UniformTypeIdentifiers

// The kinds the inbox and the store's day now send: panes, popovers,
// messages, an activity feed, a span of days and a file.

@Test func aCompactWidthShowsTheCurrentPaneAndARegularOneTheListBesideIt() throws {
    let thread = try answerScreen("inbox-thread")
    let panes = PaneRules.panes(in: thread.body)
    #expect(panes.map(\.id) == ["conversations", "thread", "customer"])
    let compact = PaneRules.layout(panes, width: .compact)
    #expect(compact.shown.map(\.id) == ["thread"])
    #expect(compact.inspector?.id == "customer")
    let regular = PaneRules.layout(panes, width: .regular)
    #expect(regular.shown.map(\.id) == ["conversations", "thread"])
    #expect(regular.inspector?.id == "customer")
    // On the list's own screen the list is the current pane.
    let inbox = PaneRules.layout(PaneRules.panes(in: try answerScreen("inbox-inbox").body), width: .compact)
    #expect(inbox.shown.map(\.id) == ["conversations"])
}

@Test func aPopoverStandsByTheControlThatOpensIt() throws {
    let thread = try answerScreen("inbox-thread")
    let popovers = PopoverRules.popovers(in: thread.body)
    #expect(Set(popovers.keys) == ["assign-popover", "snooze-popover", "saved-replies"])
    for id in popovers.keys {
        #expect(PopoverRules.anchored(id, in: thread.body, actions: thread.actions), "\(id)")
    }
    #expect(!PopoverRules.anchored("nowhere", in: thread.body))
    let open = Navree.Action.commands(Navree.Action.Commands(list: [.open(Navree.Command.Open(id: "saved-replies"))]))
    #expect(PopoverRules.opens(open) == "saved-replies")
}

@Test func aMessageSaysHowFarItGotInWordsWithASymbol() {
    #expect(MessageRules.delivery(.failed)?.text == "Not Delivered")
    #expect(MessageRules.delivery(.read)?.symbol == "checkmark.circle")
    #expect(MessageRules.noteWords == "Internal Note")
}

@Test func theStoresDayHasAnActivityFeedThatOpensEachOrder() throws {
    let root = try answer("store-root-day")
    var rows: [Navree.FeedRow] = []
    root.screen?.body.walk { node in
        if case .feedRow(let row) = node { rows.append(row) }
    }
    #expect(rows.count > 10)
    guard case .navigate(let link)? = rows.first?.action else {
        Issue.record("a feed row opens nothing")
        return
    }
    #expect(link.href.hasPrefix("/orders/"))
}

@Test func aSpanNeverRunsBackwards() {
    #expect(RangeRules.values(from: "2026-11-10", to: "2026-11-14", kind: .date) == ["2026-11-10", "2026-11-14"])
    #expect(RangeRules.values(from: "2026-11-14", to: "2026-11-10", kind: .date) == ["2026-11-14", "2026-11-14"])
    #expect(RangeRules.values(from: "", to: "2026-11-10", kind: .date) == ["", "2026-11-10"])
    let range = Navree.Input.range(Navree.RangeInput(
        from: Navree.RangeEnd(name: "from", value: "2026-11-01"), to: Navree.RangeEnd(name: "to", value: "2026-11-07")
    ))
    #expect(range.initialValues == ["2026-11-01", "2026-11-07"])
}

@Test func aFileFieldOffersTheTypesItTakes() {
    #expect(FileRules.types("image/*") == [.image])
    #expect(FileRules.types("application/pdf, .csv") == [.pdf, .commaSeparatedText])
    #expect(FileRules.types(nil) == [.item])
    #expect(FileRules.types("x-unknown/thing") == [.item])
}

@Test func onlyALinkThatPushesShowsTheDisclosureChevron() throws {
    let routes = [Navree.Route(match: "/*/new", present: .sheet), Navree.Route(match: "/sign-in", present: .full)]
    func link(_ href: String, _ present: Navree.Present = .auto) -> Navree.Action {
        .navigate(Navree.Action.Navigate(href: href, present: present))
    }
    #expect(NavigationRules.pushes(link("/orders/1"), routes: routes))
    #expect(NavigationRules.pushes(link("/orders/1", .push), routes: routes))
    // The shell's routes present this address as a sheet: no chevron.
    #expect(!NavigationRules.pushes(link("/orders/new?from=today"), routes: routes))
    #expect(NavigationRules.pushes(link("/orders/new", .push), routes: routes))
    #expect(!NavigationRules.pushes(link("/orders/1", .sheet), routes: routes))
    #expect(!NavigationRules.pushes(link("/orders/1", .task), routes: routes))
    #expect(!NavigationRules.pushes(.request(Navree.Action.Request(href: "/orders/1/peek")), routes: routes))
    #expect(NavigationRules.matches("/*/new", "https://shop.example/orders/new#top"))
    #expect(!NavigationRules.matches("/*/new", "/orders/new/items"))
}

@Test func aChartsValuesAreWrittenAsThePageWritesThemNeverAsAComputerDoes() throws {
    let rupees = ValueWords.style(of: "₹31,55,138.50")
    #expect(rupees == ValueStyle(prefix: "₹", indian: true))
    #expect(ValueWords.text(4_000_000, rupees) == "₹40,00,000")
    #expect(ValueWords.text(125_000, rupees) == "₹1,25,000")
    #expect(ValueWords.text(0, rupees) == "₹0")
    let kcal = ValueWords.style(of: "1,234 kcal")
    #expect(ValueWords.text(2_500, kcal) == "2,500 kcal")
    #expect(ValueWords.text(7.5, ValueWords.style(of: "7.2 h")) == "7.5 h")
    // The store's day: revenue by day, in rupees.
    var revenue: Navree.Chart?
    try answer("store-root-day").screen?.body.walk { node in
        if case .chart(let chart) = node, chart.kind == .area { revenue = chart }
    }
    let chart = try #require(revenue)
    #expect(ValueWords.text(4_000_000, ValueWords.style(chart)) == "₹40,00,000")
}

@Test func aDurationsAxisReadsInTheUnitTheChartNamesNotInTheLastWordOfItsValues() {
    #expect(ValueWords.compound("7 h 6 min"))
    #expect(ValueWords.compound("7:15 am"))
    #expect(!ValueWords.compound("₹31,55,138.50"))
    #expect(!ValueWords.compound("91%"))
    #expect(!ValueWords.compound("7.1 h"))
    // Hours asleep a night, written as the page writes a duration, on an axis in hours.
    let asleep = Navree.Chart(
        kind: .bar, labels: ["Mon", "Tue"],
        series: [Navree.Series(label: "Asleep", shown: ["7 h 6 min", "8 h 32 min"], values: [7.1, 8.5])],
        y: Navree.Measure(name: "Asleep", unit: "h")
    )
    let style = ValueWords.style(asleep)
    #expect([8.0, 4, 0].map { ValueWords.text($0, style) } == ["8 h", "4 h", "0 h"])
    // A value in one number keeps the page's own way of writing it.
    let duration = Navree.Chart(
        kind: .area, labels: ["Mon"], series: [Navree.Series(label: "Duration", shown: ["7.1 h"], values: [7.1])],
        y: Navree.Measure(name: "Duration", unit: "h")
    )
    #expect(ValueWords.text(6, ValueWords.style(duration)) == "6 h")
}

/// Every chart in a folder of recorded answers whose value axis names its unit
/// writes its axis in that unit. The folder is named at run time
/// (`NAVREE_ANSWERS`), never kept with the tests: without it, nothing runs.
@Test(.enabled(if: ProcessInfo.processInfo.environment["NAVREE_ANSWERS"] != nil))
func everyAxisOfAFolderOfAnswersReadsInTheUnitItsChartNames() throws {
    let folder = URL(fileURLWithPath: ProcessInfo.processInfo.environment["NAVREE_ANSWERS"] ?? "", isDirectory: true)
    let files = try FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)
        .filter { $0.pathExtension == "json" }
    var checked = 0
    for file in files {
        guard let screen = try? WireDecoder.decode(Navree.Envelope.self, from: Data(contentsOf: file)).screen else { continue }
        screen.body.walk { node in
            guard case .chart(let chart) = node, chart.levels.isEmpty, let unit = chart.y?.unit, !unit.isEmpty else { return }
            let written = ValueWords.text(4, ValueWords.style(chart))
            #expect(written.hasSuffix(unit) || written.hasPrefix(unit), "\(chart.title ?? "") writes \(written)")
            checked += 1
        }
    }
    #expect(checked > 0)
}
