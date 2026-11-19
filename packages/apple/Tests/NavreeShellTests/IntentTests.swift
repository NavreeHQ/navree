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
        context.send(.change(form: refund, field: "refund-amount", values: ["0"]))
        #expect(shell.client.screen(order)?.forms["refund"]?.fields["refund-amount"]?.value == .text("0"))
        #expect(context.values(refund, "refund-amount") == ["0"])

        context.send(.act(.submit(.init()), Place(form: refund, overlay: "refund")))
        let post = try #require(shell.transport.fetched.last)
        #expect(post.method == "post")
        #expect(post.url == "https://shop.example/orders/1/refund")
        // The 422 came back at once: the message is at its field, read by the drawing.
        #expect(context.errors(refund, "refund-amount") == ["Enter an amount above zero, like 24.00."])
        #expect(context.errors(refund, "refund-reason") == [])
        // A field is never found by its name.
        #expect(context.values(refund, "amount") == nil)
        #expect(shell.problems.list.isEmpty, "\(shell.problems.list)")
    }

    @Test func twoFieldsOfOneNameEachShowAndChangeTheirOwnValue() throws {
        let shell = try Shell()
        // An order's lines: a quantity for each, every one named `qty[]`.
        shell.transport.pages["/orders"] = """
            {"build": "152bd84abf4d", "navree": 1, "vocab": 0, "screen": {"title": "Order CS-20481", "body":
              {"t": "form", "id": "lines", "method": "post", "action": "/orders/1/lines", "children": [
                {"t": "field", "name": "qty[]", "label": "Phulkari dupatta", "input": {"t": "text", "value": "2"}},
                {"t": "field", "name": "qty[]", "label": "Punjabi jutti", "input": {"t": "text", "value": "1"}},
                {"t": "field", "name": "qty[]", "label": "Brass lota", "input": {"t": "text", "value": "4"}}]}}}
            """
        shell.client.begin()
        shell.client.send(.select(destination: 1))
        let order = try #require(shell.client.nav.front)
        let tree = try #require(shell.client.screen(order)?.screen)
        var fields: [Navree.Field] = []
        var found: Navree.Form?
        tree.body.walk { node in
            if case .field(let field) = node { fields.append(field) }
            if case .form(let form) = node { found = form }
        }
        let form = FormRef(try #require(found))
        // The core gave each line's field an id of its own.
        #expect(fields.count == 3)
        #expect(Set(fields.map(\.key)).count == 3)
        let context = shell.client.context(for: order, depth: 0, base: nil)
        #expect(fields.map { context.values(form, $0.key) } == [["2"], ["1"], ["4"]])

        // The person changes the third line to 5.
        context.send(.change(form: form, field: fields[2].key, values: ["5"]))

        #expect(fields.map { context.values(form, $0.key) } == [["2"], ["1"], ["5"]])
        let held = try #require(shell.client.screen(order)?.forms[form.key])
        #expect(held.fields[fields[0].key]?.value == .text("2"))
        #expect(held.fields[fields[2].key]?.value == .text("5"))
        #expect(shell.problems.list.isEmpty, "\(shell.problems.list)")
    }

    @Test func aPickedFileThatIsNotAddedIsSaidAtItsFieldByNameAndWithWhy() async throws {
        let shell = try Shell()
        // An expense claim: a field that takes several receipts, and does not ask for where they were taken.
        shell.transport.pages["/orders"] = """
            {"build": "152bd84abf4d", "navree": 1, "vocab": 0, "screen": {"title": "Expense claim", "body":
              {"t": "form", "id": "claim", "method": "post", "action": "/claims", "children": [
                {"t": "field", "name": "receipts", "label": "Receipts",
                 "input": {"t": "file", "accept": "image/*,.txt", "multiple": true}}]}}}
            """
        shell.client.begin()
        shell.client.send(.select(destination: 1))
        let claim = try #require(shell.client.nav.front)
        let tree = try #require(shell.client.screen(claim)?.screen)
        var receipts: Navree.Field?
        var found: Navree.Form?
        tree.body.walk { node in
            if case .field(let field) = node { receipts = field }
            if case .form(let form) = node { found = form }
        }
        let form = FormRef(try #require(found))
        let field = try #require(receipts).key
        let context = shell.client.context(for: claim, depth: 0, base: nil)
        #expect(context.notes(form, field).isEmpty)

        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-claim-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        // A receipt typed up, and a photo of one that does not read as a
        // picture: whether it says where it was taken cannot be told.
        let typed = folder.appendingPathComponent("taxi.txt")
        try Data("Taxi, Ludhiana to Amritsar, 2,400".utf8).write(to: typed)
        let photo = folder.appendingPathComponent("Kapurthala.jpg")
        try Data("not a picture".utf8).write(to: photo)

        context.send(.files(form: form, field: field, urls: [typed, photo]))
        await shell.effects.fileNotes.settled()

        // The receipt that could be added is the field's. The other is not
        // dropped without a word: the field says which file, and why.
        #expect(context.values(form, field) == ["taxi.txt"])
        #expect(context.notes(form, field) == [
            "Kapurthala.jpg is a kind of picture this device cannot check for the place it was taken, so it was not added."
        ])
        // It is said at that field, and at no other.
        #expect(context.notes(form, "another").isEmpty)

        // The person picks again, only what can be added: nothing is said there any more.
        context.send(.files(form: form, field: field, urls: [typed]))
        await shell.effects.fileNotes.settled()
        #expect(context.values(form, field) == ["taxi.txt"])
        #expect(context.notes(form, field).isEmpty)

        // When every file picked is refused, the field holds none and says why for each.
        context.send(.files(form: form, field: field, urls: [photo, folder.appendingPathComponent("gone.txt")]))
        await shell.effects.fileNotes.settled()
        #expect(context.values(form, field) == [])
        #expect(context.notes(form, field).count == 2)
        #expect(context.notes(form, field).last == "gone.txt could not be read and was not added.")
        // The words name a person's files: they go when the session does.
        shell.effects.run(.forgetSession, for: shell.client)
        #expect(context.notes(form, field).isEmpty)
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
        shell.client.perform(.search(form: FormRef(key: "search"), field: "q", text: "Ludhiana"), on: orders)
        #expect(shell.problems.list.isEmpty, "\(shell.problems.list)")
        // Whether the core asks now or after a pause is its own; nothing is refused.
        #expect(shell.transport.fetched.count >= before)
    }

    @Test func aFormTheServerGaveNoIdGoesByTheNameTheCoreGaveIt() {
        #expect(NativeClient.formID(nil) == nil)
        #expect(NativeClient.formID(.loose) == nil)
        // The core names such a form `@` and its address, and writes that name in the tree.
        #expect(NativeClient.formID(FormRef(key: "@/cards/1/peek")) == "@/cards/1/peek")
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

/// A screen opened from a card that zooms grows out of that card: pushed
/// from a destination's screen, or presented over the screen in front. Less
/// motion, or a screen opened from anything else, comes in as any screen does.
@MainActor
@Test func aScreenGrowsOutOfTheCardItWasOpenedFromUnlessThePersonAskedForLessMotion() throws {
    let shell = try Shell(answers: [
        "/_navree/native/shell": (200, "made/shell-health.json"),
        "/": (200, "../../../navree-native/corpus/screen-designed-health.json"),
        "/sleep": (200, "made/designed-sleep.json"),
        "/sleep/night": (200, "made/designed-sleep.json"),
        "/heart": (200, "made/designed-sleep.json"),
    ])
    shell.client.begin()
    let today = try #require(shell.client.nav.front)
    #expect(shell.client.zoomSource(of: today, reduceMotion: false) == nil)

    shell.client.perform(.act(.navigate(.init(href: "/sleep")), Place(from: "pillar-sleep")), on: today)
    let sleep = try #require(shell.client.nav.front)
    #expect(sleep != today)
    #expect(shell.client.zoomSource(of: sleep, reduceMotion: false) == "pillar-sleep")
    #expect(shell.client.zoomSource(of: sleep, reduceMotion: true) == nil)

    shell.client.perform(.act(.navigate(.init(href: "/sleep/night", present: .sheet)), Place(from: "night-card")), on: sleep)
    let night = try #require(shell.client.nav.modals.first?.stack.first)
    #expect(shell.client.zoomSource(of: night, reduceMotion: false) == "night-card")
    #expect(shell.client.zoomSource(of: night, reduceMotion: true) == nil)
    shell.client.send(.dismissed(screen: night))

    // Opened by a link of the same screen rather than by its card: a plain push.
    shell.client.send(.popTo(screen: today))
    shell.client.perform(.act(.navigate(.init(href: "/heart")), Place(from: "pillar-heart-link")), on: today)
    let heart = try #require(shell.client.nav.front)
    #expect(heart != today)
    #expect(shell.client.zoomSource(of: heart, reduceMotion: false) == nil)
    #expect(shell.problems.list.isEmpty, "\(shell.problems.list)")
}
