import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

@testable import NavreeShell

/// Conversations with the store demo, replayed from its recorded answers.
struct Store {
    static let folder = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Demo/NavreeDemo/Recorded")

    static func reply(_ name: String, status: Int = 200) throws -> OriginReply {
        .body(
            status: status, contentType: "application/vnd.navree.native+json",
            data: try Data(contentsOf: folder.appendingPathComponent("\(name).json"))
        )
    }

    static func runtime(_ restoration: Restoration = Restoration()) -> InterimRuntime {
        let origin = Origin("https://store.example.com")!
        return InterimRuntime(configuration: ShellConfiguration(
            origin: origin, base: origin.url!, appVersion: "1.0", keepsSession: false, restoration: restoration
        ))
    }
}

extension InterimRuntime {
    /// Answers every fetch among `effects` from the recorded files `names`,
    /// in order.
    mutating func answer(_ effects: [Effect], with replies: [OriginReply]) -> [Effect] {
        var next: [Effect] = []
        var replies = replies
        for case .fetch(let ticket, _) in effects where !replies.isEmpty {
            next += send(.answered(ticket, replies.removeFirst()))
        }
        return next
    }
}

func fetches(_ effects: [Effect]) -> [OriginRequest] {
    effects.compactMap { effect in
        if case .fetch(_, let request) = effect { return request }
        return nil
    }
}

@Test func theAppStartsWithTheShellThenItsFirstDestination() throws {
    var runtime = Store.runtime()
    let start = runtime.send(.start)
    #expect(fetches(start).map(\.address) == ["/_navree/native/shell"])
    let next = runtime.answer(start, with: [try Store.reply("shell")])
    #expect(runtime.state.destinations.map(\.href) == ["/", "/orders", "/inventory"])
    #expect(runtime.state.selected == "/")
    #expect(fetches(next).map(\.address) == ["/"])
}

@Test func aRestoredStackIsAskedForScreenByScreen() throws {
    var runtime = Store.runtime(Restoration(selected: "/orders", stacks: ["/orders": ["/orders/1"]]))
    let next = runtime.answer(runtime.send(.start), with: [try Store.reply("shell")])
    #expect(fetches(next).map(\.address) == ["/orders", "/orders/1"])
    _ = runtime.answer(next, with: [try Store.reply("orders"), try Store.reply("orders_1")])
    let order = try #require(runtime.state.page(PageRef(.destination("/orders"), 1)))
    #expect(order.title == "Order CS-20481")
}

@Test func aPageTheServerCantSayNativelyIsTheWebViews() throws {
    var runtime = Store.runtime()
    let next = runtime.answer(runtime.send(.start), with: [try Store.reply("shell")])
    _ = runtime.answer(next, with: [try Store.reply("index")])
    let page = try #require(runtime.state.page(PageRef(.destination("/"), 0)))
    #expect(page.content == .web(URL(string: "https://store.example.com/")!, title: "Dashboard"))
}

/// The order, loaded at the second place of the orders stack.
func orderOpen() throws -> (InterimRuntime, PageRef) {
    var runtime = Store.runtime(Restoration(selected: "/orders", stacks: ["/orders": ["/orders/1"]]))
    let next = runtime.answer(runtime.send(.start), with: [try Store.reply("shell")])
    _ = runtime.answer(next, with: [try Store.reply("orders"), try Store.reply("orders_1")])
    return (runtime, PageRef(.destination("/orders"), 1))
}

func dialog(_ id: String, in runtime: InterimRuntime, at ref: PageRef) -> Node.Dialog? {
    guard case .screen(let screen)? = runtime.state.page(ref)?.content,
        case .dialog(let dialog)? = screen.body.find(id, kinds: ["dialog"])
    else { return nil }
    return dialog
}

func field(_ name: String, in runtime: InterimRuntime, at ref: PageRef) -> Node.Field? {
    guard case .screen(let screen)? = runtime.state.page(ref)?.content else { return nil }
    let found = screen.body.first { node in
        if case .field(let field) = node { return field.name == name }
        return false
    }
    if case .field(let field)? = found { return field }
    return nil
}

@Test func aRefusedRefundShowsWhyAtItsFieldAndAnAcceptedOneClosesAndSaysSo() throws {
    var (runtime, order) = try orderOpen()
    _ = runtime.send(.intent(.act(.commands([.open(id: "refund")]), Place()), at: order))
    #expect(dialog("refund", in: runtime, at: order)?.open == true)

    let form = try #require(runtime.formRef("refund", at: order))
    _ = runtime.send(.intent(.change(form: form, name: "amount", values: ["0"]), at: order))
    let post = runtime.send(.intent(.act(.submit(form: nil, href: nil, with: nil), Place(form: form, overlay: "refund")), at: order))
    let request = try #require(fetches(post).first)
    #expect(request.method == .post)
    #expect(request.address == "/orders/1/refund")
    #expect(request.form.contains(Pair(name: "amount", value: "0")))
    #expect(request.form.contains(Pair(name: "reason", value: "returned")))

    let refused = runtime.answer(post, with: [try Store.reply("post-refund-refused", status: 422)])
    #expect(field("amount", in: runtime, at: order)?.error == "Enter an amount above zero, like 24.00.")
    #expect(refused.contains(.haptic(.error)))
    #expect(dialog("refund", in: runtime, at: order)?.open == true)

    _ = runtime.send(.intent(.change(form: form, name: "amount", values: ["10"]), at: order))
    let again = runtime.send(.intent(.act(.submit(form: nil, href: nil, with: nil), Place(form: form, overlay: "refund")), at: order))
    let accepted = runtime.answer(again, with: [try Store.reply("post-refund")])
    #expect(dialog("refund", in: runtime, at: order)?.open == false)
    #expect(field("amount", in: runtime, at: order)?.error == nil)
    #expect(runtime.state.page(order)?.notice?.title == "Refunded")
    #expect(accepted.contains(.haptic(.success)))
}

@Test func aToastThatSaysSomethingFailedIsAnAlertNotALineOfStatus() throws {
    var (runtime, order) = try orderOpen()
    let post = runtime.send(.intent(.act(.request(method: .post, href: "/orders/1/cancel", fields: []), Place(overlay: "cancel")), at: order))
    _ = runtime.answer(post, with: [try Store.reply("post-cancel")])
    #expect(runtime.state.page(order)?.failure?.title == "Couldn't cancel CS-20481")
    #expect(runtime.state.page(order)?.notice == nil)
}

@Test func aPeekFillsTheSheetAndOpensIt() throws {
    var runtime = Store.runtime(Restoration(selected: "/orders"))
    let next = runtime.answer(runtime.send(.start), with: [try Store.reply("shell")])
    _ = runtime.answer(next, with: [try Store.reply("orders")])
    let orders = PageRef(.destination("/orders"), 0)
    let peek = Node.Form.peek
    let get = runtime.send(.intent(.act(.submit(form: nil, href: nil, with: nil), Place(form: FormRef(peek))), at: orders))
    #expect(fetches(get).first?.address == "/orders/200000/peek")
    _ = runtime.answer(get, with: [try Store.reply("orders_1_peek")])
    guard case .screen(let screen)? = runtime.state.page(orders)?.content,
        case .sheet(let sheet)? = screen.body.find("order", kinds: ["sheet"])
    else {
        Issue.record("no sheet")
        return
    }
    #expect(sheet.open)
    #expect(sheet.title == "Order CS-20481")
}

@Test func theSystemsBackGestureTellsTheRuntimeWhatItTookAway() throws {
    var (runtime, order) = try orderOpen()
    #expect(runtime.state.stacks[order.container]?.count == 2)
    _ = runtime.send(.path(order.container, []))
    #expect(runtime.state.stacks[order.container]?.count == 1)
    let pushed = runtime.send(.path(order.container, [Step(address: "/orders/7", depth: 1)]))
    #expect(fetches(pushed).map(\.address) == ["/orders/7"])
    #expect(runtime.state.stacks[order.container]?.count == 2)
}

@Test func aLinkToAnotherSiteLeavesForTheSystemBrowser() throws {
    var (runtime, order) = try orderOpen()
    let effects = runtime.send(.intent(.act(.navigate(href: "https://pay.example/7", present: .push), Place()), at: order))
    #expect(effects == [.openExternal(URL(string: "https://pay.example/7")!)])
    #expect(runtime.send(.intent(.act(.navigate(href: "javascript:alert(1)", present: .push), Place()), at: order)).isEmpty)
}

@Test func aSignInPathOpensTheSystemsSignInSession() throws {
    var (runtime, order) = try orderOpen()
    let effects = runtime.send(.intent(.act(.navigate(href: "/auth/google/redirect", present: .push), Place()), at: order))
    #expect(effects == [.signIn(URL(string: "https://store.example.com/auth/google/redirect")!)])
}

@Test func aRegionThatCameEmptyIsFetchedOnceAndFilled() throws {
    var runtime = Store.runtime(Restoration(selected: "/orders"))
    let next = runtime.answer(runtime.send(.start), with: [try Store.reply("shell")])
    _ = runtime.answer(next, with: [try Store.reply("orders")])
    let orders = PageRef(.destination("/orders"), 0)
    let peek = runtime.send(.intent(.act(.submit(form: nil, href: nil, with: nil), Place(form: FormRef(Node.Form.peek))), at: orders))
    _ = runtime.answer(peek, with: [try Store.reply("orders_1_peek")])
    let steps = runtime.send(.intent(.appear(region: "order-steps"), at: orders))
    #expect(fetches(steps).map(\.address) == ["/orders/1/steps"])
    #expect(runtime.send(.intent(.appear(region: "order-steps"), at: orders)).isEmpty)
    _ = runtime.answer(steps, with: [try Store.reply("orders_1_steps")])
    guard case .screen(let screen)? = runtime.state.page(orders)?.content,
        case .region(let region)? = screen.body.find("order-steps", kinds: ["region"])
    else {
        Issue.record("no region")
        return
    }
    #expect(!region.children.isEmpty)
}

extension Node.Form {
    /// The first order's peek form, as the orders table holds it.
    static var peek: Node.Form {
        try! WireDecoder.decode(Node.Form.self, from: Data(#"{"action":"/orders/200000/peek","children":[]}"#.utf8))
    }
}
