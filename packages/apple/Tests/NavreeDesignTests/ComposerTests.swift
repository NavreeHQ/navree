import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// A conversation's reply rests at the foot of the screen, as Messages keeps
// its field: read from what the screen holds, since the protocol has no word
// for a composer.

@Test func theInboxThreadsReplyIsItsComposerAndTheMessagesScrollWithoutIt() throws {
    let screen = try answerScreen("inbox-thread")
    let panes = try #require(SplitRules.panes(screen.body))
    let thread = SplitRules.screen(.detail, of: screen, panes: panes)
    let found = try #require(ComposerRules.composer(in: thread.body))
    #expect(found.form.id == "composer")
    #expect(found.message.name == "body")
    #expect(found.send.label == "Send")
    // What scrolls keeps the thread's head and its messages, not the reply.
    #expect(thread.body.first { $0.id == "composer" } != nil)
    #expect(found.rest.first { $0.id == "composer" } == nil)
    #expect(found.rest.first { $0.id == "thread-messages" } != nil)
    // Over the field: how it is sent, and the saved replies; never their popover.
    let others = ComposerRules.others(found)
    #expect(others.map(\.kind) == ["field", "button"])
    #expect(others.compactMap { node -> String? in if case .button(let b) = node { b.label } else { nil } } == ["Saved replies"])
    // Send waits for words.
    #expect(!ComposerRules.canSend("  \n"))
    #expect(ComposerRules.canSend("Looking into it"))
}

@Test func aFormWithManyLinesIsNoComposerWithoutAConversationBeforeIt() throws {
    let note = Navree.Node.form(Navree.Form(
        action: "/orders/1/notes",
        children: [
            .field(Navree.Field(input: .text(Navree.TextInput(kind: .multiline)), label: "Note", name: "note")),
            .button(Navree.Button(action: .submit(Navree.Action.Submit()), label: "Save", role: .primary)),
        ],
        method: .post
    ))
    let message = Navree.Node.message(Navree.Message(author: "Jasleen Kaur", body: [.text(Navree.Text(value: "Hi"))], id: "m1"))
    #expect(ComposerRules.composer(in: .stack(Navree.Stack(children: [.text(Navree.Text(value: "Order")), note]))) == nil)
    #expect(ComposerRules.composer(in: .stack(Navree.Stack(children: [message, note]))) != nil)
    // A conversation that does not end with the form keeps it where it stands.
    #expect(ComposerRules.composer(in: .stack(Navree.Stack(children: [note, message]))) == nil)
    // The store's order, whose refund form has no conversation, has none.
    #expect(ComposerRules.composer(in: try answerScreen("store-order1").body) == nil)
}
