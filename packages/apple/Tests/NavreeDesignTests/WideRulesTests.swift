import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// Where a list stands beside its detail, and how a wide table draws a
// record's form: each rule with the case that proves it.

private func corpusScreen(_ name: String) throws -> Navree.Screen {
    let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
    let data = try Data(contentsOf: root.appendingPathComponent("crates/navree-native/corpus/\(name)"))
    return try #require(try WireDecoder.decode(Navree.Envelope.self, from: data).screen)
}

private func pane(_ role: Navree.PaneRole, _ title: String, current: Bool = false, _ children: [Navree.Node] = []) -> Navree.Node {
    .pane(Navree.Pane(children: children, current: current, role: role, title: title))
}

private func row(_ title: String, _ action: Navree.Action?) -> Navree.Node {
    .row(Navree.Row(action: action, title: title))
}

private func go(_ href: String, _ present: Navree.Present = .auto) -> Navree.Action {
    .navigate(Navree.Action.Navigate(href: href, present: present))
}

@Test func theInboxsPanesAreAListBesideItsThreadWithTheCustomerAsItsInspector() throws {
    let inbox = try corpusScreen("screen-inbox.json")
    let panes = try #require(SplitRules.panes(inbox.body))
    #expect(panes.list.title == "Inbox")
    #expect(panes.detail?.title == "Harpreet Sandhu")
    #expect(panes.detail?.role == .detail)
    #expect(panes.inspector?.title == "Customer")
    // Columns at every width: the system folds them into one stack when narrow.
    for width in WindowClass.allCases {
        #expect(SplitRules.layout(inbox.body, width: width) == .panes(panes))
    }
}

@Test func aNarrowWindowShowsThePaneTheServerSaysIsCurrent() throws {
    let inbox = try #require(SplitRules.panes(try corpusScreen("screen-inbox.json").body))
    #expect(SplitRules.compactColumn(inbox) == .detail)
    let listFirst = SplitPanes(
        list: Navree.Pane(current: true, role: .list), detail: Navree.Pane(role: .detail)
    )
    #expect(SplitRules.compactColumn(listFirst) == .list)
    // Nothing current: the list, as Mail opens on its mailbox.
    #expect(SplitRules.compactColumn(SplitPanes(list: Navree.Pane(role: .list), detail: Navree.Pane(role: .detail))) == .list)
}

@Test func contentBesideWhatSupportsItIsNoListBesideADetail() throws {
    // The dashboard's sales beside its filters stand inside their screen.
    let dashboard = try corpusScreen("screen-dashboard.json")
    #expect(SplitRules.panes(dashboard.body) == nil)
    #expect(SplitRules.layout(dashboard.body, width: .expanded) == .stack)
    // A split whose detail side holds anything but panes is no columns either.
    let mixed = Navree.Node.split(Navree.Split(detail: .text(Navree.Text(value: "Detail")), primary: pane(.list, "List")))
    #expect(SplitRules.panes(mixed) == nil)
}

@Test func theInspectorStandsBesideTheDetailOnlyWhereThreeColumnsFit() {
    #expect(SplitRules.showsInspector(width: .expanded))
    #expect(!SplitRules.showsInspector(width: .medium))
    #expect(!SplitRules.showsInspector(width: .compact))
    #expect(SplitRules.sideBySide(width: .medium))
    #expect(!SplitRules.sideBySide(width: .compact))
}

@Test func eachColumnDrawsItsPaneWithTheActionsThatBelongThere() throws {
    let compose = Navree.Node.button(Navree.Button(action: go("/inbox/new"), label: "New message", role: .create))
    let archive = Navree.Node.button(Navree.Button(action: go("/inbox/88/archive"), label: "Archive", role: .secondary))
    let refund = Navree.Node.sheet(Navree.Sheet(id: "refund", title: "Refund"))
    let body = Navree.Node.stack(Navree.Stack(children: [
        .split(Navree.Split(
            detail: .split(Navree.Split(
                detail: pane(.inspector, "Customer", [.text(Navree.Text(value: "+91 98140 12345"))]),
                primary: pane(.detail, "Harpreet Sandhu", current: true, [.text(Navree.Text(value: "Thread"))])
            )),
            primary: pane(.list, "Inbox", [row("Harpreet Sandhu", go("/inbox/88"))])
        )),
        refund,
    ]))
    let screen = Navree.Screen(actions: [compose, archive], body: body, search: Navree.Search(action: "/inbox", name: "q"), title: "Support")
    let panes = try #require(SplitRules.panes(screen.body))

    let list = SplitRules.screen(.list, of: screen, panes: panes)
    #expect(list.title == "Inbox")
    #expect(list.actions == [compose])
    #expect(list.search != nil)
    #expect(list.body.children.count == 1)

    let detail = SplitRules.screen(.detail, of: screen, panes: panes)
    #expect(detail.title == "Harpreet Sandhu")
    #expect(detail.actions == [archive])
    #expect(detail.search == nil)
    // The screen's sheets are presented over its detail.
    #expect(detail.body.children.last == refund)

    let inspector = SplitRules.screen(.inspector, of: screen, panes: panes)
    #expect(inspector.title == "Customer")
    #expect(inspector.actions.isEmpty)

    // A screen with no panes is drawn whole.
    let plain = Navree.Screen(body: .text(Navree.Text(value: "Hello")), title: "Plain")
    #expect(SplitRules.part(.list, of: plain) == plain)
    #expect(SplitRules.part(nil, of: screen) == screen)
}

@Test func aChoiceInTheListReplacesTheSplitRatherThanStackingOne() {
    #expect(SplitRules.choice(go("/inbox/87")) == go("/inbox/87", .replace))
    #expect(SplitRules.choice(go("/inbox/87", .push)) == go("/inbox/87", .replace))
    // A sheet stays a sheet; anything that is not a link goes as it came.
    #expect(SplitRules.choice(go("/orders/new", .sheet)) == go("/orders/new", .sheet))
    let submit = Navree.Action.submit(Navree.Action.Submit())
    #expect(SplitRules.choice(submit) == submit)
}

@Test func aListWhoseRowsOpenScreensStandsBesideTheOneOpenWhereThereIsRoom() throws {
    let orders = try corpusScreen("screen-list.json")
    #expect(SplitRules.rowsOpenScreens(orders.body))
    #expect(SplitRules.layout(orders.body, width: .medium) == .listBesideDetail)
    #expect(SplitRules.layout(orders.body, width: .expanded) == .listBesideDetail)
    // On a compact width its rows push as ever.
    #expect(SplitRules.layout(orders.body, width: .compact) == .stack)

    let mail = Navree.Node.list(Navree.List(children: [row("Simran Bhatia", go("/inbox/87")), row("Harpreet Sandhu", go("/inbox/88", .push))]))
    #expect(SplitRules.rowsOpenScreens(mail))
}

@Test func rowsThatOpenSheetsActInPlaceOrOpenNothingKeepTheirScreenWhole() throws {
    let glances = Navree.Node.list(Navree.List(children: [row("CS-1042", go("/orders/1042/peek", .sheet))]))
    #expect(!SplitRules.rowsOpenScreens(glances))
    let mixed = Navree.Node.list(Navree.List(children: [row("CS-1042", go("/orders/1042")), row("Note", nil)]))
    #expect(!SplitRules.rowsOpenScreens(mixed))
    #expect(!SplitRules.rowsOpenScreens(.list(Navree.List())))
    // The store's records open their glance from a form, not a row's link.
    let orders = try recordedScreen("orders.json")
    #expect(SplitRules.layout(orders.body, width: .expanded) == .stack)
    // A screen of several lists (a board) is no list beside a detail.
    let board = try corpusScreen("screen-board.json")
    #expect(SplitRules.layout(board.body, width: .expanded) == .stack)
    #expect(SplitRules.layout(nil, width: .expanded) == .stack)
}

@Test func theChosenRowIsTheOneWhoseLinkLeadsToTheDetailOpen() throws {
    let orders = try corpusScreen("screen-list.json")
    let chosen = try #require(SplitRules.chosenRow(in: orders.body, opened: "https://charkha.example/orders/1042?tab=items"))
    #expect(chosen.title == "CH-1042")
    #expect(SplitRules.chosenRow(in: orders.body, opened: "/orders/9") == nil)

    let delete = Navree.Button(action: go("/inbox/88/delete"), label: "Delete", role: .destructive, standard: .delete)
    let thread = Navree.Row(action: go("/inbox/88"), menu: [.button(Navree.Button(action: go("/inbox/88/read"), label: "Mark as read"))], swipe: [delete], title: "Harpreet Sandhu")
    let mail = Navree.Node.list(Navree.List(children: [.row(thread)]))
    let found = try #require(SplitRules.chosenRow(in: mail, opened: "/inbox/88"))
    #expect(found.actions.count == 2)
    #expect(found.actions.last == .button(delete))
}

@Test func aFormInAWideTablesCellIsAButtonThatOpensItsFieldsInASheet() throws {
    let inventory = try recordedScreen("inventory.json")
    guard case .table(let table) = try #require(inventory.body.first { $0.kind == "table" }) else {
        Issue.record("no table")
        return
    }
    let column = try #require(table.columns.firstIndex { $0.label == "Restock" })
    let restock = try #require(table.rows.first?.cells[column])
    let controls = RecordRules.controls(in: restock)
    #expect(controls.count == 1)
    let control = try #require(controls.first)
    #expect(control.opensSheet)
    #expect(control.fields > 0)
    #expect(control.form?.method == .post)

    // A form with nothing to fill acts at once from its button.
    let resend = Navree.Node.form(Navree.Form(
        action: "/orders/1/resend",
        children: [.button(Navree.Button(action: .submit(Navree.Action.Submit()), label: "Resend"))],
        method: .post
    ))
    #expect(RecordRules.controls(in: resend).map(\.opensSheet) == [false])
    // A button alone is that button.
    let archive = Navree.Node.button(Navree.Button(action: go("/orders/1/archive"), label: "Archive"))
    #expect(RecordRules.controls(in: archive).map(\.button.label) == ["Archive"])
}

@Test func aWindowClassFollowsTheSystemsSizeClassForItsComponents() {
    #expect(WindowClass.compact.widthClass == .compact)
    #expect(WindowClass.medium.widthClass == .regular)
    #expect(WindowClass.expanded.widthClass == .regular)
}

@Test func aScreenWithPanesAmongItsOtherPartsIsDrawnWhole() throws {
    // The dashboard's sales beside its filters stand among its figures and
    // its feed: the screen is drawn whole, its split in place.
    let dashboard = try corpusScreen("screen-dashboard.json")
    #expect(PaneRules.panes(in: dashboard.body).isEmpty)
    // A screen made of panes alone is still drawn as its panes.
    let inbox = try corpusScreen("screen-inbox.json")
    #expect(PaneRules.panes(in: inbox.body).map(\.id) == ["conversations", "thread", "customer"])
}

@Test func aSplitInPlaceStandsSideBySideWhereThereIsRoomAndTheCurrentPaneFirstWhereNot() throws {
    let content = pane(.content, "Sales")
    let supporting = pane(.supporting, "Filters", current: true)
    let split = Navree.Split(detail: supporting, primary: content)
    let wide = SplitRules.inPlace(split, width: .medium)
    #expect(wide.sideBySide)
    #expect(wide.panes == [content, supporting])
    let narrow = SplitRules.inPlace(split, width: .compact)
    #expect(!narrow.sideBySide)
    #expect(narrow.panes == [supporting, content])
}

@Test func aDateInAFormIsTheCompactPickerOnEverySystem() {
    #expect(ContentRules.dateStyle(Navree.DateInput(kind: .date)) == .compact)
    #expect(ContentRules.dateStyle(Navree.DateInput(kind: .time)) == .compact)
    #expect(ContentRules.dateStyle(Navree.DateInput(kind: .datetime)) == .compact)
}

@Test func aMacsDestinationsAreTheSidebarOfOneSplitViewWithAListBesideItsDetailAsTwoMoreColumns() throws {
    let orders = SplitRules.layout(try corpusScreen("screen-list.json").body, width: .expanded)
    let inbox = SplitRules.layout(try corpusScreen("screen-inbox.json").body, width: .expanded)
    let dashboard = SplitRules.layout(try corpusScreen("screen-dashboard.json").body, width: .expanded)
    // A Mac's window (a sidebar that never turns into a tab bar): Orders and
    // the inbox are sidebar, list and detail in one split view, as Mail.
    #expect(NavigationRules.window(destinations: 3, sidebarOnly: true, stack: orders) == .sidebarListDetail)
    #expect(NavigationRules.window(destinations: 3, sidebarOnly: true, stack: inbox) == .sidebarListDetail)
    // A destination that is one stack has the same sidebar, its stack beside it.
    #expect(NavigationRules.window(destinations: 3, sidebarOnly: true, stack: dashboard) == .sidebarStack)
    // An iPad's destinations are a tab bar that may turn into a sidebar: the
    // tab view stays at every width, with the split view inside its tab.
    #expect(NavigationRules.window(destinations: 3, sidebarOnly: false, stack: orders) == .tabs)
    #expect(NavigationRules.window(destinations: 3, sidebarOnly: false, stack: inbox) == .tabs)
    // One destination has no sidebar to share a split view with.
    #expect(NavigationRules.window(destinations: 1, sidebarOnly: true, stack: orders) == .tabs)
}

@Test func theListsChosenRowIsTheOneOpenBesideItNeverOneTickedForMany() throws {
    // The corpus orders: CH-1041 is ticked for an action on many, CH-1042 is
    // open beside the list. The list shows CH-1042 chosen, as Mail shows the
    // message open, and no other.
    let orders = try corpusScreen("screen-list.json")
    #expect(SplitRules.openRow(in: orders.body, opened: "https://charkha.example/orders/1042") == "1042")
    #expect(SplitRules.openRow(in: orders.body, opened: "/orders/1041?from=list") == "1041")
    #expect(SplitRules.openRow(in: orders.body, opened: "/orders/9") == nil)
    // A list's rows are chosen by their ids the same way.
    let mail = Navree.Node.list(Navree.List(children: [
        .row(Navree.Row(action: go("/inbox/87"), id: "87", title: "Simran Bhatia")),
        .row(Navree.Row(action: go("/inbox/88"), id: "88", title: "Harpreet Sandhu")),
    ]))
    #expect(SplitRules.openRow(in: mail, opened: "/inbox/88") == "88")
}

// What a split view hands its rows and its detail's stack is compared by what
// it acts on, never by its code: drawn again with the same list and stack, it
// hands down the same values, so nothing under it is drawn again for them.

@Test func aChoiceIsTheSameForTheSameListWhateverItsCode() {
    let first = Choice(key: "panes 1") { _, _ in }
    let again = Choice(key: "panes 1") { _, _ in }
    let other = Choice(key: "panes 2") { _, _ in }
    #expect(first == again)
    #expect(first != other)
}

@Test func aColumnsPathIsTheSameForTheSameEntriesOfTheSameStack() {
    let first = ColumnPath(entries: [.screen(2)], key: "[1, 2]") { _ in }
    let again = ColumnPath(entries: [.screen(2)], key: "[1, 2]") { _ in }
    #expect(first == again)
    #expect(first != ColumnPath(entries: [], key: "[1, 2]") { _ in })
    #expect(first != ColumnPath(entries: [.screen(2)], key: "[1, 3]") { _ in })
}
