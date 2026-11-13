import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// A menu in the content shows what it sets in words, not only as a symbol.

@Test func theThreadsStatusAndPriorityMenusShowTheirWordsBesideTheirSymbols() throws {
    // The live thread's assignment row: the assignee as a button with words,
    // the status ("Open") and the priority ("High") as menus with symbols.
    let screen = try answerScreen("inbox-thread")
    var menus: [Navree.Menu] = []
    screen.body.walk { node in
        if case .menu(let menu) = node, ["status-menu", "priority-menu"].contains(menu.id) { menus.append(menu) }
    }
    #expect(menus.map(\.label) == ["Open", "High"])
    #expect(menus.map(ContentRules.menuLabel) == [.titleAndIcon, .titleAndIcon])
    // A menu with no symbol shows its words; one with no words, its symbol.
    #expect(ContentRules.menuLabel(Navree.Menu(label: "Sort")) == .title)
    #expect(ContentRules.menuLabel(Navree.Menu(icon: Navree.Icon(name: "ellipsis"), label: " ")) == .icon)
}
