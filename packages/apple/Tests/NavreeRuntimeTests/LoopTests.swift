import Foundation
import NavreeCoreBridge
import NavreeProtocol
import Testing

@testable import NavreeRuntime

/// The loop over recorded conversations with the store: what the model holds
/// after each thing the person does.
@MainActor
struct LoopTests {
    @Test func theAppStartsShowsAScreenPushesAnotherAndGoesBack() throws {
        let h = try Harness()
        h.client.begin()
        #expect(h.client.nav.stage == .starting)
        #expect(h.recorder.takeFetches().map(\.2) == ["https://shop.example/_navree/native/shell"])

        try h.answer(1, with: "store/shell.json")
        #expect(h.client.nav.stage == .ready)
        #expect(h.client.app?.name == "Charkha Supply Co.")
        #expect(h.client.app?.user?.name == "Ravneet Kaur")
        #expect(h.client.nav.destinations.map(\.href) == ["/", "/orders", "/inventory"])
        #expect(h.client.nav.destinations.map(\.label) == ["Dashboard", "Orders", "Inventory"])
        #expect(h.client.screen(1)?.loading == true)
        #expect(h.recorder.takeFetches().map(\.2) == ["https://shop.example/"])

        // The dashboard is the web view's.
        try h.answer(2, with: "store/dashboard.json")
        guard case .web(let url, _, _, _)? = h.client.screen(1)?.content else {
            Issue.record("the dashboard is not a web page: \(String(describing: h.client.screen(1)?.content))")
            return
        }
        #expect(url == "https://shop.example/")

        h.client.send(.select(destination: 1))
        #expect(h.client.nav.selected == 1)
        #expect(h.recorder.takeFetches().map(\.2) == ["https://shop.example/orders"])
        try h.answer(3, with: "store/orders.json")
        let orders = try #require(h.client.screen(2)?.screen)
        #expect(orders.title == "Orders")
        #expect(h.client.front?.id == 2)

        h.client.send(.act(.init(screen: 2, action: .navigate(.init(href: "/orders/1")))))
        #expect(h.client.nav.destinations[1].stack == [2, 3])
        #expect(h.client.nav.front == 3)
        #expect(h.client.nav.canGoBack)
        try h.answer(4, with: "store/order.json")
        #expect(h.client.screen(3)?.screen?.title == "Order CS-20481")

        // The system's back gesture took the order away.
        h.client.send(.popTo(screen: 2))
        #expect(h.client.nav.destinations[1].stack == [2])
        #expect(h.client.screens[3] == nil)
        #expect(h.client.nav.front == 2)
        #expect(h.problems.list.isEmpty)
    }

    @Test func aRefusedFormShowsEachMessageAtItsField() throws {
        let h = try Harness()
        try h.started()
        h.client.send(.select(destination: 1))
        try h.answer(3, with: "store/orders.json")
        h.client.send(.act(.init(screen: 2, action: .navigate(.init(href: "/orders/1")))))
        try h.answer(4, with: "store/order.json")

        h.client.send(.act(.init(screen: 3, action: .commands(.init(list: [.open(.init(id: "refund"))])))))
        #expect(h.client.screen(3)?.overlays == ["refund"])
        h.client.send(.field(screen: 3, form: "refund", field: "refund-amount", value: .text("0")))
        #expect(h.client.screen(3)?.forms["refund"]?.fields["refund-amount"]?.value == .text("0"))
        h.recorder.effects.removeAll()
        h.client.send(.act(.init(screen: 3, action: .submit(.init()), form: "refund", overlay: "refund")))
        let post = try #require(h.recorder.lastFetch)
        #expect(post.method == "post")
        #expect(post.url == "https://shop.example/orders/1/refund")
        #expect(post.xsrf)
        guard case .form(let body)? = post.body else {
            Issue.record("the refund is not a form")
            return
        }
        #expect(body.contains("amount=0"))
        #expect(h.client.screen(3)?.forms["refund"]?.pending == true)

        h.recorder.effects.removeAll()
        try h.answer(post.request, with: "made/refund-refused.json", status: 422)
        let refund = try #require(h.client.screen(3)?.forms["refund"])
        #expect(refund.pending == false)
        #expect(refund.fields["refund-amount"]?.errors == ["Enter an amount above zero, like 24.00."])
        #expect(refund.fields["refund-amount"]?.value == .text("0"))
        #expect(refund.field(named: "amount")?.id == "refund-amount")
        let announced = h.recorder.effects.contains { effect in
            if case .announce(_, .fieldsNeedAttention?, 1?) = effect { return true }
            return false
        }
        #expect(announced)
    }

    @Test func aQuestionIsAnsweredNoThenYes() throws {
        let h = try Harness()
        try h.started(first: "made/dispatch.json")
        h.client.send(.choose(screen: 1, table: "dispatch", row: "1043", on: true, across: false))
        let ask = Navree.Question(cancel: "Keep", confirm: "Cancel order", text: "₹1,499.00 goes back to the customer.", title: "Cancel 1 order?")
        let cancel = CoreEvent.Act(
            screen: 1, action: .submit(.init(with: Navree.Pair(name: "intent", value: "cancel"))), form: "bulk", ask: ask
        )
        h.recorder.effects.removeAll()
        h.client.send(.act(cancel))
        let question = try #require(h.client.screen(1)?.question)
        #expect(question.title == "Cancel 1 order?")
        #expect(question.confirm == "Cancel order")
        #expect(question.cancel == "Keep")
        #expect(h.recorder.takeFetches().isEmpty)

        h.client.send(.answer(screen: 1, question: question.id, yes: false))
        #expect(h.client.screen(1)?.question == nil)
        #expect(h.recorder.takeFetches().isEmpty)

        h.client.send(.act(cancel))
        let again = try #require(h.client.screen(1)?.question)
        h.client.send(.answer(screen: 1, question: again.id, yes: true))
        #expect(h.client.screen(1)?.question == nil)
        let posted = h.recorder.takeFetches()
        #expect(posted.map(\.1) == ["post"])
        #expect(posted.map(\.2) == ["https://shop.example/dispatch/bulk"])
    }

    @Test func theEndOfAListAsksForMoreOnceAndTheRowsJoinIt() throws {
        let h = try Harness()
        try h.started()
        h.client.send(.select(destination: 1))
        try h.answer(3, with: "store/orders.json")
        h.client.send(.act(.init(screen: 2, action: .navigate(.init(href: "/activity")))))
        try h.answer(4, with: "made/activity.json")
        h.recorder.effects.removeAll()

        h.client.send(.more(screen: 3, id: "feed", before: false))
        #expect(h.client.screen(3)?.lists["feed"]?.loading == true)
        let more = try #require(h.recorder.lastFetch)
        #expect(more.url == "https://shop.example/activity?cursor=a3")
        #expect(more.header("x-navree-region") == "feed")
        h.recorder.effects.removeAll()
        h.client.send(.more(screen: 3, id: "feed", before: false))
        #expect(h.recorder.takeFetches().isEmpty, "the core asks once")

        try h.answer(more.request, with: "made/activity-2.json")
        #expect(h.client.screen(3)?.lists["feed"]?.loading == false)
        guard case .list(let feed)? = h.client.screen(3)?.screen?.body else {
            Issue.record("the activity is not a list")
            return
        }
        #expect(feed.children.compactMap(\.id) == ["e1", "e2", "e3", "e4", "e5"])
    }

    @Test func aTaskIsPresentedOverTheScreenAndSwipedAway() throws {
        let h = try Harness()
        try h.started(shell: "made/shell-routes.json")
        h.client.send(.select(destination: 1))
        try h.answer(3, with: "store/orders.json")

        h.client.send(.act(.init(screen: 2, action: .navigate(.init(href: "/orders/1/returns/new", present: .task)))))
        let modal = try #require(h.client.nav.modals.first)
        #expect(modal.present == .task)
        #expect(modal.locked == false)
        #expect(modal.stack == [3])
        #expect(h.client.nav.front == 3)
        try h.answer(4, with: "made/return-task.json")
        #expect(h.client.screen(3)?.screen != nil)

        h.client.send(.dismissed(screen: 3))
        #expect(h.client.nav.modals.isEmpty)
        #expect(h.client.screens[3] == nil)
        #expect(h.client.nav.front == 2)
    }

    @Test func anEventSentFromInsideAnEffectWaitsItsTurn() throws {
        let h = try Harness()
        var stageWhenAnswered: Stage?
        h.recorder.react = { effect, client in
            guard case .fetch(let request) = effect, request.request == 1 else { return }
            // Answered at once, from inside the effect: the start's update is not done yet.
            client.send(.response(Response(
                request: 1, status: 200, contentType: "application/vnd.navree.native+json",
                body: (try? Transcripts.fixture("store/shell.json")) ?? ""
            )))
            stageWhenAnswered = client.nav.stage
        }
        h.client.begin()
        #expect(stageWhenAnswered == .starting)
        #expect(h.client.nav.stage == .ready)
        #expect(h.recorder.takeFetches().map(\.0) == [1, 2])
    }

    @Test func aRefusedEventIsLoggedAndNothingIsDrawnFromIt() throws {
        let h = try Harness()
        h.client.begin()
        let before = h.client.fingerprint
        h.client.send(json: Data(#"{"t":"teleport"}"#.utf8), tag: "teleport")
        #expect(h.problems.list.count == 1)
        #expect(h.problems.list.first?.contains("teleport") == true)
        #expect(h.client.fingerprint == before)
    }

    @Test func aSnapshotIsTakenAndARestoredLaunchShowsWhereThePersonWas() throws {
        let h = try Harness()
        try h.started()
        h.client.send(.select(destination: 1))
        try h.answer(3, with: "store/orders.json")
        h.client.send(.act(.init(screen: 2, action: .navigate(.init(href: "/orders/1")))))
        try h.answer(4, with: "store/order.json")
        let snapshot = try #require(h.client.snapshot())

        let again = try Harness()
        again.client.begin(restoring: snapshot)
        // The shell is asked for first; the stacks come back with it.
        #expect(again.recorder.takeFetches().map(\.2) == ["https://shop.example/_navree/native/shell"])
        try again.answer(1, with: "store/shell.json")
        #expect(again.problems.list.isEmpty, "\(again.problems.list)")
        try #require(again.client.nav.destinations.count == 3)
        #expect(again.client.nav.selected == 1)
        let stack = again.client.nav.destinations[1].stack
        #expect(stack.count == 2)
        // A restored screen has its title for the bar while its page is fetched again.
        let top = try #require(stack.last.flatMap(again.client.screen))
        #expect(top.title == "Order CS-20481")
        #expect(again.client.nav.front == stack.last)
    }
}
