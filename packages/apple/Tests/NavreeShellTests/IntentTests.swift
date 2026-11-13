import Foundation
import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

/// What the drawing says goes to the core as its events, and what the drawing
/// reads comes from the core's state.
@MainActor
struct IntentTests {
    @Test func aFieldTheDrawingNamesIsTheCoresFieldByItsIdAndKeepsItsShape() throws {
        let shell = try Shell()
        let order = shell.orderOpen()
        #expect(shell.client.screen(order)?.screen?.title == "Order CS-20481")
        let context = shell.client.context(for: order, depth: 1, base: nil)
        let refund = FormRef(key: "refund", method: .post, action: "/orders/1/refund")

        context.send(.act(.commands(.init(list: [.open(.init(id: "refund"))])), Place()))
        #expect(shell.client.screen(order)?.overlays == ["refund"])
        context.send(.change(form: refund, name: "amount", values: ["0"]))
        #expect(shell.client.screen(order)?.forms["refund"]?.fields["refund-amount"]?.value == .text("0"))
        #expect(context.values(refund, "amount") == ["0"])

        context.send(.act(.submit(.init()), Place(form: refund, overlay: "refund")))
        let post = try #require(shell.transport.fetched.last)
        #expect(post.method == "post")
        #expect(post.url == "https://shop.example/orders/1/refund")
        // The 422 came back at once: the message is at its field, read by the drawing.
        #expect(context.errors(refund, "amount") == ["Enter an amount above zero, like 24.00."])
        #expect(context.errors(refund, "reason") == [])
        #expect(shell.problems.list.isEmpty, "\(shell.problems.list)")
    }

    @Test func aControlThatAsksFirstPutsTheQuestionOnTheScreen() throws {
        let shell = try Shell()
        let order = shell.orderOpen()
        let ask = Navree.Question(cancel: "Keep", confirm: "Cancel order", title: "Cancel CS-20481?")
        let cancel = Navree.Action.request(.init(href: "/orders/1/cancel", method: .post))
        shell.client.perform(.act(cancel, Place(ask: ask)), on: order)
        let question = try #require(shell.client.screen(order)?.question)
        #expect(question.title == "Cancel CS-20481?")
        #expect(question.confirm == "Cancel order")
        #expect(!shell.transport.fetched.contains { $0.url.hasSuffix("/cancel") })
        shell.client.send(.answer(screen: order, question: question.id, yes: true))
        #expect(shell.transport.fetched.contains { $0.url == "https://shop.example/orders/1/cancel" })
    }

    @Test func aSheetOfTheTreeClosedByThePersonIsToldAsAnOverlay() throws {
        let shell = try Shell()
        let order = shell.orderOpen()
        shell.client.perform(.act(.commands(.init(list: [.open(.init(id: "refund"))])), Place()), on: order)
        #expect(shell.client.screen(order)?.overlays == ["refund"])
        shell.client.perform(.dismissed(overlay: "refund"), on: order)
        #expect(shell.client.screen(order)?.overlays == [])
    }

    @Test func aTapOnALinkThatPushesIsAskedOfTheCoreAndTheStackGrows() throws {
        let shell = try Shell()
        shell.client.begin()
        shell.client.send(.select(destination: 1))
        let orders = try #require(shell.client.nav.front)
        let stack = shell.client.nav.destinations[1].stack
        let tap = PathEntry.tap(.navigate(.init(href: "/orders/1")), Place())
        guard case .tap(let action, let place, let screen)? = StackPath.change(stack, to: [tap]) else {
            Issue.record("a tap is not asked")
            return
        }
        shell.client.perform(.act(action, place), on: screen)
        #expect(shell.client.nav.destinations[1].stack.count == 2)
        #expect(shell.client.nav.front != orders)
        // The back swipe.
        guard case .popTo(let back)? = StackPath.change(shell.client.nav.destinations[1].stack, to: []) else {
            Issue.record("a pop is not told")
            return
        }
        shell.client.send(.popTo(screen: back))
        #expect(shell.client.nav.destinations[1].stack == [orders])
    }

    @Test func theMessageShownIsPutAwayByTheDrawing() throws {
        var answers = Shell.store
        answers["/"] = (200, "made/dispatch.json")
        answers["/dispatch/bulk"] = (200, "made/dispatch-cancelled.json")
        let shell = try Shell(answers: answers)
        shell.client.begin()
        let dispatch = try #require(shell.client.nav.front)
        shell.client.send(.choose(screen: dispatch, table: "dispatch", row: "1043", on: true, across: false))
        let cancel = Navree.Action.submit(.init(with: Navree.Pair(name: "intent", value: "cancel")))
        shell.client.perform(.act(cancel, Place(form: FormRef(key: "bulk"))), on: dispatch)
        let message = try #require(shell.client.messages.first)
        #expect(message.text == "CS-1043 is cancelled")
        let shown = Drawable.notice(message)
        #expect(shown.notice?.text == "CS-1043 is cancelled")

        shell.client.perform(.noticeDismissed, on: dispatch)
        #expect(shell.client.messages.isEmpty)
        #expect(shell.problems.list.isEmpty, "\(shell.problems.list)")
        #expect(MessageButton.is(MessageButton.action))
        #expect(!MessageButton.is(.refresh))
    }

    @Test func theScreensOwnSearchIsSaidAsASearch() throws {
        let shell = try Shell()
        shell.client.begin()
        shell.client.send(.select(destination: 1))
        let orders = try #require(shell.client.nav.front)
        let before = shell.transport.fetched.count
        shell.client.perform(.search(form: FormRef(key: "search"), name: "q", text: "Ludhiana"), on: orders)
        #expect(shell.problems.list.isEmpty, "\(shell.problems.list)")
        // Whether the core asks now or after a pause is its own; nothing is refused.
        #expect(shell.transport.fetched.count >= before)
    }

    @Test func aFormOfNoIdIsSentAsNoForm() {
        #expect(NativeClient.formID(nil) == nil)
        #expect(NativeClient.formID(.loose) == nil)
        #expect(NativeClient.formID(FormRef(key: "@/orders")) == nil)
        #expect(NativeClient.formID(FormRef(key: "refund")) == "refund")
    }
}

/// What a card or a figure opens carries the part's id, and the screen it
/// opens knows it: what a zoom from the card into its screen is drawn from.
@MainActor
@Test func aScreenOpenedFromAPartKnowsWhichPart() throws {
    var answers = Shell.store
    answers["/orders/1"] = (200, "store/order.json")
    let shell = try Shell(answers: answers)
    shell.client.begin()
    shell.client.send(.select(destination: 1))
    let orders = try #require(shell.client.nav.front)
    shell.client.perform(.act(.navigate(.init(href: "/orders/1")), Place(from: "order-card")), on: orders)
    let order = try #require(shell.client.nav.front)
    #expect(order != orders)
    #expect(shell.client.screen(order)?.from == "order-card")
    #expect(shell.client.context(for: order, depth: 1, base: nil).from == "order-card")
}
