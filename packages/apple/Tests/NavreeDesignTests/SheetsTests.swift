import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// A sheet is a task the person finishes (Cancel and Done, full height) or a
// glance they swipe away (half height, Close).

@Test func aTaskTakesTheFullHeightAndAGlanceRestsAtHalf() {
    #expect(SheetRules.height(task: true) == .full)
    #expect(SheetRules.height(task: false) == .half)
}

@Test func aTasksSubmitLeavesTheContentForTheBarAsItsConfirmingAction() throws {
    let body = try decode([Navree.Node].self, """
        [{"t":"form","id":"new-order","method":"post","action":"/orders","children":[
           {"t":"field","name":"customer","label":"Customer","input":{"t":"text"}},
           {"t":"button","label":"Create order","action":{"t":"submit"}}]},
         {"t":"dialog","id":"discard","children":[{"t":"form","action":"/x","children":[
           {"t":"button","label":"Discard","action":{"t":"submit"}}]}]}]
        """)
    let lifted = SheetRules.lift(body)
    #expect(lifted.form?.id == "new-order")
    #expect(lifted.confirms.map(\.label) == ["Create order"])
    #expect(lifted.confirms.first?.role == .confirm)
    #expect(SheetRules.holdsInput(body))
}

@Test func aTaskThatHoldsInputIsCancelledAndAnythingElseClosed() throws {
    #expect(SheetRules.dismissal(task: true, holdsInput: true) == .cancel)
    #expect(SheetRules.dismissal(task: true, holdsInput: false) == .close)
    #expect(SheetRules.dismissal(task: false, holdsInput: true) == .close)
}

@Test func anOrdersPeekIsATaskWithNoFormOfItsOwn() throws {
    let peek = try answer("store-peek")
    guard case .morph(let morph)? = peek.ops.first, case .sheet(let sheet) = morph.node else {
        Issue.record("the peek does not fill its sheet")
        return
    }
    #expect(sheet.task)
    // The refund form stands in a dialog of its own: it is not the sheet's.
    #expect(SheetRules.lift(sheet.children).form == nil)
}

@Test func aRecordsInlineEditIsNotItsScreensOwnForm() throws {
    let inventory = try answerScreen("store-inventory")
    #expect(SheetRules.ownForm([inventory.body])?.method != .post)
}
