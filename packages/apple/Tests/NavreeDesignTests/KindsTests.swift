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
