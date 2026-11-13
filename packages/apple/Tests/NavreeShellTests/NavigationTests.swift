import Foundation
import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

@Test func theSystemsPathShowsEveryScreenOverTheFirst() {
    #expect(StackPath.path([2]) == [])
    #expect(StackPath.path([2, 3, 5]) == [.screen(3), .screen(5)])
}

@Test func aTappedLinkThatPushesAsksTheCoreFromTheScreenOnTop() {
    let action = Navree.Action.navigate(.init(href: "/orders/1"))
    let place = Place(from: "row-1")
    #expect(StackPath.change([2], to: [.tap(action, place)]) == .tap(action, place, screen: 2))
    #expect(StackPath.change([2, 3], to: [.screen(3), .tap(action, place)]) == .tap(action, place, screen: 3))
}

@Test func theBackGestureAndTheLongPressOnBackGoDownTheStack() {
    #expect(StackPath.change([2, 3], to: []) == .popTo(2))
    #expect(StackPath.change([2, 3, 5, 8], to: [.screen(3)]) == .popTo(3))
    // A path the system wrote back as it was is no change.
    #expect(StackPath.change([2, 3], to: [.screen(3)]) == nil)
    #expect(StackPath.change([], to: []) == nil)
}

@Test func aFullPresentationCoversTheScreenAndAnyOtherIsASheet() {
    #expect(ModalRules.presentation(.full) == .cover)
    #expect(ModalRules.presentation(.sheet) == .sheet)
    #expect(ModalRules.presentation(.task) == .sheet)
    #expect(ModalRules.presentation(.push) == .sheet)
}

@Test func aLockedPresentationOrATaskWithUnsavedInputCannotBeSwipedAway() throws {
    let sheet = try modal(#"{"present":"sheet","locked":false,"stack":[4]}"#)
    let locked = try modal(#"{"present":"full","locked":true,"stack":[4]}"#)
    let task = try modal(#"{"present":"task","locked":false,"stack":[4]}"#)
    #expect(ModalRules.dismissible(sheet, unsaved: true))
    #expect(!ModalRules.dismissible(locked, unsaved: false))
    #expect(ModalRules.dismissible(task, unsaved: false))
    #expect(!ModalRules.dismissible(task, unsaved: true))
    #expect(ModalRules.dismissal(task) == .cancel)
    #expect(ModalRules.dismissal(sheet) == .close)
    #expect(ModalRules.dismissal(locked) == nil)
}

func modal(_ json: String) throws -> Modal {
    try JSONDecoder().decode(Modal.self, from: Data(json.utf8))
}

@Test func aFieldChangedFromWhatThePageCameWithIsUnsavedInput() throws {
    func screen(_ value: String) throws -> ScreenState {
        try JSONDecoder().decode(ScreenState.self, from: Data(
            #"""
            {"id":4,"url":"/returns/new","content":{"t":"native","screen":{"title":"Return","body":
              {"t":"form","id":"return","method":"post","action":"/returns","children":[
                {"t":"field","id":"return-reason","name":"reason","input":{"t":"text","value":"Too small"}},
                {"t":"field","id":"return-pickup","name":"pickup","input":{"t":"toggle","on":true}}]}}},
             "forms":{"return":{"pending":false,"fields":{
                "return-reason":{"name":"reason","value":{"t":"text","text":"\#(value)"},"errors":[]},
                "return-pickup":{"name":"pickup","value":{"t":"flag","on":true},"errors":[]}}}}}
            """#.utf8
        ))
    }
    #expect(!Unsaved.holds(try screen("Too small")))
    #expect(Unsaved.holds(try screen("Too small for Harpreet")))
}

@Test func aPartTheCoreKeepsHiddenIsDrawnAsNothing() throws {
    let screen = try JSONDecoder().decode(Navree.Screen.self, from: Data(
        #"""
        {"title":"Order","body":{"t":"stack","children":[
          {"t":"section","id":"notes","title":"Notes","children":[{"t":"text","value":"Gift wrap"}]},
          {"t":"text","id":"total","value":"₹1,00,000"}]}}
        """#.utf8
    ))
    let drawn = Drawable.screen(screen, states: ["notes": [.hidden], "total": [.selected]])
    guard case .stack(let body) = drawn.body else {
        Issue.record("the body is not a stack")
        return
    }
    guard case .stack(let notes) = body.children[0] else {
        Issue.record("the hidden section is still drawn: \(body.children[0])")
        return
    }
    #expect(notes.hidden)
    #expect(notes.id == "notes")
    #expect(body.children[1].id == "total")
    #expect(Drawable.screen(screen, states: [:]) == screen)
}

@Test func aMessageThatSaysSomethingFailedIsAnAlertAndAnyOtherALineOfStatus() throws {
    let failed = try JSONDecoder().decode(Message.self, from: Data(#"{"id":2,"tone":"danger","notice":"failed"}"#.utf8))
    let done = try JSONDecoder().decode(
        Message.self, from: Data(#"{"id":3,"tone":"success","text":"Refunded ₹10.00","action":"Undo"}"#.utf8)
    )
    let alert = Drawable.notice(failed)
    #expect(alert.notice == nil)
    #expect(alert.failure?.text == "That didn't work. Try again.")
    let status = Drawable.notice(done)
    #expect(status.failure == nil)
    #expect(status.notice?.text == "Refunded ₹10.00")
    #expect(status.notice?.actionLabel == "Undo")
    #expect(status.notice?.action.map(MessageButton.is) == true)
    #expect(Drawable.notice(nil).notice == nil)
}

@Test func aPresentedGlanceRestsAtHalfHeightAndATaskTakesTheFullHeight() throws {
    #expect(ModalRules.height(try modal(#"{"present":"sheet","stack":[4]}"#)) == .half)
    #expect(ModalRules.height(try modal(#"{"present":"task","stack":[4]}"#)) == .full)
    #expect(ModalRules.height(try modal(#"{"present":"sheet","locked":true,"stack":[4]}"#)) == .full)
}

@Test func aScreenThatShowsSomethingWhileItLoadsAgainHasASpinnerInItsBar() throws {
    func state(_ json: String) throws -> ScreenState {
        try JSONDecoder().decode(ScreenState.self, from: Data(json.utf8))
    }
    let reloading = try state(#"{"id":2,"url":"/orders","loading":true,"content":{"t":"native","screen":{"title":"Orders","body":{"t":"stack"}}}}"#)
    let first = try state(#"{"id":2,"url":"/orders","loading":true,"content":{"t":"blank"}}"#)
    let still = try state(#"{"id":2,"url":"/orders","content":{"t":"native","screen":{"title":"Orders","body":{"t":"stack"}}}}"#)
    #expect(LoadingRules.progressInBar(reloading))
    // The first load has the screen's own spinner; a screen at rest has none.
    #expect(!LoadingRules.progressInBar(first))
    #expect(!LoadingRules.progressInBar(still))
}
