import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// The menu bar of a Mac (and an iPad with a keyboard): every toolbar and
// context command is also there, with the shortcut Apple's apps give it.

private func button(
    _ label: String, role: Navree.Role = .secondary, standard: Navree.Standard? = nil, ask: Bool = false,
    undoable: Bool = false, disabled: Bool = false
) -> Navree.Node {
    .button(Navree.Button(
        action: .navigate(Navree.Action.Navigate(href: "/x")),
        ask: ask ? Navree.Question(confirm: label, title: "\(label)?") : nil, disabled: disabled, label: label,
        role: role, standard: standard, undoable: undoable
    ))
}

@Test func makingSomethingNewIsFileNewWithCommandN() {
    let entries = CommandRules.entries([button("New order", role: .create), button("New return", standard: .add)])
    #expect(entries.map(\.slot) == [.new, .new])
    #expect(entries[0].title == "New Order")
    #expect(entries[0].shortcut == .new)
    // ⌘N goes to the first only.
    #expect(entries[1].shortcut == nil)
    #expect(entries[1].symbol == "plus")
}

@Test func deletingIsTheLastCommandOfTheScreensMenuWithCommandDelete() {
    let entries = CommandRules.entries([
        button("Delete order", role: .destructive, standard: .delete, ask: true),
        button("Cancel order", role: .destructive),
    ])
    #expect(entries.map(\.slot) == [.destructive, .destructive])
    #expect(entries[0].shortcut == .delete)
    // A command that asks first says so with an ellipsis.
    #expect(entries[0].title == "Delete Order…")
    // Only the standard delete takes ⌘⌫; another destructive act has none.
    #expect(entries[1].shortcut == nil)
    #expect(entries[1].title == "Cancel Order")
}

@Test func sharingIsFileShareAndSearchingIsEditFind() {
    let entries = CommandRules.entries([button("Share", standard: .share), button("Search", standard: .search)])
    #expect(entries.map(\.slot) == [.share, .find])
    #expect(entries[0].title == "Share…")
    #expect(entries[0].shortcut == nil)
    #expect(entries[0].symbol == "square.and.arrow.up")
    #expect(entries[1].shortcut == .find)
}

@Test func anyOtherActionIsACommandOfTheScreensMenuAndKeepsItsState() {
    let entries = CommandRules.entries([
        button("Mark as packed", role: .primary), button("Print labels", disabled: true),
        .link(Navree.Link(action: .navigate(Navree.Action.Navigate(href: "/orders/1/history")), label: "View history")),
    ])
    #expect(entries.map(\.slot) == [.screen, .screen, .screen])
    #expect(entries.map(\.title) == ["Mark as Packed", "Print Labels", "View History"])
    #expect(entries.map(\.disabled) == [false, true, false])
    #expect(entries.allSatisfy { $0.shortcut == nil })
}

@Test func aMenuOfActionsStandsForTheActionsItHolds() {
    let menu = Navree.Node.menu(Navree.Menu(items: [button("Refund"), button("Resend invoice")], label: "More"))
    let leaves = CommandRules.leaves([menu, button("Archive"), .text(Navree.Text(value: "Not an action"))])
    #expect(leaves.count == 3)
    #expect(CommandRules.entries(leaves).map(\.title) == ["Refund", "Resend Invoice", "Archive"])
    // A button with nothing to do is no command.
    #expect(CommandRules.leaves([.button(Navree.Button(label: "Nothing"))]).isEmpty)
}

@Test func theDashboardsToolbarIsEveryOneOfItsCommands() throws {
    let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
    let data = try Data(contentsOf: root.appendingPathComponent("crates/navree-native/corpus/screen-dashboard.json"))
    let screen = try #require(try WireDecoder.decode(Navree.Envelope.self, from: data).screen)
    let entries = CommandRules.entries(CommandRules.leaves(screen.actions))
    #expect(entries.map(\.title) == ["Share…", "New Order", "Close the Day", "Edit the Dashboard", "Search Orders…"])
    #expect(entries.map(\.slot) == [.share, .new, .screen, .screen, .find])
    #expect(entries.map(\.shortcut) == [nil, .new, nil, nil, .find])
    // A command that cannot be used now is shown, and disabled.
    #expect(entries[2].disabled)
}

@Test func findGoesToTheScreensSearchOrItsTablesSearchField() throws {
    #expect(CommandRules.searches(Navree.Screen(body: .text(Navree.Text(value: "Hi")), search: Navree.Search(name: "q"), title: "Orders")))
    #expect(!CommandRules.searches(Navree.Screen(body: .text(Navree.Text(value: "Hi")), title: "Orders")))
    #expect(CommandRules.searches(try recordedScreen("orders.json")))
}

@Test func theDestinationsAreCommandOneToNine() {
    #expect(CommandRules.destination(0) == Shortcut(.digit(1)))
    #expect(CommandRules.destination(8) == Shortcut(.digit(9)))
    #expect(CommandRules.destination(9) == nil)
}

@Test func menuTitlesAreInTitleCase() {
    #expect(CommandRules.title("mark as read") == "Mark as Read")
    #expect(CommandRules.title("Restock") == "Restock")
    #expect(CommandRules.title("send to Amritsar office") == "Send to Amritsar Office")
    #expect(CommandRules.title("Refund CH-1042") == "Refund CH-1042")
    #expect(CommandRules.title("Share", more: true) == "Share…")
    #expect(CommandRules.title("Share…", more: true) == "Share…")
}

@Test func theScreensMenuIsNamedForTheDestinationItShows() {
    #expect(CommandRules.screenMenu("Orders") == "Orders")
    #expect(CommandRules.screenMenu("inventory") == "Inventory")
    // Never a second File or View menu.
    #expect(CommandRules.screenMenu("View") == "Actions")
    #expect(CommandRules.screenMenu(nil) == "Actions")
}

@Test func theUserMenusSettingsEntryIsFoundByItsGearOrItsAddress() {
    let profile = Navree.Node.link(Navree.Link(action: .navigate(Navree.Action.Navigate(href: "/profile")), label: "Profile"))
    let settings = Navree.Node.link(Navree.Link(action: .navigate(Navree.Action.Navigate(href: "/account/settings")), label: "Settings"))
    let gear = Navree.Node.button(Navree.Button(
        action: .navigate(Navree.Action.Navigate(href: "/me/preferences-page")), icon: Navree.Icon(name: "settings"), label: "Your account"
    ))
    let signOut = Navree.Node.button(Navree.Button(action: .request(Navree.Action.Request(href: "/sign-out", method: .post)), label: "Sign out"))
    #expect(SettingsRules.destination(in: [profile, settings, signOut])?.index == 1)
    #expect(SettingsRules.destination(in: [profile, settings, signOut])?.href == "/account/settings")
    #expect(SettingsRules.destination(in: [profile, gear, signOut])?.index == 1)
    #expect(SettingsRules.destination(in: [profile, signOut]) == nil)
}

// MARK: A detail beside its list

@Test func theListsCommandsStayWhileADetailIsOpenItsNewKeepingCommandN() throws {
    // The corpus orders list ("New order") beside an order, and the live order
    // with its own actions: File > New Order keeps ⌘N, as Mail's New Message
    // stays whatever is open, and the order's own commands follow.
    let list = CommandRules.leaves(try answerScreen("store-orders").actions)
        + [button("New order", role: .create)]
    let detail = CommandRules.leaves(try answerScreen("store-order1").actions)
    let window = CommandRules.window(detail: detail, list: list)
    let entries = CommandRules.entries(window.map(\.node))
    #expect(entries.first?.title == "New Order")
    #expect(entries.first?.shortcut == .new)
    #expect(window.first?.fromList == true)
    #expect(entries.map(\.title).contains("Resend Confirmation…"))
    #expect(window.filter { !$0.fromList }.count == detail.count)
    // The detail's own way to make something new comes after and has no ⌘N.
    let both = CommandRules.window(detail: [button("New refund", role: .create)], list: [button("New order", role: .create)])
    let made = CommandRules.entries(both.map(\.node))
    #expect(made.map(\.title) == ["New Order", "New Refund"])
    #expect(made.map(\.shortcut) == [.new, nil])
}

@Test func aCommandTheListAndTheDetailBothOfferIsListedOnce() {
    let share = button("Share", standard: .share)
    let window = CommandRules.window(detail: [share, button("Refund")], list: [button("New order", role: .create), share, button("Export")])
    #expect(window.map(\.fromList) == [true, false, false, true])
    #expect(CommandRules.entries(window.map(\.node)).map(\.title) == ["New Order", "Share…", "Refund", "Export"])
    // With nothing beside it, the detail's commands are the window's.
    #expect(CommandRules.window(detail: [button("Refund")], list: []).map(\.fromList) == [false])
}
