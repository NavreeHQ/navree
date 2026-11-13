import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// A screen's own actions: placed by role, a toolbar's items read as buttons,
// a disabled one saying why, and each question put the way its act asks.

@Test func anOrdersActionsArePlacedByRoleWithTheDestructiveOneApart() throws {
    let order = try answerScreen("store-order1")
    let actions = ActionRules.expand(order.actions)
    let slots = PlacementRules.place(actions.map { ActionTraits($0) ?? ActionTraits(role: Navree.Role.none) }, depth: .pushed)
    // Resend and Refund are secondary: the system's overflow menu on a
    // compact width. Cancel order destroys: a section of its own, never prominent.
    #expect(slots == [.overflow, .overflow, .destructiveSection])
}

@Test func aDisabledButtonSaysWhyWhereItStands() throws {
    let order = try answerScreen("store-order1")
    let buttons = order.actions.compactMap { node -> Navree.Button? in
        if case .button(let button) = node { return button }
        return nil
    }
    let cancel = try #require(buttons.first { $0.role == .destructive })
    #expect(ActionRules.reason(cancel) == "CS-20481 has shipped, so it can't be cancelled. Refund it instead.")
    #expect(ActionRules.footer(buttons) == ActionRules.reason(cancel))
    // An enabled button says nothing, whatever reason it carries.
    var enabled = cancel
    enabled.disabled = false
    #expect(ActionRules.reason(enabled) == nil)
}

@Test func aToolbarsItemsAreTheButtonsAndMenusTheyStandFor() throws {
    let toolbar = try decode(Navree.Node.self, """
        {"t":"toolbar","items":[
          {"title":"Share","icon":{"name":"share"},"standard":"share","action":{"t":"navigate","href":"/share"}},
          {"title":"New order","icon":{"name":"plus"},"role":"create","prominence":"high","action":{"t":"navigate","href":"/orders/new"}},
          {"title":"Close the day","icon":{"name":"lock"},"disabled":true,"reason":"3 orders are still to be packed."},
          {"title":"More","icon":{"name":"ellipsis"},"standard":"more","menu":[{"t":"button","label":"Edit"}]}]}
        """)
    let nodes = ActionRules.expand([toolbar])
    #expect(nodes.map(\.kind) == ["button", "button", "button", "menu"])
    let slots = PlacementRules.place(nodes.map { ActionTraits($0) ?? ActionTraits(role: Navree.Role.none) }, depth: .root)
    #expect(slots == [.overflow, .create, .overflow, .overflow])
    guard case .button(let close) = nodes[2] else { return }
    #expect(ActionRules.reason(close) == "3 orders are still to be packed.")
}

@Test func aDestructiveActsQuestionIsAnActionSheetAndAnyOtherAnAlert() throws {
    let order = try answerScreen("store-order1")
    let cancel = QuestionRules.asker(title: "Cancel CS-20481?", in: order)
    let resend = QuestionRules.asker(title: "Send the confirmation of CS-20481 again?", in: order)
    #expect(cancel?.label == "Cancel order")
    #expect(resend?.label == "Resend confirmation")
    #expect(QuestionRules.style(asker: cancel) == .confirmationDialog)
    #expect(QuestionRules.style(asker: resend) == .alert)
    #expect(QuestionRules.confirmIsDestructive(asker: resend) == false)
    #expect(QuestionRules.style(asker: nil) == .alert)
}

@Test func aToolbarInTheBodyStandsAtTheScreensFoot() throws {
    let body = try decode(Navree.Node.self, """
        {"t":"stack","children":[{"t":"text","value":"Today"},
          {"t":"toolbar","items":[{"title":"Share","icon":{"name":"share"},"standard":"share"}]}]}
        """)
    #expect(ActionRules.footToolbars(body).count == 1)
    #expect(SectionRules.sections(body).flatMap(\.rows).count == 1)
}
